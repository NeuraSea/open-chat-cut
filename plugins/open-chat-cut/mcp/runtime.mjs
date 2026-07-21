import { constants as fsConstants } from "node:fs";
import { access, readFile, stat } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const DEFAULT_API_BASE_URL = "http://127.0.0.1:3210/api/v1";
const MAX_DESCRIPTOR_BYTES = 64 * 1024;
const MAX_TOKEN_BYTES = 16 * 1024;
const DEFAULT_RESPONSE_BYTES = 32 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS = 30_000;
const SENSITIVE_KEY = /(authorization|cookie|password|secret|token|api[-_]?key|credential)/i;

export class BridgeError extends Error {
  constructor(code, message, options = {}) {
    super(message);
    this.name = "BridgeError";
    this.code = code;
    this.httpStatus = options.httpStatus;
    this.daemonCode = options.daemonCode;
    this.retryable = options.retryable ?? false;
    this.details = options.details;
    this.cause = options.cause;
  }
}

function positiveInteger(value, fallback, minimum, maximum) {
  const parsed = Number.parseInt(value ?? "", 10);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(maximum, Math.max(minimum, parsed));
}

function unique(values) {
  return [...new Set(values.filter(Boolean).map((value) => path.resolve(value)))];
}

export function runtimeDescriptorCandidates({
  env = process.env,
  platform = process.platform,
  homeDirectory = os.homedir(),
} = {}) {
  if (env.OPENCHATCUT_RUNTIME_DESCRIPTOR) {
    return [path.resolve(env.OPENCHATCUT_RUNTIME_DESCRIPTOR)];
  }
  if (env.OPENCHATCUT_HOME) {
    const configured = path.resolve(env.OPENCHATCUT_HOME);
    return [configured.endsWith(".json") ? configured : path.join(configured, "runtime.json")];
  }

  if (platform === "darwin") {
    return unique([
      path.join(homeDirectory, "Library", "Application Support", "OpenChatCut", "runtime.json"),
      path.join(homeDirectory, ".openchatcut", "runtime.json"),
    ]);
  }
  if (platform === "win32") {
    return unique([
      env.APPDATA && path.join(env.APPDATA, "OpenChatCut", "runtime.json"),
      env.LOCALAPPDATA && path.join(env.LOCALAPPDATA, "OpenChatCut", "runtime.json"),
      path.join(homeDirectory, "AppData", "Roaming", "OpenChatCut", "runtime.json"),
      path.join(homeDirectory, ".openchatcut", "runtime.json"),
    ]);
  }

  return unique([
    env.XDG_STATE_HOME && path.join(env.XDG_STATE_HOME, "openchatcut", "runtime.json"),
    env.XDG_CONFIG_HOME && path.join(env.XDG_CONFIG_HOME, "openchatcut", "runtime.json"),
    env.XDG_DATA_HOME && path.join(env.XDG_DATA_HOME, "openchatcut", "runtime.json"),
    path.join(homeDirectory, ".local", "state", "openchatcut", "runtime.json"),
    path.join(homeDirectory, ".config", "openchatcut", "runtime.json"),
    path.join(homeDirectory, ".openchatcut", "runtime.json"),
  ]);
}

async function firstReadableFile(candidates) {
  for (const candidate of candidates) {
    try {
      await access(candidate, fsConstants.R_OK);
      return candidate;
    } catch {
      // Continue to the next platform-standard location.
    }
  }
  return undefined;
}

async function readBoundedFile(filePath, maximumBytes, label) {
  let fileStat;
  try {
    fileStat = await stat(filePath);
  } catch (cause) {
    throw new BridgeError("RUNTIME_FILE_UNAVAILABLE", `${label} is not readable`, {
      cause,
      details: { path: filePath },
    });
  }
  if (!fileStat.isFile()) {
    throw new BridgeError("RUNTIME_FILE_INVALID", `${label} must be a regular file`, {
      details: { path: filePath },
    });
  }
  if (fileStat.size > maximumBytes) {
    throw new BridgeError("RUNTIME_FILE_INVALID", `${label} exceeds the allowed size`, {
      details: { path: filePath, maximumBytes },
    });
  }
  return { contents: await readFile(filePath, "utf8"), stat: fileStat };
}

