# OpenChatCut daemon

`openchatcutd` is the host-native authority for local projects. It binds only to
loopback, stores projects and revisions in SQLite WAL mode, and exposes the same
semantic transaction API to the Web editor and the Codex MCP bridge. It does not
pretend that transcription, generation, or export workers exist: the status
document reports those capabilities as unavailable until a real adapter is
installed.

## Start and discovery

```sh
cargo run --manifest-path apps/daemon/Cargo.toml --bin openchatcutd
```

Defaults (override with the corresponding `OPENCHATCUT_*` environment variable):

| Setting | Default |
| --- | --- |
| `--bind` | `127.0.0.1:3210` |
| home | `~/.openchatcut` (`OPENCHATCUT_HOME`) |
| data | `~/.openchatcut/data` |
| descriptor | `~/.openchatcut/runtime.json` |
| bearer token | `~/.openchatcut/daemon.token` |
| editor URL | `http://127.0.0.1:3100` (`OPENCHATCUT_EDITOR_URL`) |
| worker editor URL | editor URL (`OPENCHATCUT_WORKER_EDITOR_URL`; internal Compose origin only in explicit constrained-container mode) |
| media worker | disabled (`OPENCHATCUT_MEDIA_WORKER=/absolute/path/to/openchatcut-media-worker`) |
| Codex CLI | `codex` when executable (`OPENCHATCUT_CODEX_COMMAND=/absolute/path/to/codex`) |
| authorized imports | none (`--authorized-import-root /absolute/directory`, repeatable; or comma-separated `OPENCHATCUT_IMPORT_ROOTS`) |

`OPENCHATCUT_CONTAINERIZED=true` is reserved for the repository's `full-cpu`
Compose profile. It permits only an unspecified container bind and explicitly
configured internal HTTP origins; Compose still publishes ports to host loopback.
Native launches continue to reject every non-loopback bind/origin.

The private runtime descriptor contains `protocolVersion`, `instanceId`,
`apiBaseUrl`, `tokenPath`, `pid`, and `startedAt`. It never embeds the token. A
native client reads `tokenPath`, trims the one trailing newline, and sends:

```http
Authorization: Bearer <token>
```

Every route except `GET /health`, browser bootstrap, and CORS preflight requires
authentication. The listener rejects non-loopback binds and requests whose Host
is not `localhost`, `127.0.0.1`, or another loopback address.

## Browser session and CORS

Only `http://127.0.0.1:3100` and `http://localhost:3100` are allowed by default.
Additional origins must be explicit loopback HTTP origins. The Web editor starts
a short session without learning the daemon token:

```http
POST /api/v1/session/bootstrap
Host: 127.0.0.1:3210
Origin: http://127.0.0.1:3100
```

The response sets an HttpOnly, SameSite=Strict `openchatcut_session` cookie and
returns a short-lived CSRF value:

```json
{
  "csrfToken": "64 lowercase hex characters",
  "expiresAt": "2026-07-15T10:15:00Z"
}
```

Browser writes must include the allowed `Origin`, the session cookie, and
`X-OpenChatCut-CSRF: <csrfToken>`. Native bearer-token clients do not use CSRF.

## HTTP API

All JSON fields use camelCase. Errors have one stable shape:

```json
{
  "error": {
    "code": "revisionConflict",
    "message": "the project changed after this edit was prepared",
    "details": { "expectedRevision": 4, "currentRevision": 5 }
  }
}
```

### Projects and transactions

```http
GET  /api/v1/status
GET  /api/v1/projects
POST /api/v1/projects
GET  /api/v1/projects/{projectId}
DELETE /api/v1/projects/{projectId}
POST /api/v1/projects/{projectId}/transactions/validate
POST /api/v1/projects/{projectId}/transactions
GET  /api/v1/projects/{projectId}/revisions?limit=100
GET  /api/v1/projects/{projectId}/revisions/{revision}
POST /api/v1/projects/{projectId}/undo
POST /api/v1/projects/{projectId}/redo
```

Undo and redo accept `{ expectedRevision, idempotencyKey }`. Each navigation is
an append-only CAS revision, survives daemon restart, and moves one complete
transaction (including an Agent batch) at a time. A normal edit after undo
starts a new branch and clears the redo stack.

