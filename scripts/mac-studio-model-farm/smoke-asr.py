#!/usr/bin/env python3
"""Run a real, credential-safe WhisperX transcription smoke."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import secrets
import urllib.request


def multipart(audio: Path) -> tuple[bytes, str]:
    boundary = f"openchatcut-{secrets.token_hex(16)}"
    chunks: list[bytes] = []

    def field(name: str, value: str) -> None:
        chunks.extend(
            [
                f"--{boundary}\r\n".encode(),
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode(),
                value.encode(),
                b"\r\n",
            ]
        )

    field("model", "large-v3")
    field("language", "en")
    field("response_format", "verbose_json")
    chunks.extend(
        [
            f"--{boundary}\r\n".encode(),
            (
                'Content-Disposition: form-data; name="file"; '
                f'filename="{audio.name}"\r\n'
            ).encode(),
            b"Content-Type: audio/wav\r\n\r\n",
            audio.read_bytes(),
            b"\r\n",
            f"--{boundary}--\r\n".encode(),
        ]
    )
    return b"".join(chunks), boundary


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("audio", type=Path)
    parser.add_argument("--url", default="http://127.0.0.1:8191/v1/audio/transcriptions")
    parser.add_argument(
        "--token-file",
        type=Path,
        default=Path("/Volumes/External/openchatcut-models/config/asr.token"),
    )
    parser.add_argument("--timeout", type=int, default=1_800)
    arguments = parser.parse_args()

    token = arguments.token_file.read_text(encoding="utf-8").strip()
    body, boundary = multipart(arguments.audio)
    request = urllib.request.Request(
        arguments.url,
        data=body,
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": f"multipart/form-data; boundary={boundary}",
            "Accept": "application/json",
        },
    )
    with urllib.request.urlopen(request, timeout=arguments.timeout) as response:
        payload = json.load(response)
    text = payload.get("text", "") if isinstance(payload, dict) else ""
    words = payload.get("words", []) if isinstance(payload, dict) else []
    if not isinstance(text, str) or "open" not in text.lower():
        raise SystemExit("ASR smoke returned an unexpected transcript")
    if not isinstance(words, list) or not words:
        raise SystemExit("ASR smoke returned no aligned word timestamps")
    print(f"ASR inference: yes ({len(words)} aligned words)")


if __name__ == "__main__":
    main()
