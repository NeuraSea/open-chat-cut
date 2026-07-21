import { describe, expect, test } from "bun:test";
import { ProviderError, providerDescriptor, runProviderJob } from "../src/protocol.mjs";
import { providerJson, validateProviderOutputs } from "../src/http.mjs";
import { createSeedanceAdapter } from "../src/seedance.mjs";
import { createSunoAdapter } from "../src/suno.mjs";

function memoryStore() {
  const versions = [];
  return {
    versions,
    async update(_id, job) { versions.push(structuredClone(job)); },
  };
}

function adapter(overrides = {}) {
  return {
    descriptor: providerDescriptor({ id: "fake-provider", capabilities: ["video"] }),
    submit: async () => ({ remoteId: "remote-1" }),
    poll: async () => ({ state: "succeeded", outputs: [{ url: "https://provider/output" }] }),
    download: async () => [{ path: "/managed/sha256/file" }],
    normalize: async () => ({ result: { assetId: "asset-1" } }),
    ...overrides,
  };
}

const baseJob = {
  id: "job-1", state: "queued", progress: 0, provider: "fake-provider",
  model: "v1", prompt: "A calm product shot", seed: 42,
};

describe("durable provider protocol", () => {
  test("persists submit, poll, download, normalization and provenance", async () => {
    const store = memoryStore();
    const result = await runProviderJob({ adapter: adapter(), job: baseJob, store, sleep: async () => {} });
    expect(store.versions.map((job) => job.state)).toEqual([
      "submitting", "polling", "polling", "downloading", "normalizing", "succeeded",
    ]);
    expect(result.provenance).toMatchObject({ provider: "fake-provider", remoteId: "remote-1", seed: 42 });
  });

  test("resumes polling without submitting twice", async () => {
    let submits = 0;
    const result = await runProviderJob({
      adapter: adapter({ submit: async () => { submits += 1; return { remoteId: "wrong" }; } }),
      job: { ...baseJob, remoteId: "existing", state: "polling" },
      store: memoryStore(), sleep: async () => {},
    });
    expect(submits).toBe(0);
    expect(result.state).toBe("succeeded");
  });

  test("persists cancellation", async () => {
    const controller = new AbortController();
    controller.abort();
    const store = memoryStore();
    await expect(runProviderJob({ adapter: adapter(), job: baseJob, store, signal: controller.signal })).rejects.toMatchObject({ code: "PROVIDER_CANCELLED" });
    expect(store.versions.at(-1).state).toBe("cancelled");
  });

  test("does not retry provider 401", async () => {
    let attempts = 0;
    const fetchImpl = async () => {
      attempts += 1;
      return new Response(JSON.stringify({ error: { message: "bad key" } }), { status: 401, headers: { "content-type": "application/json" } });
    };
    await expect(providerJson({ fetchImpl, url: "https://provider.invalid/tasks", token: "secret" })).rejects.toMatchObject({ code: "PROVIDER_AUTH_FAILED", retryable: false });
    expect(attempts).toBe(1);
  });

  test("retries 429 and records retry state", async () => {
    let calls = 0;
    const store = memoryStore();
    const result = await runProviderJob({
      adapter: adapter({
        submit: async () => {
          calls += 1;
          if (calls === 1) throw new ProviderError("PROVIDER_RATE_LIMITED", "slow down", { retryable: true, retryAfterMs: 0 });
          return { remoteId: "remote-1" };
        },
      }),
      job: baseJob,
      store,
      sleep: async () => {},
    });
    expect(calls).toBe(2);
    expect(store.versions.some((job) => job.lastRetry === "PROVIDER_RATE_LIMITED")).toBe(true);
    expect(result.state).toBe("succeeded");
  });

  test("classifies request timeouts as retryable without exposing credentials", async () => {
    const fetchImpl = async (_url, init) => new Promise((_resolve, reject) => {
      init.signal.addEventListener("abort", () => reject(init.signal.reason), { once: true });
    });
    const error = await providerJson({
      fetchImpl,
      url: "https://provider.invalid/tasks",
      token: "must-not-leak",
      timeoutMs: 1,
    }).catch((caught) => caught);
    expect(error).toMatchObject({ code: "PROVIDER_TIMEOUT", retryable: true });
    expect(JSON.stringify(error)).not.toContain("must-not-leak");
  });

  test("rejects unsafe provider endpoints and output URLs by default", async () => {
    const callbacks = {
      downloadToManagedStore: async () => [],
      normalizeMedia: async () => ({ result: {} }),
    };
    expect(() => createSunoAdapter({ baseUrl: "file:///etc/passwd", ...callbacks })).toThrow();
    expect(() => createSeedanceAdapter({ baseUrl: "https://169.254.169.254/latest", ...callbacks })).toThrow("private");
    expect(() => createSeedanceAdapter({ baseUrl: "http://provider.example.com", ...callbacks })).toThrow("HTTPS");
    expect(() => validateProviderOutputs([{ url: "https://127.0.0.1/output.mp4" }])).toThrow("private");
    expect(() => validateProviderOutputs(["file:///etc/passwd"])).toThrow("HTTPS");
    await expect(providerJson({
      url: "https://[::1]/tasks",
      fetchImpl: async () => new Response("{}"),
    })).rejects.toThrow("private");
  });

  test("private compatible endpoints require an explicit opt-in", () => {
    const instance = createSeedanceAdapter({
      baseUrl: "http://127.0.0.1:9010/v1",
      allowPrivateBaseUrl: true,
      downloadToManagedStore: async () => [],
      normalizeMedia: async () => ({ result: {} }),
    });
    expect(instance.descriptor.id).toBe("seedance-compatible");
  });

  test("provider options cannot override the approved model, prompt, or seed", async () => {
    let requestBody;
    const instance = createSeedanceAdapter({
      baseUrl: "https://provider.invalid/v1",
      token: "token",
      fetchImpl: async (_url, init) => {
        requestBody = JSON.parse(init.body);
        return new Response(JSON.stringify({ id: "remote-1" }), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      },
      downloadToManagedStore: async () => [],
      normalizeMedia: async () => ({ result: {} }),
    });
    await instance.submit({
      job: {
        model: "approved-model",
        prompt: "approved prompt",
        seed: 42,
        options: { model: "attacker-model", prompt: "attacker prompt", seed: 999, duration: 5 },
      },
    });
    expect(requestBody).toEqual({ duration: 5, model: "approved-model", prompt: "approved prompt", seed: 42 });
  });

  test("normalized provenance cannot forge authoritative audit fields", async () => {
    const result = await runProviderJob({
      adapter: adapter({
        normalize: async () => ({
          result: { assetId: "asset-1" },
          provenance: { provider: "forged", prompt: "forged", remoteId: "forged", custom: "retained" },
        }),
      }),
      job: baseJob,
      store: memoryStore(),
      sleep: async () => {},
    });
    expect(result.provenance).toMatchObject({
      provider: "fake-provider",
      prompt: baseJob.prompt,
      remoteId: "remote-1",
      custom: "retained",
    });
  });
});
