# OpenChatCut acceptance status

This document is a factual snapshot of the local v1 acceptance surface. It is
intentionally separate from the product plan: a capability is only marked
available when the repository contains the implementation and a local test can
exercise it without a paid account.

## Verified in this checkout

- Native daemon health, loopback Web shell, plugin marketplace installation and
  STDIO MCP bridge initialization.
- Media-worker capability detection is recoverable: the daemon waits up to 45
  seconds for cold FFmpeg/accelerator probes, and a transient failure starts a
  background retry that updates the live status and publishes a
  `worker.capabilities.changed` event without requiring a daemon restart. The
  live Apple Silicon daemon currently reports verified CPU and VideoToolbox
  adapters (`ffmpegAvailable: true`, selected `apple`).
- The Web Agent subscribes to that capability-change event and refreshes its
  provider/status snapshot in place, so a cold worker does not require closing
  the editor or clicking reconnect before generators become selectable.
- Fresh-browser project creation, daemon-backed manual rename, Agent
  app-server streaming, structured plan review, approval, apply, undo and redo.
- The signed-in host Codex CLI 0.144.5 has completed a real
  `initialize → thread/start → turn/start → structured empty plan` round trip
  against the live daemon (not only the fake CI app-server). Quiet app-server
  periods now expose durable progress stages for process start, protocol
  handshake, thread connection, accepted turn and periodic model-wait
  heartbeats; the integration test asserts those stages before the proposal is
  persisted, so a slow local Codex database/model response no longer looks like
  an undifferentiated frozen spinner.
- Agent-approved edits are attached to their durable SQLite session message;
  after a full editor reload, the same message still exposes Undo/Redo and the
  daemon rejects stale CAS revisions instead of silently overwriting them.
- A lower-third request is converted from a Codex intent into a server-signed
  `lower-third-signal` motion-graphic proposal (0–5 seconds), applied through
  the shared operation engine, and remains editable in the project envelope.
- Approved creative workflows now emit ordered `started → progress → completed`
  events. The Agent sidebar shows the current capability, step count, durable
  daemon handoff and then follows every returned persistent Job with live
  state/progress/message updates. It reaches a terminal success/failure state
  instead of leaving an indefinite planning spinner; the daemon integration
  test asserts the event order for a real approved export workflow and the Web
  pure-function tests cover queued, succeeded and failed jobs.
- Approved workflow metadata (proposal, pinned revision and Job IDs) is stored
  on the durable Agent session message after each dispatched capability.
  Reopening the session rehydrates the same Job records instead of losing the
  background workflow context if the daemon restarts mid-workflow.
- Informational Agent questions (for example, “你能做什么？”) return a normal
  completed reply without creating an empty transaction or an Apply card.
- WebSocket revision hydration after an Agent/MCP write, including a reload-safe
  project-name update and protection against a transient missing active scene.
- Web UI named versions: save a daemon-authoritative checkpoint, review the
  revision list (including automatic Agent checkpoints), restore a selected
  version with an explicit confirmation and revision CAS, then rehydrate the
  editor without losing newer history or editable motion graphics. The browser
  smoke covers create → edit → restore end to end; MCP `change_history` exposes
  the same named versions alongside revision entries.
- Managed media upload, content-addressed storage, project-package export,
  asynchronous job tracking, delete/import round-trip, and byte-for-byte media
  recovery.
- Generated image, video, voice, music and SFX jobs accept daemon-only
  `options.placement` metadata. After the output is downloaded and normalized,
  the worker commits it as a normal editable media item on a compatible
  Graphic/Video/Audio track, preserves an optional transcript-word anchor, and
  reports the final placement revision. Reference validation happens before a
  paid submission, worker retries are idempotent, and integration tests assert
  that placement metadata is never forwarded to the external Provider.
- Revision CAS, idempotency, operation reducer, external-revision conflict
  handling, transcript/caption adapter, motion-graphic safe IR, provider
  protocol, export shaping, security and plugin tests.
- Read-only delivery jobs are pinned to immutable historical envelopes: video,
  audio, subtitle, project-package and Premiere/Resolve XML exports continue to
  render revision N even when media derivatives or an editor commit advance the
  project head to N+1. A regression test advances the head before enqueueing an
  older export, while generation and edit jobs retain strict current-head CAS.
- The complete Bun suite (`bun test`, 297 tests) passed in the host test
  harness, including the browser-side timeline, mask, caption, migration,
  provider, MG and plugin regression tests. The current workflow change has a
  focused 3-test suite (queued, succeeded and failed Jobs) passing in the
  production Web builder. Bun's test-only preload supplies deterministic WASM
  time helpers and a text-measurement canvas shim; production builds continue
  to use the real browser APIs.
- The native media-worker suite (`python -m pytest services/media-worker/tests`,
  27 passed on this macOS host) passes across audio
  processing, export, local generation, hardware fallback, Headless Chromium,
  derivatives, protocol security and transcription fixtures.
