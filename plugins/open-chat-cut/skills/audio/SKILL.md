---
name: audio
description: "Clean, derive, and mix OpenChatCut audio. Use for dialogue denoise, loudness normalization, compression, music ducking, looping, crossfades, source inspection, or reversible replacement of a timeline audio source."
---

# Audio Cleanup and Mixing

1. Read the project and call `inspect_media` for each relevant source.
2. Explain the proposed chain and preserve the source. Prefer dialogue cleanup before loudness and mix operations.
3. Use `process_audio` to create a derived asset; never overwrite original media.
4. Pass the latest revision and one new idempotency key per logical operation. Track asynchronous work with `track_jobs`.
5. Re-read/inspect the derived asset before using it. Confirm duration, channels, sample rate, provenance, and source relationship.
6. Validate any timeline replacement as a semantic edit, show the diff, then apply it only under the project's approval policy.
7. Preview dialogue starts, cuts, crossfades, music entrances, and ducking recovery. Validate the project before export.

Do not invent a cleaned asset when DeepFilterNet, RNNoise, or another requested capability is unavailable. Preserve a reversible path to the original at every step.
