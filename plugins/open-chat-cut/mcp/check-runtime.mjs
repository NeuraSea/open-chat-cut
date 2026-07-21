#!/usr/bin/env node
import { BridgeError, DaemonClient, redactSensitive } from "./runtime.mjs";

try {
  const status = await new DaemonClient({ timeoutMs: 5_000 }).request("get_status", {
    method: "GET",
    path: "/status",
  });
  if (!status || status.status !== "ready" || status.protocolVersion !== "1") {
    throw new BridgeError(
      "DAEMON_PROTOCOL_MISMATCH",
      "OpenChatCut daemon did not return a ready protocol-v1 status document",
      { details: redactSensitive(status) },
    );
  }
  process.stdout.write(
    `OpenChatCut daemon ready (${status.instanceId ?? "local instance"}, protocol ${status.protocolVersion}).\n`,
  );
} catch (error) {
  const code = error instanceof BridgeError ? error.code : "RUNTIME_CHECK_FAILED";
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`OpenChatCut runtime check failed [${code}]: ${message}\n`);
  process.exitCode = 1;
}