Every transaction whose actor is `agent` also creates a named snapshot of its
base revision in the same SQLite transaction. The returned `agentCheckpoint`
can be restored through the normal version API; an Agent edit can therefore
never commit without its pre-edit recovery point.

Create a project (omit `projectId` to generate one). Send the stable
`Idempotency-Key: create-product-demo-1` header; the body form is also accepted
for browser callers:

```json
{
  "name": "Product demo",
  "projectId": "018f3f40-40d2-7cb4-8f18-51acb8c79c48",
  "idempotencyKey": "create-product-demo-1"
}
```

The transaction validation and commit routes accept the shared Rust domain
`EditTransaction` directly. The exact operation payload is defined by
`openchatcut-domain`; the stable outer payload is:

```json
{
  "transactionId": "018f3f41-5e3f-7ce9-b812-44f432ec25c0",
  "projectId": "018f3f40-40d2-7cb4-8f18-51acb8c79c48",
  "baseRevision": 0,
  "idempotencyKey": "agent-turn-28-edit-1",
  "actor": { "kind": "agent", "id": "codex", "displayName": "Codex" },
  "operations": [{ "type": "setProjectName", "name": "Product demo — short" }]
}
```

A successful commit returns `envelope`, `inverseOperations`, `changes`, and
`replayed`. A retry with the same project/key/fingerprint returns the original
receipt with `replayed: true`. Reusing the key for different input or committing
against a stale revision returns HTTP 409. The project row, revision snapshot,
and receipt are committed atomically.

### Agent Auto-Apply policy

```http
POST /api/v1/projects/{projectId}/settings/auto-apply
```

The body is `{ "expectedRevision": 8, "enabled": true, "idempotencyKey":
"auto-apply-1" }`. The setting is CAS-checked against the current document
revision and does not itself create a document revision. When enabled, the
Agent may commit only the Rust-domain allowlist of reversible mechanical
operations (caption style/text/speaker corrections, track/property changes,
safe moves, and gap closure). Deletions, content replacement, timing cuts,
scene-graph replacement, external generation, and export always remain
proposal-and-confirmation flows. Every automatic commit still creates one
normal revision with an inverse operation, so it can be undone atomically.

Delete is also CAS/idempotent and unlinks project rows without eagerly deleting
content-addressed media (safe GC handles unreferenced bytes later):

```json
{ "expectedRevision": 8, "idempotencyKey": "delete-project-1" }
```

### Safe asset GC

```http
POST /api/v1/maintenance/media-gc
```

The default `{ "confirm": false, "minAgeHours": 24 }` performs a read-only
inventory. Pass `confirm: true` only after reviewing the candidate hashes and
bytes. GC never considers current documents, any retained revision, named
versions, or queued/running job checkpoints collectible; it refuses destructive
execution while jobs are active, rechecks each hash immediately before unlink,
and never follows unexpected content-store symlinks. The one-hour minimum grace
also protects newly installed bytes from racing a project commit.

### Named versions

```http
GET  /api/v1/projects/{projectId}/versions
POST /api/v1/projects/{projectId}/versions
POST /api/v1/projects/{projectId}/restore
```

Create:

```json
{
  "name": "Before short-form pass",
  "expectedRevision": 7,
  "idempotencyKey": "version-before-short-1"
}
```

Restore (restoring creates a new revision; it never rewinds or deletes history):

```json
{
  "versionId": "8a39b8e6-4784-42d3-badc-67ab2a1bd125",
  "expectedRevision": 9,
  "idempotencyKey": "restore-before-short-1"
}
```

### Durable jobs and events

```http
GET  /api/v1/jobs?projectId={projectId}&limit=100
GET  /api/v1/jobs/{jobId}
POST /api/v1/jobs/{jobId}/cancel
GET  /api/v1/events/ws           # WebSocket (editor default)
GET  /api/v1/events              # text/event-stream compatibility
```

Job creation is intentionally not a generic public endpoint. A real capability
must validate a purpose-built request before enqueueing work. This prevents an
API shape that claims a provider or worker is available when it is not. The SSE
stream publishes project commits/restores and job cancellation requests; a slow
subscriber receives an explicit `stream.lagged` event.

### Remote video and music generation

