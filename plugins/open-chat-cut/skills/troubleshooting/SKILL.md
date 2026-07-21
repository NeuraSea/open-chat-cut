---
name: troubleshooting
description: "Diagnose OpenChatCut Codex plugin failures. Use for missing runtime descriptors, daemon connection or authentication errors, protocol mismatch, unavailable capabilities, revision conflicts, rejected paths or URLs, provider failures, stuck jobs, invalid motion graphics, or plugin installation problems."
---

# Troubleshooting

Start with `get_status` when the bridge can connect. Preserve the structured error code and avoid speculative fixes.

## Error handling

- `RUNTIME_NOT_CONFIGURED`: start `openchatcutd` or point `OPENCHATCUT_HOME` at its app-data directory. Do not create a fake descriptor.
- `INSECURE_RUNTIME_PERMISSIONS`: restrict the named token/descriptor file to the current user. Never print its contents.
- `DAEMON_UNAVAILABLE` or `DAEMON_TIMEOUT`: verify the local daemon is running and retry a read before retrying a write.
- `DAEMON_AUTH_FAILED`: restart/re-register the daemon runtime. Do not read, copy, or expose the token.
- `CAPABILITY_UNAVAILABLE`: the running daemon returned 404/501 for that tool. Report it and offer only genuinely available alternatives.
- `REVISION_CONFLICT`: read the newest project, rebuild and revalidate the plan, then obtain approval again if the diff changed.
- `DAEMON_RATE_LIMITED`: inspect the job/provider state and honor retry guidance; never submit duplicate paid work with a new key.

## Safe retries

Reuse an idempotency key only for the exact same logical write after an ambiguous timeout. Create a new key when any input changes. Use `track_jobs` before resubmitting transcription, generation, processing, preview, or export work.

Never work around bridge errors by opening SQLite, editing project JSON, disabling loopback validation, downloading remote media in shell, or executing rejected JSX outside the sandbox.
