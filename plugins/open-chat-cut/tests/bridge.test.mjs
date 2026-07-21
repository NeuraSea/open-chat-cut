import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { chmod, mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { BridgeError, DaemonClient, loadRuntimeDescriptor } from "../mcp/runtime.mjs";
import { handleRpc, SERVER_INSTRUCTIONS } from "../mcp/server.mjs";
import {
  TOOL_DEFINITIONS,
  buildDaemonRequest,
  validateToolArguments,
} from "../mcp/tools.mjs";

const PLUGIN_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const REPOSITORY_ROOT = path.resolve(PLUGIN_ROOT, "..", "..");

async function createRuntime(apiBaseUrl) {
  const home = await mkdtemp(path.join(os.tmpdir(), "openchatcut-plugin-"));
  const tokenPath = path.join(home, "daemon.token");
  const descriptorPath = path.join(home, "runtime.json");
  await writeFile(tokenPath, "test-token\n", "utf8");
  await writeFile(
    descriptorPath,
    JSON.stringify({ apiBaseUrl, protocolVersion: "1", tokenPath: "daemon.token" }),
    "utf8",
  );
  if (process.platform !== "win32") {
    await chmod(tokenPath, 0o600);
    await chmod(descriptorPath, 0o600);
  }
  return { home, descriptorPath };
}

async function startFakeDaemon(handler) {
  // Bun's test fetch pool can retain an idle connection after a fake server is
  // closed and the OS immediately reuses the same ephemeral port for the next
  // test. Force each response to close so one test cannot observe the previous
  // daemon handler; production daemon connections remain unaffected.
  const server = http.createServer((request, response) => {
    response.setHeader("connection", "close");
    return handler(request, response);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  return {
    server,
    apiBaseUrl: `http://127.0.0.1:${address.port}/api/v1`,
    close: () => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())),
  };
}

test("tool catalog is complete and explicitly annotated", () => {
  const names = TOOL_DEFINITIONS.map((tool) => tool.name);
  assert.equal(new Set(names).size, names.length);
  assert.deepEqual(names, [
    "get_status",
    "list_projects",
    "create_project",
    "read_project",
    "get_editor_url",
    "import_local_media",
    "import_remote_media",
    "import_project_package",
    "inspect_media",
    "search_broll",
    "process_audio",
    "validate_timeline_edit",
    "apply_timeline_edit",
    "change_history",
    "start_transcription",
    "read_script",
    "apply_script_edit",
    "edit_captions",
    "list_generators",
    "generate_asset",
    "create_motion_graphic",
    "render_preview_frames",
    "validate_project",
    "start_export",
    "track_jobs",
  ]);
  for (const definition of TOOL_DEFINITIONS) {
    assert.equal(typeof definition.annotations.readOnlyHint, "boolean");
    assert.equal(typeof definition.annotations.destructiveHint, "boolean");
    assert.equal(typeof definition.annotations.idempotentHint, "boolean");
    assert.equal(typeof definition.annotations.openWorldHint, "boolean");
  }
  assert.equal(TOOL_DEFINITIONS.find((tool) => tool.name === "generate_asset").annotations.openWorldHint, true);
  assert.equal(TOOL_DEFINITIONS.find((tool) => tool.name === "apply_script_edit").annotations.destructiveHint, true);
});

test("plugin, marketplace, MCP, and focused skill manifests stay wired together", async () => {
  const manifest = JSON.parse(await readFile(path.join(PLUGIN_ROOT, ".codex-plugin", "plugin.json"), "utf8"));
  const mcp = JSON.parse(await readFile(path.join(PLUGIN_ROOT, ".mcp.json"), "utf8"));
  const marketplace = JSON.parse(
    await readFile(path.join(REPOSITORY_ROOT, ".agents", "plugins", "marketplace.json"), "utf8"),
  );
  assert.equal(manifest.name, "open-chat-cut");
  assert.equal(manifest.mcpServers, "./.mcp.json");
  assert.deepEqual(mcp.mcpServers.openchatcut.args, ["./mcp/server.mjs", "--stdio"]);
  await readFile(path.join(PLUGIN_ROOT, "mcp", "check-runtime.mjs"), "utf8");
  assert.equal(marketplace.name, "openchatcut-local");
  assert.equal(marketplace.plugins[0].source.path, "./plugins/open-chat-cut");

  const skills = (await readdir(path.join(PLUGIN_ROOT, "skills"))).sort();
  assert.deepEqual(skills, [
    "audio",
    "basics",
    "captions",
    "export",
    "generation",
    "media-import",
    "motion-graphics",
    "speech-edit",
    "troubleshooting",
    "verification",
  ]);
  for (const skill of skills) {
    const contents = await readFile(path.join(PLUGIN_ROOT, "skills", skill, "SKILL.md"), "utf8");
    assert.doesNotMatch(contents, /\[TODO:/);
  }
});

test("tool validation and route mapping enforce revision-safe writes", () => {
  assert.throws(
    () => validateToolArguments("apply_timeline_edit", { projectId: "p1", operations: [{}] }),
    /expectedRevision is required/,
  );
  const args = {
    projectId: "project/one",
    expectedRevision: 7,
    idempotencyKey: "12345678-abcd",
    operations: [{ type: "setProjectName", name: "Edited project" }],
    proposalId: "proposal-12345678",
    confirm: true,
  };
  validateToolArguments("apply_timeline_edit", args);
  assert.deepEqual(buildDaemonRequest("apply_timeline_edit", args), {
    method: "POST",
    path: "/tools/apply_timeline_edit",
    headers: {
      "Idempotency-Key": "12345678-abcd",
      "X-OpenChatCut-Expected-Revision": "7",
    },
    body: {
      arguments: {
        projectId: "project/one",
        expectedRevision: 7,
        proposalId: "proposal-12345678",
        confirm: true,
        transaction: {
          transactionId: "tx:12345678-abcd",
          projectId: "project/one",
          baseRevision: 7,
          idempotencyKey: "12345678-abcd",
          actor: { kind: "agent", id: "codex", displayName: "Codex" },
          operations: [{ type: "setProjectName", name: "Edited project" }],
        },
      },
      idempotencyKey: "12345678-abcd",
    },
  });
});

test("MCP writes cannot spoof actors or replace project documents", () => {
  const base = {
    projectId: "p1",
    expectedRevision: 2,
    idempotencyKey: "12345678-security",
    proposalId: "proposal-security",
    confirm: true,
  };
  assert.throws(
    () => validateToolArguments("apply_timeline_edit", {
      ...base,
      operations: [{ type: "replaceDocument", document: {} }],
    }),
    /must be one of/,
  );
  assert.throws(
    () => validateToolArguments("apply_timeline_edit", {
      ...base,
      operations: [{ type: "replaceSceneGraph", scenes: [] }],
    }),
    /must be one of/,
  );
  assert.throws(
    () => validateToolArguments("apply_timeline_edit", {
      ...base,
      actor: { kind: "user", id: "owner" },
      operations: [{ type: "setProjectName", name: "Unsafe" }],
    }),
    /actor is not accepted/,
  );
  assert.throws(
    () => validateToolArguments("apply_timeline_edit", {
      ...base,
      confirm: false,
      operations: [{ type: "setProjectName", name: "Unconfirmed" }],
    }),
    /confirm must be true/,
  );
});

test("script mutations require an explicitly confirmed proposal", () => {
  const base = {
    projectId: "p1",
    expectedRevision: 2,
    idempotencyKey: "12345678-script",
  };
  validateToolArguments("apply_script_edit", {
    ...base,
    dryRun: true,
    edit: { type: "delete_words", wordIds: ["word-1"] },
  });
  assert.throws(
    () => validateToolArguments("apply_script_edit", {
      ...base,
      operations: [{ type: "setTranscriptWordsDeleted", wordIds: ["word-1"], deleted: true }],
    }),
    /proposalId is required/,
  );
  assert.throws(
    () => validateToolArguments("apply_script_edit", {
      ...base,
      proposalId: "proposal-script",
      confirm: true,
      operations: [{ type: "replaceDocument", document: {} }],
    }),
    /must be one of/,
  );
  validateToolArguments("apply_script_edit", {
    ...base,
    proposalId: "proposal-script",
    confirm: true,
    operations: [{ type: "setTranscriptWordsDeleted", wordIds: ["word-1"], deleted: true }],
  });
});

test("runtime descriptor prefers tokenPath and daemon requests stay authenticated", { concurrency: false }, async (context) => {
  let observed;
  const daemon = await startFakeDaemon(async (request, response) => {
    let body = "";
    for await (const chunk of request) body += chunk;
    observed = { url: request.url, headers: request.headers, body };
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ status: "ok", token: "must-not-leak" }));
  });
  const runtime = await createRuntime(daemon.apiBaseUrl);
  context.after(async () => {
    await daemon.close();
    await rm(runtime.home, { recursive: true, force: true });
  });

  const descriptor = await loadRuntimeDescriptor({ env: { OPENCHATCUT_HOME: runtime.home } });
  assert.equal(descriptor.token, "test-token");
  const client = new DaemonClient({
    loadDescriptor: () => loadRuntimeDescriptor({ env: { OPENCHATCUT_HOME: runtime.home } }),
  });
  const result = await client.request("get_status", buildDaemonRequest("get_status", {}));
  assert.deepEqual(result, { status: "ok", token: "[REDACTED]" });
  assert.equal(observed.url, "/api/v1/status");
  assert.equal(observed.headers.authorization, "Bearer test-token");
  assert.equal(observed.headers["x-openchatcut-protocol-version"], "1");
});

