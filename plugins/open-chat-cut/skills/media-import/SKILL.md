---
name: media-import
description: "Import and inspect media in OpenChatCut. Use when a user provides a local video, audio, image, or an HTTP(S) media URL; asks to add source footage or B-roll; or needs technical metadata, proxies, waveforms, or provenance checked."
---

# Import Media

## Local files

1. Call `read_project` to obtain the current revision.
2. Confirm the user-selected absolute path. Do not broaden file access or follow a different path.
3. Default to `mode: managed`, which copies content into the portable media library.
4. For an `.occproj` bundle, use `import_project_package` only after explicit confirmation; the daemon validates archive paths, the canonical envelope hash, and every media digest before creating the project.
4. Use `mode: linked` only on explicit request; warn that the project will depend on the external path, confirm the path is under a daemon-authorized root, and set `confirmLinkedRisk: true` only after the user accepts the non-portable project warning. Linked bytes are fingerprinted and fail closed if the file changes.
5. Call `import_local_media` with a new idempotency key, then `inspect_media` on the returned asset.

## Remote files

1. Show the exact URL and explain that the daemon will make a network request.
2. Obtain confirmation before calling `import_remote_media`.
3. Let the daemon enforce redirects, SSRF, MIME, and size policy. Do not download through shell as a workaround.
4. Inspect the managed result and preserve returned provenance.

## Completion

- Track a returned job with `track_jobs` until terminal state.
- Re-read the project because a successful import may advance its revision.
- Report `CAPABILITY_UNAVAILABLE` honestly; never insert an untracked path or expiring remote URL into a timeline.
