import { providerDescriptor } from "./protocol.mjs";
import {
  providerJson,
  providerRequestOptions,
  validateProviderOutputs,
  validateProviderUrl,
} from "./http.mjs";

export function createSeedanceAdapter({
  baseUrl,
  token,
  fetchImpl,
  downloadToManagedStore,
  normalizeMedia,
  id = "seedance-compatible",
  name = "Seedance compatible",
  allowPrivateBaseUrl = false,
}) {
  const root = validateProviderUrl(baseUrl, {
    allowPrivateNetwork: allowPrivateBaseUrl,
    purpose: "Seedance baseUrl",
  });
  if (typeof downloadToManagedStore !== "function" || typeof normalizeMedia !== "function") {
    throw new TypeError("Seedance requires managed download and media normalization callbacks");
  }
  const endpoint = (path) => new URL(path.replace(/^\//, ""), root.toString().replace(/\/?$/, "/")).toString();
  return {
    descriptor: providerDescriptor({
      id, name, capabilities: ["video"], external: true, paid: true,
      configured: Boolean(token), models: ["seedance"],
    }),
    async submit({ job, signal }) {
      const payload = await providerJson({
        fetchImpl, url: endpoint("tasks"), token, method: "POST", signal, allowPrivateNetwork: allowPrivateBaseUrl,
        body: { ...providerRequestOptions(job.options), model: job.model, prompt: job.prompt, seed: job.seed },
      });
      return { remoteId: payload.id ?? payload.taskId, providerState: payload, progress: 0.01 };
    },
    async poll({ remoteId, signal }) {
      const payload = await providerJson({
        fetchImpl,
        url: endpoint(`tasks/${encodeURIComponent(remoteId)}`),
        token,
        signal,
        allowPrivateNetwork: allowPrivateBaseUrl,
      });
      const raw = String(payload.status ?? payload.state ?? "").toLowerCase();
      if (["failed", "error"].includes(raw)) return { state: "failed", error: payload.error };
      if (["succeeded", "completed", "success"].includes(raw)) {
        return { state: "succeeded", progress: 1, outputs: payload.outputs ?? payload.data?.outputs ?? [] };
      }
      return { state: "pending", progress: Number(payload.progress ?? 0.1), pollAfterMs: 1500, providerState: payload };
    },
    async download({ outputs, signal }) {
      const validatedOutputs = validateProviderOutputs(outputs, { allowPrivateNetwork: allowPrivateBaseUrl });
      return downloadToManagedStore({
        outputs: validatedOutputs,
        signal,
        maximumBytes: 2 * 1024 ** 3,
        urlPolicy: "validate-public-address-on-every-redirect",
      });
    },
    async normalize({ downloaded, signal }) {
      return normalizeMedia({ downloaded, signal, target: { videoCodec: "h264", pixelFormat: "yuv420p" } });
    },
  };
}