test("daemon 404 and 501 responses become honest capability errors", { concurrency: false }, async (context) => {
  const daemon = await startFakeDaemon((request, response) => {
    response.writeHead(request.url.endsWith("generate_asset") ? 501 : 404, {
      "content-type": "application/json",
    });
    response.end(JSON.stringify({ message: "not installed" }));
  });
  const runtime = await createRuntime(daemon.apiBaseUrl);
  context.after(async () => {
    await daemon.close();
    await rm(runtime.home, { recursive: true, force: true });
  });
  const descriptor = await loadRuntimeDescriptor({ env: { OPENCHATCUT_HOME: runtime.home } });
  assert.equal(descriptor.apiBaseUrl, daemon.apiBaseUrl);
  const client = new DaemonClient({
    loadDescriptor: async () => descriptor,
  });
  await assert.rejects(
    () => client.request("generate_asset", { method: "POST", path: "/tools/generate_asset", body: {} }),
    (error) => error instanceof BridgeError && error.code === "CAPABILITY_UNAVAILABLE" && error.httpStatus === 501,
  );
  const rpc = await handleRpc(
    { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "get_editor_url", arguments: { projectId: "p1" } } },
    { client },
  );
  assert.equal(rpc.result.isError, true);
  assert.equal(rpc.result.structuredContent.error.code, "CAPABILITY_UNAVAILABLE");
  assert.equal(rpc.result.structuredContent.error.daemonCode, undefined);
});

