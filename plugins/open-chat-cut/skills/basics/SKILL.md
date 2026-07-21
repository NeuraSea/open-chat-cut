---
name: basics
description: "Connect to the local OpenChatCut daemon and manage projects. Use for OpenChatCut setup checks, listing or creating projects, reading project state, opening the editor, or beginning any multi-step local video-editing workflow."
---

# OpenChatCut Basics

Use the MCP tools as the only project-control surface. Never read or edit the daemon database, runtime token, or project JSON directly.

## Start a workflow

1. Call `get_status`. Treat unavailable workers or capabilities as real constraints.
2. Call `list_projects`, then select a project from user context. Do not guess an ID.
3. Call `read_project` and retain its `revision` and `documentHash` for planning.
4. Call `get_editor_url` only when the user wants to inspect the project in the Web editor.

## Create a project

1. Choose a human-readable name from the request.
2. Generate one unique `idempotencyKey` for the logical create request.
3. Call `create_project`, then `read_project` using the returned ID.
4. Reuse the key only for an exact retry after a timeout; use a new key for changed input.

## State rules

- Re-read the project after every successful write and before planning the next write.
- Pass the current revision as `expectedRevision`; never bypass a conflict.
- If a tool returns `CAPABILITY_UNAVAILABLE`, state which capability the running daemon lacks. Do not simulate success with shell or direct file changes.
- Keep read-only checks automatic. Follow the focused skill for approval rules before edits, external transfers, generation, or export.