function assertPrivatePermissions(filePath, fileStat, platform, label) {
  if (platform === "win32") return;
  if ((fileStat.mode & 0o077) !== 0) {
    throw new BridgeError("INSECURE_RUNTIME_PERMISSIONS", `${label} must not be readable by group or other users`, {
      details: {
        path: filePath,
        remediation: `chmod 600 ${filePath}`,
      },
    });
  }
}

function normalizeLoopbackApiBase(rawValue) {
  let parsed;
  try {
    parsed = new URL(rawValue || DEFAULT_API_BASE_URL);
  } catch {
    throw new BridgeError("RUNTIME_DESCRIPTOR_INVALID", "apiBaseUrl must be a valid loopback URL");
  }
  const hostname = parsed.hostname.toLowerCase().replace(/^\[|\]$/g, "");
  if (!["127.0.0.1", "localhost", "::1"].includes(hostname)) {
    throw new BridgeError("RUNTIME_DESCRIPTOR_INVALID", "apiBaseUrl must target the local loopback interface", {
      details: { hostname },
    });
  }
  if (!["http:", "https:"].includes(parsed.protocol)) {
    throw new BridgeError("RUNTIME_DESCRIPTOR_INVALID", "apiBaseUrl must use HTTP or HTTPS");
  }
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    throw new BridgeError(
      "RUNTIME_DESCRIPTOR_INVALID",
      "apiBaseUrl must not contain credentials, query parameters, or fragments",
    );
  }
  return parsed.toString().replace(/\/$/, "");
}

export async function loadRuntimeDescriptor(options = {}) {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const candidates = runtimeDescriptorCandidates({
    env,
    platform,
    homeDirectory: options.homeDirectory ?? os.homedir(),
  });
  const descriptorPath = await firstReadableFile(candidates);
  if (!descriptorPath) {
    throw new BridgeError(
      "RUNTIME_NOT_CONFIGURED",
      "OpenChatCut runtime descriptor was not found; start openchatcutd or set OPENCHATCUT_HOME",
      { details: { searched: candidates } },
    );
  }

  const descriptorFile = await readBoundedFile(
    descriptorPath,
    MAX_DESCRIPTOR_BYTES,
    "OpenChatCut runtime descriptor",
  );
  let descriptor;
  try {
    descriptor = JSON.parse(descriptorFile.contents);
  } catch {
    throw new BridgeError("RUNTIME_DESCRIPTOR_INVALID", "OpenChatCut runtime descriptor is not valid JSON", {
      details: { path: descriptorPath },
    });
  }
  if (!descriptor || typeof descriptor !== "object" || Array.isArray(descriptor)) {
    throw new BridgeError("RUNTIME_DESCRIPTOR_INVALID", "OpenChatCut runtime descriptor must contain an object");
  }
  if (typeof descriptor.protocolVersion !== "string" || descriptor.protocolVersion.trim() === "") {
    throw new BridgeError("RUNTIME_DESCRIPTOR_INVALID", "runtime descriptor is missing protocolVersion");
  }

  let token;
  if (typeof descriptor.tokenPath === "string" && descriptor.tokenPath.trim() !== "") {
    const tokenPath = path.resolve(path.dirname(descriptorPath), descriptor.tokenPath);
    const tokenFile = await readBoundedFile(tokenPath, MAX_TOKEN_BYTES, "OpenChatCut daemon token");
    assertPrivatePermissions(tokenPath, tokenFile.stat, platform, "OpenChatCut daemon token");
    token = tokenFile.contents.trim();
  } else if (typeof descriptor.token === "string" && descriptor.token.trim() !== "") {
    assertPrivatePermissions(
      descriptorPath,
      descriptorFile.stat,
      platform,
      "Runtime descriptor containing an embedded token",
    );
    token = descriptor.token.trim();
  } else {
    throw new BridgeError(
      "RUNTIME_DESCRIPTOR_INVALID",
      "runtime descriptor must contain tokenPath or an embedded token",
    );
  }
  if (!token) throw new BridgeError("RUNTIME_DESCRIPTOR_INVALID", "OpenChatCut daemon token is empty");

  return {
    apiBaseUrl: normalizeLoopbackApiBase(descriptor.apiBaseUrl),
    protocolVersion: descriptor.protocolVersion.trim(),
    token,
    descriptorPath,
  };
}

