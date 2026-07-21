---
name: export
description: "Validate and export OpenChatCut projects. Use for MP4, WebM, WAV, MP3, subtitle, PNG, alpha, ProRes 4444, Premiere XML, Resolve XML, selected-range output, caption burn-in, or monitoring and recovering export jobs."
---

# Export

1. Read the latest project envelope and pin its exact revision. Never export a moving latest target.
2. Call `validate_project` with the intended format. Stop for missing assets, unsafe graphics, invalid anchors, or incompatible effects.
3. For visual delivery, render representative frames before a final export when no recent verified preview exists.
4. Resolve an authorized output path. If it exists, default to a new filename; set `allowOverwrite` only after explicit confirmation.
5. Show format, revision, range, resolution, frame rate, codec/audio settings, captions, alpha behavior, and destination.
6. Call `start_export` with the pinned revision as `expectedRevision` and a new idempotency key.
7. Follow the job with `track_jobs` through success, failure, or cancellation. Report the daemon-verified output path and validation metadata.

WAV and MP3 delivery materializes the complete pinned audio timeline. Multiple
dialogue/music/SFX clips, gaps, source trims, retiming, gain, mute state, and
speech-cut boundary fades are mixed by FFmpeg; do not flatten the project to a
single source asset before export.

Do not treat job submission as delivery. Do not change revisions silently to avoid a conflict, and do not substitute a different format when the requested exporter returns `CAPABILITY_UNAVAILABLE`.