- The paid-provider acceptance harness has four credential-free safety tests:
  it refuses missing cost confirmation, rejects symlinked private runtime
  files, stops before project creation when a provider is unavailable, and
  accepts success only when the durable job materializes a normalized
  SHA-256-backed asset. The daemon's seven provider lifecycle integration tests
  also pass (confirmation, 401/408/429, cancellation and restart recovery).
- `cargo test -p openchatcut-domain -p openchatcut-daemon` (all daemon/domain
  unit and integration suites) passes in this checkout; the focused browser
  smoke also covers the reload/recoverable-Agent path.
- The current Web source passes a clean production Docker build (including
  Next.js TypeScript checking), and the rebuilt image is the one serving the
  local editor on port 3110. The clean-cache build also verifies the slim Rust
  WASM stage installs its own download prerequisites and includes every Cargo
  workspace manifest instead of depending on stale Docker layers.
- The full cross-platform smoke (`scripts/verify-cross-platform-smoke.py`) has
  passed on this host with the isolated daemon, fake Codex app-server, plugin
  installer, MCP bridge, browser edit/reload flow, managed-media import and
  project-package round-trip.
- The release fixture passes `ffprobe` validation for a 30-second 1080p30
  H.264/AAC delivery and ProRes 4444 alpha. The live daemon → Playwright → Web
  scene graph → FFmpeg acceptance also passes for a 640×360 H.264/AAC export,
  verified preview PNG and deterministic three-frame PNG sequence.
- A real 68 MiB macOS arm64 portable archive has been produced and verified at
  both the archive and per-file SHA-256 layers. Its installer completed in an
  isolated HOME, prepared the render-only Worker, reused system Chrome, started
  the bundled daemon and standalone Web editor, created a project, released
  both ports on stop, and recovered the exact revision/document hash after a
  restart. The release matrix is configured to run the same install/restart
  smoke on macOS, Windows and Linux artifacts. Generated Python caches and archive
  AppleDouble/symlink/traversal entries are hard failures.
- Worker status now probes installed Python modules instead of equating “Worker
  process exists” with “ML model runtime exists”. A `--without-ml` install was
  verified to report faster-whisper/diarization/DeepFilterNet unavailable while
  still advertising real FFmpeg export, Playwright preview and VideoToolbox.
- The live project browser on port 3110 was checked through a real Chrome CDP
  session after the production image rebuild. It loaded four daemon projects
  without lingering skeletons and now uses daemon persistence timestamps rather
  than displaying the 1970 fallback from the versioned scene document.

The reproducible browser acceptance command is:

```sh
python3 -m pip install playwright
python3 -m playwright install chromium
python3 scripts/verify-cross-platform-smoke.py --repo "$PWD"
```

It uses a fake Codex app-server only to avoid credentials in CI. The protocol,
approval policy and project operations are the production paths.

## Requires explicit local setup

These are implemented adapters, but are not silently enabled:

- `codex login` for the real Codex app-server allowance.
- `OPENCHATCUT_MEDIA_WORKER` plus faster-whisper for transcription. pyannote
  diarization additionally needs the user-authorized model/token; otherwise
  speakers remain manually editable.
- User-owned Seedance-compatible/Volcengine or fal.ai credentials for video,
  and Suno credentials for music. Paid-provider smoke tests are opt-in.
- Kokoro/Piper voice, optional AudioGen SFX, DeepFilterNet/RNNoise models, and
  an optional stock adapter.
- A configured import root for host local-media ingestion.

Real paid-provider verification is intentionally outside the default test
matrix. `scripts/verify-paid-provider-smoke.py` and the manual-only
`OpenChatCut paid provider smoke` workflow require an exact cost confirmation,
then verify `submit → poll/resume → download → normalize` ends in a local
content-addressed asset. No paid smoke has been run in this checkout because no
user provider credentials were supplied.

The daemon returns a structured capability error (rather than fabricating a
result) when one of these optional runtimes is absent.

## Verification caveats

- The local full Rust workspace test is blocked by the machine's missing Xcode
  Metal toolchain when compiling GPUI; domain, daemon, integration, plugin,
  media-worker and Web focused tests pass independently.
- The smoke harness is platform-neutral Python/Playwright code, while the
  repository CI matrix is responsible for the final macOS/Windows/Linux fresh
  machine run. Every matrix host now runs the complete Bun suite rather than a
  selected subset. Real paid provider calls are deliberately not part of the
  default CI job.
- Running `bun test` directly inside the minimal Web builder image does not
  include the repository test preload and reports five WASM-bindgen loader
  errors; this is a test-environment limitation, not a production-build or
  workflow regression. Use the repository root test harness/CI for the full
  suite.
- A production Web image is rebuilt from the current `.next` artifact before
  the local 3110 service is started; a clean machine should use the documented
  setup script/Docker build rather than the local image cache.