export function redactSensitive(value, seen = new WeakSet()) {
  if (Array.isArray(value)) return value.map((item) => redactSensitive(item, seen));
  if (!value || typeof value !== "object") return value;
  if (seen.has(value)) return "[Circular]";
  seen.add(value);
  const sanitized = {};
  for (const [key, item] of Object.entries(value)) {
    sanitized[key] = SENSITIVE_KEY.test(key) ? "[REDACTED]" : redactSensitive(item, seen);
  }
  return sanitized;
}

async function readLimitedResponse(response, maximumBytes) {
  if (!response.body) return undefined;
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > maximumBytes) {
      await reader.cancel();
      throw new BridgeError("DAEMON_RESPONSE_TOO_LARGE", "OpenChatCut daemon response exceeded the bridge limit", {
        details: { maximumBytes },
      });
    }
    chunks.push(value);
  }
  if (total === 0) return undefined;
  const joined = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    joined.set(chunk, offset);
    offset += chunk.byteLength;
  }
  const text = new TextDecoder().decode(joined);
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function daemonMessage(payload, fallback) {
  if (payload && typeof payload === "object") {
    if (typeof payload.message === "string" && payload.message.trim()) return payload.message;
    if (payload.error && typeof payload.error.message === "string" && payload.error.message.trim()) {
      return payload.error.message;
    }
  }
  if (typeof payload === "string" && payload.trim() && payload.length <= 1000) return payload;
  return fallback;
}

function httpError(capability, requestPath, status, payload) {
  const daemonError = payload && typeof payload === "object" && payload.error && typeof payload.error === "object"
    ? payload.error
    : undefined;
  const daemonCode = typeof daemonError?.code === "string" ? daemonError.code : undefined;
  const details = redactSensitive(daemonError?.details ?? payload);
  const missingCapability =
    status === 501 ||
    (status === 404 && (
      requestPath.startsWith("/tools/") ||
      daemonCode === "route_not_found" ||
      daemonCode === "capability_not_implemented"
    ));
  if (missingCapability) {
    return new BridgeError(
      "CAPABILITY_UNAVAILABLE",
      `The running OpenChatCut daemon does not provide ${capability}`,
      { httpStatus: status, daemonCode, details, retryable: false },
    );
  }
  if (status === 404) {
    return new BridgeError(daemonCode ?? "NOT_FOUND", daemonMessage(payload, "The requested OpenChatCut resource was not found"), {
      httpStatus: status,
      daemonCode,
      details,
    });
  }
  if (status === 401 || status === 403) {
    return new BridgeError("DAEMON_AUTH_FAILED", "The OpenChatCut daemon rejected the runtime token", {
      httpStatus: status,
      daemonCode,
      details,
    });
  }
  if (status === 409) {
    return new BridgeError(
      "REVISION_CONFLICT",
      daemonMessage(payload, "The project changed after this operation was planned; read and validate the latest revision"),
      { httpStatus: status, daemonCode, details },
    );
  }
  if (status === 429) {
    return new BridgeError("DAEMON_RATE_LIMITED", daemonMessage(payload, "The daemon or provider is rate limited"), {
      httpStatus: status,
      daemonCode,
      details,
      retryable: true,
    });
  }
  if (status >= 500) {
    return new BridgeError("DAEMON_ERROR", daemonMessage(payload, "The OpenChatCut daemon failed the request"), {
      httpStatus: status,
      daemonCode,
      details,
      retryable: true,
    });
  }
  return new BridgeError(daemonCode ?? "DAEMON_REQUEST_REJECTED", daemonMessage(payload, "The daemon rejected the request"), {
    httpStatus: status,
    daemonCode,
    details,
  });
}

