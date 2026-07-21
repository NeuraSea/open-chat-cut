import { ProviderError } from "./protocol.mjs";

function normalizedHostname(url) {
  return url.hostname.toLowerCase().replace(/^\[/, "").replace(/\]$/, "").replace(/\.$/, "");
}

function ipv4Parts(hostname) {
  const parts = hostname.split(".");
  if (parts.length !== 4 || parts.some((part) => !/^\d{1,3}$/.test(part))) return null;
  const numbers = parts.map(Number);
  return numbers.some((part) => part > 255) ? null : numbers;
}

function isPrivateOrSpecialHost(hostname) {
  if (
    hostname === "localhost" || hostname.endsWith(".localhost") ||
    hostname.endsWith(".local") || hostname.endsWith(".internal") || hostname.endsWith(".home.arpa")
  ) return true;
  const ipv4 = ipv4Parts(hostname);
  if (ipv4) {
    const [a, b] = ipv4;
    return a === 0 || a === 10 || a === 127 ||
      (a === 100 && b >= 64 && b <= 127) ||
      (a === 169 && b === 254) ||
      (a === 172 && b >= 16 && b <= 31) ||
      (a === 192 && [0, 2, 168].includes(b)) ||
      (a === 198 && [18, 19, 51].includes(b)) ||
      (a === 203 && b === 0) || a >= 224;
  }
  if (hostname.includes(":")) {
    const compact = hostname.toLowerCase();
    if (compact === "::" || compact === "::1" || compact.startsWith("::ffff:") || compact.startsWith("2001:db8:") || compact.startsWith("fc") || compact.startsWith("fd") || compact.startsWith("fe8") || compact.startsWith("fe9") || compact.startsWith("fea") || compact.startsWith("feb")) return true;
    const mapped = compact.match(/::ffff:(\d+\.\d+\.\d+\.\d+)$/)?.[1];
    if (mapped && isPrivateOrSpecialHost(mapped)) return true;
  }
  return false;
}

export function validateProviderUrl(value, { allowPrivateNetwork = false, purpose = "provider URL" } = {}) {
  let url;
  try {
    url = new URL(value);
  } catch {
    throw new TypeError(`${purpose} must be an absolute URL`);
  }
  if (url.username || url.password) throw new TypeError(`${purpose} must not contain credentials`);
  if (url.protocol !== "https:" && url.protocol !== "http:") {
    throw new TypeError(`${purpose} must use HTTPS (HTTP is allowed only for an explicitly enabled private endpoint)`);
  }
  const hostname = normalizedHostname(url);
  if (!hostname) throw new TypeError(`${purpose} must contain a hostname`);
  const privateHost = isPrivateOrSpecialHost(hostname);
  if (privateHost && !allowPrivateNetwork) {
    throw new TypeError(`${purpose} must not target localhost, private, link-local, or special-use addresses`);
  }
  if (url.protocol !== "https:") {
    if (!(allowPrivateNetwork && url.protocol === "http:" && privateHost)) {
      throw new TypeError(`${purpose} must use HTTPS (HTTP is allowed only for an explicitly enabled private endpoint)`);
    }
  }
  return url;
}

function validateOutputValue(value, path, options, state, depth = 0) {
  if (depth > 12) throw new ProviderError("PROVIDER_INVALID_RESPONSE", `Provider output is too deeply nested at ${path}`);
  state.values += 1;
  if (state.values > 10_000) throw new ProviderError("PROVIDER_INVALID_RESPONSE", "Provider returned excessive output metadata");
  if (typeof value === "string") {
    if (path === "$" || /^\$outputs\[\d+\]$/u.test(path) || /(?:url|uri)$/iu.test(path)) {
      validateProviderUrl(value, { ...options, purpose: `provider output ${path}` });
    }
    return value;
  }
  if (value === null || typeof value === "number" || typeof value === "boolean") return value;
  if (Array.isArray(value)) return value.map((item, index) => validateOutputValue(item, `${path}[${index}]`, options, state, depth + 1));
  if (!value || typeof value !== "object" || (Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null)) {
    throw new ProviderError("PROVIDER_INVALID_RESPONSE", `Provider output at ${path} is not plain JSON data`);
  }
  const result = {};
  for (const [key, item] of Object.entries(value)) {
    if (["__proto__", "prototype", "constructor"].includes(key)) {
      throw new ProviderError("PROVIDER_INVALID_RESPONSE", `Provider output contains an unsafe key at ${path}.${key}`);
    }
    result[key] = validateOutputValue(item, `${path}.${key}`, options, state, depth + 1);
  }
  return result;
}

