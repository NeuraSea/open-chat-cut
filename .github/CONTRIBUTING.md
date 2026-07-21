# Contributing to OpenChatCut

OpenChatCut welcomes focused bug fixes, tests, documentation, provider
adapters, and editor improvements. Do not submit ChatCut source, brand assets,
proprietary templates, or media whose license is unclear.

NeuraSea currently accepts code contributions only from contributors who have
completed its contributor licensing process. Open an issue before submitting a
code contribution. This preserves NeuraSea's ability to offer OpenChatCut under
both the Business Source License 1.1 and separate commercial terms. A pull
request by itself does not grant NeuraSea commercial relicensing rights.

## Local setup

Run the same flow as a user:

```bash
./scripts/setup.sh --without-ml
./scripts/openchatcut.sh start
```

For Web-only iteration, install Bun 1.2.18 and run `bun install` at the repo
root followed by `cd apps/web && bun run dev`. The native daemon remains the
authoritative project store even during Web development.

## Before a pull request

Run checks proportional to the change:

```bash
cargo fmt --all --check
cargo test -p openchatcut-domain -p openchatcut-daemon
node --test plugins/open-chat-cut/tests/*.test.mjs
bun test apps/web/src/subtitles/__tests__ apps/web/src/services/local-core/__tests__
bun test packages/provider-kit/test packages/mg-runtime/test
PYTHONPATH=services/media-worker/src python3 -m pytest services/media-worker/tests
```

Web changes must also pass `bun run build` from `apps/web`. Provider smoke tests
that spend money or transmit media are never part of the default test suite.

## Design rules

- Project mutations go through the shared semantic Operation Engine with a
  revision and idempotency key; never write SQLite or project JSON directly.
- Preserve stable entity IDs, immutable transcript `spokenText`, and Classic
  extension fields.
- Store generated/downloaded output as managed local content rather than a
  temporary provider URL.
- Keep the daemon loopback-only and add regression tests for new filesystem,
  URL, parser, or sandbox trust boundaries.
- Keep OpenCut Classic's MIT attribution and license new work under the root
  Business Source License 1.1 and NeuraSea's contributor licensing terms.