export class DaemonClient {
  constructor(options = {}) {
    this.loadDescriptor = options.loadDescriptor ?? (() => loadRuntimeDescriptor());
    this.fetch = options.fetch ?? globalThis.fetch;
    if (typeof this.fetch !== "function") {
      throw new BridgeError("RUNTIME_UNSUPPORTED", "OpenChatCut MCP requires Node.js 18+, Bun, or another runtime with fetch");
    }
    this.timeoutMs = options.timeoutMs ?? positiveInteger(
      process.env.OPENCHATCUT_MCP_TIMEOUT_MS,
      DEFAULT_TIMEOUT_MS,
      1_000,
      300_000,
    );
    this.maximumResponseBytes = options.maximumResponseBytes ?? positiveInteger(
      process.env.OPENCHATCUT_MCP_MAX_RESPONSE_BYTES,
      DEFAULT_RESPONSE_BYTES,
      1024,
      128 * 1024 * 1024,
    );
  }

  async request(capability, request) {
    const descriptor = await this.loadDescriptor();
    if (!request.path.startsWith("/") || request.path.startsWith("//")) {
      throw new BridgeError("BRIDGE_ROUTE_INVALID", "Internal daemon route must be relative to apiBaseUrl");
    }
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), this.timeoutMs);
    const headers = {
      Accept: "application/json",
      Authorization: `Bearer ${descriptor.token}`,
      "User-Agent": "OpenChatCut-Codex-Bridge/0.1.0",
      "X-OpenChatCut-Protocol-Version": descriptor.protocolVersion,
      ...request.headers,
    };
    let body;
    if (request.body !== undefined) {
      headers["Content-Type"] = "application/json";
      body = JSON.stringify(request.body);
    }

    let response;
    let payload;
    try {
      response = await this.fetch(`${descriptor.apiBaseUrl}${request.path}`, {
        method: request.method,
        headers,
        body,
        signal: controller.signal,
        redirect: "error",
      });
      payload = await readLimitedResponse(response, this.maximumResponseBytes);
    } catch (cause) {
      if (cause instanceof BridgeError) throw cause;
      if (controller.signal.aborted) {
        throw new BridgeError("DAEMON_TIMEOUT", "Timed out waiting for the local OpenChatCut daemon", {
          retryable: true,
          cause,
        });
      }
      throw new BridgeError("DAEMON_UNAVAILABLE", "Could not connect to the local OpenChatCut daemon", {
        retryable: true,
        cause,
      });
    } finally {
      clearTimeout(timeout);
    }

    if (!response.ok) throw httpError(capability, request.path, response.status, payload);
    return redactSensitive(payload ?? null);
  }
}

export function bridgeErrorPayload(capability, error) {
  const normalized = error instanceof BridgeError
    ? error
    : new BridgeError("BRIDGE_ERROR", error instanceof Error ? error.message : String(error));
  return {
    ok: false,
    error: {
      code: normalized.code,
      message: normalized.message,
      capability,
      retryable: normalized.retryable,
      ...(normalized.httpStatus === undefined ? {} : { httpStatus: normalized.httpStatus }),
      ...(normalized.daemonCode === undefined ? {} : { daemonCode: normalized.daemonCode }),
      ...(normalized.details === undefined ? {} : { details: redactSensitive(normalized.details) }),
    },
  };
}
