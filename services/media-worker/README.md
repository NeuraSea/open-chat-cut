# OpenChatCut media worker

This optional native worker runs local ML and FFmpeg jobs for `openchatcutd`. It
communicates with JSON on stdin/stdout, never opens a network listener, and can
only access paths below the daemon-provided data root.

Base installation has no heavyweight Python dependencies:

```bash
python -m venv .venv
.venv/bin/pip install -e ./services/media-worker
```

Enable local transcription when needed:

```bash
.venv/bin/pip install -e './services/media-worker[transcription]'
```

Every transformation creates a derived asset; source audio is never replaced.
Provider credentials remain in the daemon/provider environment and are not
forwarded to worker subprocesses.

`openchatcut-media-worker --probe-capabilities` performs real one-frame encoder
smoke tests and prints the selected `auto`, `cpu`, `apple`, or `nvidia` H.264
path as JSON. Set `OPENCHATCUT_VIDEO_ACCELERATION` to choose a preference.
Apple/NVIDIA selections never silently use a vendor encoder that failed its
smoke test; the report records the verified CPU fallback reason.

WAV and MP3 delivery uses the `timeline_audio_export` job. The daemon supplies
a revision-pinned plan and managed input paths; FFmpeg preserves clip placement,
gaps, trims, retiming, gain, mute state, and equal-power story-cut fades, then
atomically installs an exact-duration 48 kHz mix without starting Chromium.

Managed video/audio/image imports automatically run `inspect_media` and
`prepare_media`. The latter creates applicable JPEG thumbnails, 12-frame JPEG
contact sheets, PNG waveforms, 720p H.264/AAC editing proxies, and FLAC
video-audio derivatives. Video analysis also records bounded representative
frame times and FFmpeg scene-change timestamps; it never uploads or passes the
whole video to a model. The daemon validates and content-addresses every output
before exposing it to clients.

For `process_audio` denoise jobs, `engine=auto` first uses the optional
DeepFilterNet dependency and falls back to deterministic FFmpeg `afftdn` on
CPU-only installs. `engine=rnnoise` uses FFmpeg `arnndn` with the explicitly
installed daemon-local `models/rnnoise/model.rnnn`; callers cannot supply an
arbitrary model path. Every engine writes a reversible derived WAV.