Remote video/music providers are configured only in the daemon-private
`~/.openchatcut/providers.json` (or `OPENCHATCUT_PROVIDER_CONFIG`). The Web and
MCP bridge never read this file. On Unix it must be a regular non-symlink file
with mode `0600`:

```json
{
  "seedanceCompatible": {
    "baseUrl": "https://your-seedance-compatible-provider.example/v1",
    "apiKey": "user-owned-key",
    "defaultModel": "provider-model",
    "submitPath": "tasks",
    "pollPathTemplate": "tasks/{id}"
  },
  "suno": {
    "baseUrl": "https://your-suno-compatible-provider.example/v1",
    "apiKey": "user-owned-key",
    "submitPath": "generations",
    "pollPathTemplate": "generations/{id}"
  }
}
```

The top-level alternatives are `seedance`, `seedanceCompatible`, `fal`, and
`suno`. Each entry accepts `baseUrl`, `apiKey`, optional `defaultModel`, and
optional relative `submitPath`/`pollPathTemplate`. The poll template must use
`{id}` where the provider task identifier belongs. Private-network-compatible
endpoints are rejected unless the entry explicitly sets
`allowPrivateBaseUrl: true`; public endpoints are DNS-pinned and every output
download is revalidated. Restart the daemon after changing the file and verify
the redacted availability through `list_generators` before submitting work.

New API's standard asynchronous video endpoint has a dedicated
`newApiVideo` adapter. When `baseUrl` ends in `/v1`, its default submit and poll
paths are `video/generations` and `video/generations/{id}`:

```json
{
  "newApiVideo": {
    "baseUrl": "https://api.example/v1",
    "apiKeyKeychain": {
      "account": "openchatcut",
      "service": "new-api-token"
    },
    "defaultModel": "occ-video"
  }
}
```

The adapter accepts New API's `task_id`, persists it for restart/resume,
downloads the completed `url`, and applies the same MIME, size, SSRF, and local
FFmpeg normalization checks as every other generated video provider.

No normal test submits a paid request. Use
`scripts/verify-paid-provider-smoke.py` only with the documented exact cost
confirmation; it validates that the final output is local and
content-addressed.

### Codex image generation

When the configured Codex CLI is executable, `list_generators` reports
`codex-image` as available with model `gpt-image-2`. `generate_asset` creates a
durable `codex_image_generation` job and delegates authentication to the user's
existing `codex login`; the daemon never locates, reads, or copies Codex
credential files. The call still requires `confirm: true` because it consumes
the signed-in Codex allowance and sends the approved prompt to the service.

Each job launches `codex app-server --stdio` in a private per-job directory,
disables Web search and MCP servers, grants workspace write only to that
directory, disables sandbox network access, and declines every client approval
request. Only a completed `imageGeneration.savedPath` is accepted. The daemon
rejects relative/escaping paths, symlinks, non-files, oversized output, active
markup, and magic-byte/type mismatches before installing the bytes in the
SHA-256 media store and committing an `AssetProvenance::Generated` revision.
The original and revised prompts, provider, model, parameters, requesting
revision, job ID, and allowance provenance remain editable project metadata.

After the image is saved, its relative path, digest, size, and MIME type are
checkpointed in SQLite. A daemon restart resumes local import from that verified
checkpoint without invoking image generation or consuming allowance twice.
Cancellation kills the isolated app-server process; generated source URLs are
never stored as project dependencies.

### Isolated URL capture

`generate_asset` exposes `kind: "webCapture"` through the built-in
`local-web-capture` adapter. It always requires confirmation because the daemon
contacts the approved public URL. Rust resolves and pins only public addresses,
revalidates every redirect, accepts explicit HTML MIME types, bounds the page to
4 MiB, parses image candidates with an HTML parser, and downloads at most eight
signature-validated public images under per-file and aggregate limits. Query
strings are removed from project provenance.

Downloaded HTML and images are checkpointed before rendering. The media worker
receives only local staged paths and opens the HTML in an independent
`about:blank` origin with JavaScript, service workers, downloads, and network
access disabled; a restrictive CSP and request abort handler provide additional
defense. The daemon independently validates the PNG result and bounded extracted
title, selling points, colors, and dimensions. One atomic project revision adds
the screenshot and any downloaded public images as managed assets, explicitly
marking page text as `untrustedPublicWeb`. Restart resumes from verified local
digests without contacting the page again, while cancellation removes staging
artifacts.