export function validateProviderOutputs(outputs, options = {}) {
  if (!Array.isArray(outputs) || outputs.length === 0 || outputs.length > 100) {
    throw new ProviderError("PROVIDER_INVALID_RESPONSE", "Provider outputs must be a non-empty bounded array");
  }
  return validateOutputValue(outputs, "$outputs", options, { values: 0 });
}

function cloneProviderOption(value, path, state, depth = 0) {
  if (depth > 12) throw new TypeError(`Provider options are too deeply nested at ${path}`);
  state.values += 1;
  if (state.values > 10_000) throw new TypeError("Provider options are too large");
  if (value === null || typeof value === "boolean" || typeof value === "string") return value;
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new TypeError(`Provider option ${path} must be finite`);
    return value;
  }
  if (Array.isArray(value)) return value.map((item, index) => cloneProviderOption(item, `${path}[${index}]`, state, depth + 1));
  if (!value || typeof value !== "object" || (Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null)) {
    throw new TypeError(`Provider option ${path} must be plain JSON data`);
  }
  const result = {};
  for (const [key, item] of Object.entries(value)) {
    const normalizedKey = key.toLowerCase().replaceAll("_", "").replaceAll("-", "");
    if (["__proto__", "prototype", "constructor"].includes(key) || ["authorization", "token", "apikey", "accesstoken"].includes(normalizedKey)) {
      throw new TypeError(`Provider option ${path}.${key} is reserved or unsafe`);
    }
    result[key] = cloneProviderOption(item, `${path}.${key}`, state, depth + 1);
  }
  return result;
}

export function providerRequestOptions(value) {
  if (value === undefined) return {};
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new TypeError("Provider options must be an object");
  const cloned = cloneProviderOption(value, "$options", { values: 0 });
  delete cloned.model;
  delete cloned.prompt;
  delete cloned.seed;
  return cloned;
}

function retryAfter(response) {
  const raw = response.headers.get("retry-after");
  if (!raw) return undefined;
  const seconds = Number(raw);
  if (Number.isFinite(seconds)) return Math.max(0, seconds * 1000);
  const date = Date.parse(raw);
  return Number.isFinite(date) ? Math.max(0, date - Date.now()) : undefined;
}

export async function providerJson({
  fetchImpl = fetch,
  url,
  token,
  method = "GET",
  body,
  signal,
  timeoutMs = 30_000,
  allowPrivateNetwork = false,
}) {
  const validatedUrl = validateProviderUrl(url, { allowPrivateNetwork });
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(new Error("timeout")), timeoutMs);
  const abort = () => controller.abort(signal?.reason);
  signal?.addEventListener("abort", abort, { once: true });
  try {
    const response = await fetchImpl(validatedUrl.toString(), {
      method,
      signal: controller.signal,
      redirect: "error",
      headers: {
        Accept: "application/json",
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const payload = await response.json().catch(() => undefined);
    if (!response.ok) {
      const status = response.status;
      throw new ProviderError(
        status === 401 || status === 403 ? "PROVIDER_AUTH_FAILED" :
          status === 429 ? "PROVIDER_RATE_LIMITED" :
            status >= 500 ? "PROVIDER_UNAVAILABLE" : "PROVIDER_REJECTED",
        payload?.error?.message ?? payload?.message ?? `Provider returned HTTP ${status}`,
        {
          status,
          retryable: status === 429 || status >= 500,
          retryAfterMs: retryAfter(response),
          details: payload?.error?.details,
        },
      );
    }
    return payload;
  } catch (error) {
    if (error instanceof ProviderError) throw error;
    if (signal?.aborted) throw new ProviderError("PROVIDER_CANCELLED", "Provider job was cancelled");
    if (controller.signal.aborted) {
      throw new ProviderError("PROVIDER_TIMEOUT", "Provider request timed out", { retryable: true });
    }
    throw new ProviderError("PROVIDER_NETWORK_ERROR", error instanceof Error ? error.message : String(error), { retryable: true });
  } finally {
    clearTimeout(timeout);
    signal?.removeEventListener("abort", abort);
  }
}
