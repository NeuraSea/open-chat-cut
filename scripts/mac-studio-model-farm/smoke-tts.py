#!/usr/bin/env python3
"""Run a real, credential-safe Qwen3-TTS speech smoke."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import urllib.request
import wave


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--url",
        default="http://127.0.0.1:8192/v1/audio/speech",
    )
    parser.add_argument(
        "--token-file",
        type=Path,
        default=Path("/Volumes/External/openchatcut-models/config/tts.token"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("/Volumes/External/openchatcut-models/runtime/tts-smoke.wav"),
    )
    parser.add_argument("--timeout", type=int, default=1_800)
    arguments = parser.parse_args()

    token = arguments.token_file.read_text(encoding="utf-8").strip()
    request = urllib.request.Request(
        arguments.url,
        data=json.dumps(
            {
                "model": "occ-tts",
                "input": "Open Chat Cut local voice test.",
                "voice": "alloy",
                "language": "English",
                "response_format": "wav",
            }
        ).encode(),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "Accept": "audio/wav",
        },
    )
    with urllib.request.urlopen(request, timeout=arguments.timeout) as response:
        audio = response.read()
    arguments.output.write_bytes(audio)
    with wave.open(str(arguments.output), "rb") as stream:
        frames = stream.getnframes()
        sample_rate = stream.getframerate()
        channels = stream.getnchannels()
    if frames < sample_rate // 2 or channels != 1:
        raise SystemExit("TTS smoke returned invalid or empty audio")
    print(
        f"TTS inference: yes ({frames / sample_rate:.2f}s, "
        f"{sample_rate} Hz, {channels} channel)"
    )


if __name__ == "__main__":
    main()