When `OPENCHATCUT_MEDIA_WORKER` resolves to an executable, the real
`start_transcription` dispatcher tool is enabled. It requires a revision-pinned
managed audio/video asset and queues an idempotent `transcription` job:

```json
{
  "arguments": {
    "projectId": "project-1",
    "assetId": "asset-dialogue",
    "expectedRevision": 4,
    "language": "auto",
    "engine": "faster-whisper",
    "diarization": false
  },
  "idempotencyKey": "transcribe-dialogue-1"
}
```

The daemon derives the source path from the asset content hash, verifies the
stored SHA-256 before use, and never lets a caller redirect the worker to an
arbitrary host file. It starts the worker with a
sanitized environment, sends one JSON request on stdin, consumes bounded JSONL
progress/result/error events, and persists every state transition. Interrupted
`running` jobs return to `queued` on startup. Cancelling a running job kills its
child process. Successful word-timestamp output is converted into the shared
`TranscriptDocument` and committed as an `UpsertTranscript` revision. Unrelated
project edits can be safely rebased while the worker runs; a changed source or
concurrently edited transcript fails materialization instead of overwriting it.
`transcription` remains `false` in `/status` and the tool returns a structured
501 until a worker is configured. The optional diarization installation enables
pyannote speaker turns when a Hugging Face token/model grant is present; without
that grant, stable speaker fields remain manually editable.

### Agent providers

Codex is the default Agent and delegates authentication only to `codex login`.
The private daemon provider file may additionally configure OpenAI-compatible
or Ollama Chat Completions endpoints without exposing keys to the Web editor or
MCP bridge:

```json
{
  "openaiCompatible": {
    "baseUrl": "https://provider.example/v1",
    "model": "planning-model",
    "apiKeyKeychain": {
      "account": "openchatcut",
      "service": "singularity-x-new-api-token"
    }
  },
  "ollama": {
    "baseUrl": "http://127.0.0.1:11434/v1",
    "model": "qwen3",
    "allowPrivateBaseUrl": true
  }
}
```

On Unix the file must be mode `0600`. Public endpoints use DNS-pinned clients
and private addresses are rejected unless `allowPrivateBaseUrl` is explicitly
enabled. Non-Codex planning always requires `confirmExternal=true` after showing
which pinned project/transcript/caption/asset metadata will be sent. Returned
JSON is parsed into typed semantic operations, checked by the same domain
reducer as manual edits, and remains a dry-run proposal until separately
approved. External providers do not receive contact sheets or source video.

Provider credentials may use exactly one of `apiKey`, `apiKeyEnv`, or the
macOS-only `apiKeyKeychain` object shown above. Environment variable names are
restricted to uppercase ASCII shell identifiers. Keychain lookup invokes
`/usr/bin/security` with only the account and service names; the resolved secret
is held in daemon memory and is never returned to the Web editor, MCP bridge, or
logs.

### Managed local media

`import_local_media` is disabled until at least one import root is explicitly
authorized. Managed imports require an absolute regular-file path beneath such
a root. The daemon opens the selected file without following a final symlink,
checks the opened inode against the authorized canonical path, enforces an 8 GiB
per-file bound, rejects active SVG/HTML and known-extension signature spoofing,
then streams SHA-256-addressed bytes into the private media library. Explicit
`mode=linked` imports require `confirmLinkedRisk=true`, remain restricted to the
same authorized roots, store only a path plus immutable fingerprint in project
state, and mark validation as non-portable. Every read reopens without following
the final symlink and verifies the fingerprint; changed files fail closed until
they are relinked. Exports may stage verified bytes into the private worker cache,
but portable project packages still require a managed copy.

The project revision and idempotent transaction are preflighted before content
installation. Failed CAS writes remove newly installed unreferenced content.
Existing digest paths are opened without following links and fully rehashed;
same-size pre-seeding, hard-link mutation, and symlink escape are treated as
integrity failures. `inspect_media` verifies the managed-content integrity/size
and queues a durable, sanitized `ffprobe` inspection when the media worker is
available; without a worker it returns truthful `notProbed` fields.

