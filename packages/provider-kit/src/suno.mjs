import { providerDescriptor } from "./protocol.mjs";
import {
  providerJson,
  providerRequestOptions,
  validateProviderOutputs,
  validateProviderUrl,
} from "./http.mjs";

export function createSunoAdapter({
  baseUrl,
  token,
  fetchImpl,
  downloadToManagedStore,
  normalizeMedia,
  allowPrivateBaseUrl = false,
}) {
  const root = validateProviderUrl(baseUrl, {
    allowPrivateNetwork: allowPrivateBaseUrl,
    purpose: "Suno baseUrl",
  });
  if (typeof downloadToManagedStore !== "function" || typeof normalizeMedia !== "function") {
    throw new TypeError("Suno requires managed download and media normalization callbacks");
  }
  const endpoint = (path) => new URL(path.replace(/^\//, ""), root.toString().replace(/\/?$/, "/")).toString();
  return {
    descriptor: providerDescriptor({
      id: "suno", name: "Suno", capabilities: ["music"], external: true, paid: true,
      configured: Boolean(token), models: [],
    }),
    async submit({ job, signal }) {
      const payload = await providerJson({
        fetchImpl, url: endpoint("generations"), token, method: "POST", signal, allowPrivateNetwork: allowPrivateBaseUrl,
        body: { ...providerRequestOptions(job.options), prompt: job.prompt, model: job.model, seed: job.seed },
      });
      return { remoteId: payload.id ?? payload.taskId, providerState: payload };
    },
    async poll({ remoteId, signal }) {
      const payload = await providerJson({
        fetchImpl,
        url: endpoint(`generations/${encodeURIComponent(remoteId)}`),
        token,
        signal,
        allowPrivateNetwork: allowPrivateBaseUrl,
      });
      const raw = String(payload.status ?? "").toLowerCase();
      if (["failed", "error"].includes(raw)) return { state: "failed", error: payload.error };
      if (["succeeded", "completed"].includes(raw)) return { state: "succeeded", outputs: payload.outputs ?? [] };
      return { state: "pending", progress: Number(payload.progress ?? 0.1), pollAfterMs: 2000 };
    },
    download: ({ outputs, signal }) => downloadToManagedStore({
      outputs: validateProviderOutputs(outputs, { allowPrivateNetwork: allowPrivateBaseUrl }),
      signal,
      maximumBytes: 512 * 1024 ** 2,
      urlPolicy: "validate-public-address-on-every-redirect",
    }),
    normalize: ({ downloaded, signal }) => normalizeMedia({ downloaded, signal, target: { audioCodec: "flac", sampleRate: 48_000 } }),
  };
}