test("resource 404 preserves daemon not-found details instead of claiming a missing capability", async () => {
  const descriptor = {
    apiBaseUrl: "http://127.0.0.1:3210/api/v1",
    protocolVersion: "1",
    token: "test-token",
  };
  const client = new DaemonClient({
    loadDescriptor: async () => descriptor,
    fetch: async () => new Response(JSON.stringify({
      error: {
        code: "not_found",
        message: "project was not found",
        details: { resource: "project", id: "missing" },
      },
    }), {
      status: 404,
      headers: { "content-type": "application/json" },
    }),
  });
  await assert.rejects(
    () => client.request("read_project", { method: "GET", path: "/projects/missing" }),
    (error) =>
      error instanceof BridgeError &&
      error.code === "not_found" &&
      error.details.resource === "project" &&
      error.details.id === "missing",
  );
});

test("dispatcher result envelopes are not double wrapped", async () => {
  const client = {
    async request() {
      return {
        ok: true,
        proposal: { proposalId: "proposal-1", baseRevision: 4 },
      };
    },
  };
  const rpc = await handleRpc(
    {
      jsonrpc: "2.0",
      id: 9,
      method: "tools/call",
      params: {
        name: "validate_timeline_edit",
        arguments: {
          projectId: "p1",
          expectedRevision: 4,
          operations: [{ type: "setProjectName", name: "Edited project" }],
        },
      },
    },
    { client },
  );
  assert.deepEqual(rpc.result.structuredContent, {
    ok: true,
    proposal: { proposalId: "proposal-1", baseRevision: 4 },
  });
});

test("stdio server initializes, lists tools, and proxies status", { concurrency: false }, async (context) => {
  const daemon = await startFakeDaemon((request, response) => {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ status: request.url === "/api/v1/status" ? "ready" : "unexpected" }));
  });
  const runtime = await createRuntime(daemon.apiBaseUrl);
  const child = spawn(process.execPath, [path.join(PLUGIN_ROOT, "mcp", "server.mjs"), "--stdio"], {
    cwd: PLUGIN_ROOT,
    env: { ...process.env, OPENCHATCUT_HOME: runtime.home },
    stdio: ["pipe", "pipe", "pipe"],
  });
  context.after(async () => {
    child.kill();
    await daemon.close();
    await rm(runtime.home, { recursive: true, force: true });
  });

  const messages = [];
  let buffered = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    buffered += chunk;
    const lines = buffered.split("\n");
    buffered = lines.pop();
    for (const line of lines) if (line.trim()) messages.push(JSON.parse(line));
  });

  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26" } })}\n`);
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} })}\n`);
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "get_status", arguments: {} } })}\n`);

  const deadline = Date.now() + 5_000;
  while (messages.length < 3 && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  assert.equal(messages.length, 3);
  assert.equal(messages[0].result.serverInfo.name, "openchatcut");
  assert.equal(messages[1].result.tools.length, TOOL_DEFINITIONS.length);
  assert.equal(messages[2].result.structuredContent.ok, true);
  assert.equal(messages[2].result.structuredContent.data.status, "ready");
  child.stdin.end();
});

test("embedded runtime tokens require private descriptor permissions", async (context) => {
  if (process.platform === "win32") context.skip("POSIX permission test");
  const home = await mkdtemp(path.join(os.tmpdir(), "openchatcut-plugin-insecure-"));
  context.after(() => rm(home, { recursive: true, force: true }));
  await mkdir(home, { recursive: true });
  const descriptorPath = path.join(home, "runtime.json");
  await writeFile(
    descriptorPath,
    JSON.stringify({ apiBaseUrl: "http://127.0.0.1:3210/api/v1", protocolVersion: "1", token: "secret" }),
  );
  await chmod(descriptorPath, 0o644);
  await assert.rejects(
    () => loadRuntimeDescriptor({ env: { OPENCHATCUT_HOME: home } }),
    (error) => error instanceof BridgeError && error.code === "INSECURE_RUNTIME_PERMISSIONS",
  );
});

test("server instructions isolate prompt-like project content", () => {
  assert.match(SERVER_INSTRUCTIONS, /transcripts, subtitles, OCR/i);
  assert.match(SERVER_INSTRUCTIONS, /untrusted project data/i);
});