With the worker enabled, every new video/audio/image import also queues a
durable `media_derivatives` job. The controlled FFmpeg runner creates applicable
thumbnail, 12-frame contact sheet, waveform, 720p editing proxy, and video-audio
FLAC outputs. It also records bounded representative-frame and scene-change
timestamps under `mediaAnalysis`. The daemon
accepts only exact private output paths with bounded sizes and expected magic
bytes, installs them under SHA-256, and updates the source asset in one system
revision. Clients read them through:

```http
GET /api/v1/projects/{projectId}/assets/{assetId}/derivatives/{thumbnail|contactSheet|waveform|proxy|audio}
```

Derivative hashes are protected by GC and included in portable project packages.
For Agent planning, the daemon copies at most eight managed contact sheets (or
thumbnail fallbacks) into the private Codex turn directory and attaches them as
low-detail `localImage` inputs. Codex receives their stable asset-ID mapping,
the project transcript, and sanitized media/audio metadata; it never receives
the source video or a temporary remote URL, and visible prompt-like text is
explicitly treated as untrusted image content.

### Shared tool dispatcher

Codex and the Web editor may use `POST /api/v1/tools/{toolName}`. The canonical
wire envelope is `{ "arguments": {...}, "idempotencyKey": "..." }`. The key is
required for mutating tools; raw argument objects remain temporarily accepted
for read-only calls. Implemented tools and concrete request bodies are:

```jsonc
// get_status
{ "arguments": {} }

// read_project
{ "arguments": { "projectId": "..." } }

// get_editor_url
{ "arguments": { "projectId": "..." } }

// validate_timeline_edit
{
  "arguments": {
    "projectId": "...",
    "expectedRevision": 7,
    "operations": [{ "type": "setProjectName", "name": "Reviewed name" }]
  },
  "idempotencyKey": "validate-edit-8"
}

// apply_timeline_edit (must match the server-side proposal exactly)
{
  "arguments": {
    "projectId": "...",
    "expectedRevision": 7,
    "proposalId": "proposal:...",
    "confirm": true,
    "operations": [/* exact proposal.payload */]
  },
  "idempotencyKey": "edit-8"
}

// change_history
{ "arguments": { "projectId": "...", "limit": 100 } }

// track_jobs (one job, or a filtered list when jobId is absent)
{ "arguments": { "jobId": "..." } }
{ "arguments": { "projectId": "...", "limit": 100 } }
```

Validation stores an expiring server-side proposal bound to purpose, project,
base revision, and normalized operations. Apply requires `confirm=true`; actor
spoofing, payload swaps, and whole-document/whole-scene-graph replacement are
rejected on the MCP/tool path. Direct project transactions retain the privileged
replacement operations for explicit Classic migration.

`read_script` returns both the shared domain transcript and the Web shell's
utterance/millisecond view. `apply_script_edit` uses the same proposal gate and
supports word/segment deletion, transcript splitting, speaker/display-text
changes, materialized StorySequence gap closing, clip/utterance/word-anchor
reordering, and `add_captions`. Multiple edits are validated and committed as
one revision, so “remove filler words, tighten pauses, add captions” is one
reviewable and exactly undoable transaction. Deletions create frame-aligned
real A/V cuts, and reorder/gap edits move every timeline item in the affected
`linkGroupId` together.

Aliases `status`, `validate`, `apply`, `history`, and `jobs` are accepted.
Any other name returns HTTP 501 with `capability_not_implemented` and the
implemented tool list; it never returns fabricated output.

Every successful dispatcher response has one predictable envelope. Specialized
route response shapes never leak into the caller contract:

```json
{
  "ok": true,
  "data": { "envelope": {} },
  "revision": 8
}
```

`data` is present for the implemented tools; a mutating tool also returns the
resulting top-level `revision`. Future proposal and queued-worker tools may add
top-level `proposal` and `jobId` without changing this envelope.

## On-disk layout

```text
data/
  openchatcut.sqlite3       # projects, heads, transactions, revisions, versions, jobs
  media/sha256/ab/cdef...   # immutable content-addressed source media
  derived/sha256/ab/cdef... # proxies, waveforms, thumbnails, cleaned audio
  projects/
  exports/
  tmp/
```

Content writes use a private temporary file plus atomic create-if-absent hard
link and never overwrite an existing digest. Every existing content address is
rehash-verified before use. SQLite enables foreign keys, WAL, NORMAL synchronous
mode, and a five-second busy timeout.
