export class ProviderError extends Error {
  constructor(code, message, options = {}) {
    super(message);
    this.name = "ProviderError";
    this.code = code;
    this.status = options.status;
    this.retryable = options.retryable ?? false;
    this.retryAfterMs = options.retryAfterMs;
    this.details = options.details;
  }
}

export function providerDescriptor(value) {
  if (!value || typeof value !== "object") throw new TypeError("Provider descriptor is required");
  if (!/^[a-z0-9][a-z0-9._-]{1,80}$/.test(value.id ?? "")) throw new TypeError("Invalid provider id");
  if (!Array.isArray(value.capabilities) || value.capabilities.length === 0) throw new TypeError("Provider capabilities are required");
  return Object.freeze({
    id: value.id,
    name: value.name ?? value.id,
    capabilities: Object.freeze([...value.capabilities]),
    models: Object.freeze([...(value.models ?? [])]),
    external: value.external ?? true,
    paid: value.paid ?? false,
    configured: value.configured ?? false,
  });
}

function abortError() {
  return new ProviderError("PROVIDER_CANCELLED", "Provider job was cancelled", { retryable: false });
}

function assertNotAborted(signal) {
  if (signal?.aborted) throw abortError();
}

async function withRetry(operation, options) {
  const { signal, sleep, maxAttempts, onRetry } = options;
  let attempt = 0;
  while (true) {
    assertNotAborted(signal);
    attempt += 1;
    try {
      return await operation(attempt);
    } catch (error) {
      const normalized = normalizeProviderError(error);
      if (!normalized.retryable || attempt >= maxAttempts) throw normalized;
      const wait = normalized.retryAfterMs ?? Math.min(30_000, 500 * (2 ** (attempt - 1)));
      await onRetry?.({ attempt, waitMs: wait, error: normalized });
      await sleep(wait, signal);
    }
  }
}

export function normalizeProviderError(error) {
  if (error instanceof ProviderError) return error;
  if (error?.name === "AbortError") return abortError();
  return new ProviderError("PROVIDER_REQUEST_FAILED", error instanceof Error ? error.message : String(error), {
    retryable: true,
  });
}

export async function defaultSleep(milliseconds, signal) {
  await new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, milliseconds);
    signal?.addEventListener("abort", () => {
      clearTimeout(timer);
      reject(abortError());
    }, { once: true });
  });
}

/**
 * Durable provider protocol: submit -> poll/resume -> download -> normalize.
 * `store.update` must commit each phase before the next remote side effect.
 */
export async function runProviderJob({
  adapter,
  job,
  store,
  signal,
  sleep = defaultSleep,
  maxAttempts = 5,
  onProgress,
}) {
  let current = { ...job };
  const persist = async (patch) => {
    current = { ...current, ...patch, updatedAt: new Date().toISOString() };
    await store.update(current.id, current);
    await onProgress?.(current);
  };

  try {
    assertNotAborted(signal);
    if (!current.remoteId) {
      await persist({ state: "submitting", progress: Math.max(0, current.progress ?? 0) });
      const submitted = await withRetry(
        () => adapter.submit({ job: current, signal }),
        { signal, sleep, maxAttempts, onRetry: ({ error }) => persist({ lastRetry: error.code }) },
      );
      if (!submitted?.remoteId) throw new ProviderError("PROVIDER_INVALID_RESPONSE", "Provider did not return a remote job id");
      await persist({
        remoteId: submitted.remoteId,
        providerState: submitted.providerState,
        state: "polling",
        progress: Math.max(0.01, submitted.progress ?? 0.01),
      });
    }

    let pollResult;
    while (true) {
      assertNotAborted(signal);
      pollResult = await withRetry(
        () => adapter.poll({ job: current, remoteId: current.remoteId, providerState: current.providerState, signal }),
        { signal, sleep, maxAttempts, onRetry: ({ error }) => persist({ lastRetry: error.code }) },
      );
      if (!pollResult || !["pending", "succeeded", "failed"].includes(pollResult.state)) {
        throw new ProviderError("PROVIDER_INVALID_RESPONSE", "Provider returned an invalid poll state");
      }
      if (pollResult.state === "failed") {
        throw new ProviderError(
          pollResult.error?.code ?? "PROVIDER_GENERATION_FAILED",
          pollResult.error?.message ?? "Provider generation failed",
          { retryable: false, details: pollResult.error?.details },
        );
      }
      await persist({
        providerState: pollResult.providerState ?? current.providerState,
        progress: Math.max(current.progress ?? 0, Math.min(0.9, pollResult.progress ?? 0.1)),
      });
      if (pollResult.state === "succeeded") break;
      await sleep(pollResult.pollAfterMs ?? 1500, signal);
    }

    await persist({ state: "downloading", progress: Math.max(0.91, current.progress ?? 0) });
    const downloaded = await withRetry(
      () => adapter.download({ job: current, outputs: pollResult.outputs ?? [], signal }),
      { signal, sleep, maxAttempts, onRetry: ({ error }) => persist({ lastRetry: error.code }) },
    );
    await persist({ state: "normalizing", progress: 0.97, downloaded });
    const normalized = await adapter.normalize({ job: current, downloaded, signal });
    const providerProvenance = normalized?.provenance && typeof normalized.provenance === "object" && !Array.isArray(normalized.provenance)
      ? normalized.provenance
      : {};
    const provenance = {
      ...providerProvenance,
      provider: adapter.descriptor.id,
      model: current.model,
      prompt: current.prompt,
      seed: current.seed,
      remoteId: current.remoteId,
      generatedAt: new Date().toISOString(),
    };
    await persist({ state: "succeeded", progress: 1, result: normalized.result, provenance, error: undefined });
    return current;
  } catch (error) {
    const normalized = normalizeProviderError(error);
    await persist({
      state: normalized.code === "PROVIDER_CANCELLED" ? "cancelled" : "failed",
      error: { code: normalized.code, message: normalized.message, retryable: normalized.retryable, details: normalized.details },
    });
    throw normalized;
  }
}
