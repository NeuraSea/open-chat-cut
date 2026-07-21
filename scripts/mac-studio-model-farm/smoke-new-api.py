#!/usr/bin/env python3
"""Non-destructive New API/LM Studio model-farm smoke checks.

Secrets are read from macOS Keychain and are never printed. Paid image and
video generation calls are intentionally excluded; this script checks
authentication, stable model aliases, and real planner, embedding, rerank,
private TTS, and private word-aligned ASR inference through New API.
"""

from __future__ import annotations

import json
from io import BytesIO
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
import wave

NEW_API = "https://api.singularity-x.ai:9443/v1"
EXPECTED_ALIASES = {
    "occ-edit-planner",
    "occ-edit-fast",
    "occ-vision",
    "occ-embedding",
    "occ-rerank",
    "occ-asr",
    "occ-image",
    "occ-video",
    "occ-tts",
    "occ-music",
    "occ-sfx",
    "occ-denoise",
}


def keychain(account: str, service: str) -> str:
    return subprocess.check_output(
        [
            "/usr/bin/security",
            "find-generic-password",
            "-a",
            account,
            "-s",
            service,
            "-w",
        ],
        text=True,
        stderr=subprocess.DEVNULL,
    ).rstrip("\r\n")


def get_json(url: str, token: str) -> object:
    request = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
            "User-Agent": "OpenChatCut-model-farm-smoke/0.1",
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        if response.status != 200:
            raise RuntimeError(f"unexpected HTTP status {response.status}")
        return json.load(response)


def post_json(path: str, token: str, payload: object) -> tuple[object, float]:
    request = urllib.request.Request(
        f"{NEW_API}/{path}",
        data=json.dumps(payload).encode(),
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
            "Content-Type": "application/json",
            "User-Agent": "OpenChatCut-model-farm-smoke/0.1",
        },
    )
    started = time.monotonic()
    with urllib.request.urlopen(request, timeout=600) as response:
        if response.status != 200:
            raise RuntimeError(f"unexpected HTTP status {response.status}")
        return json.load(response), time.monotonic() - started


def post_audio(path: str, token: str, payload: object) -> tuple[bytes, float]:
    request = urllib.request.Request(
        f"{NEW_API}/{path}",
        data=json.dumps(payload).encode(),
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "audio/wav",
            "Content-Type": "application/json",
            "User-Agent": "OpenChatCut-model-farm-smoke/0.1",
        },
    )
    started = time.monotonic()
    with urllib.request.urlopen(request, timeout=1_800) as response:
        if response.status != 200:
            raise RuntimeError(f"unexpected HTTP status {response.status}")
        return response.read(), time.monotonic() - started


def post_transcription(token: str, audio: bytes) -> tuple[object, float]:
    boundary = f"openchatcut-{uuid.uuid4().hex}"
    fields = {
        "model": "occ-asr",
        "language": "en",
        "response_format": "verbose_json",
    }
    chunks: list[bytes] = []
    for name, value in fields.items():
        chunks.extend(
            [
                f"--{boundary}\r\n".encode(),
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode(),
                value.encode(),
                b"\r\n",
            ]
        )
    chunks.extend(
        [
            f"--{boundary}\r\n".encode(),
            b'Content-Disposition: form-data; name="file"; filename="voice.wav"\r\n',
            b"Content-Type: audio/wav\r\n\r\n",
            audio,
            b"\r\n",
            f"--{boundary}--\r\n".encode(),
        ]
    )
    request = urllib.request.Request(
        f"{NEW_API}/audio/transcriptions",
        data=b"".join(chunks),
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
            "Content-Type": f"multipart/form-data; boundary={boundary}",
            "User-Agent": "OpenChatCut-model-farm-smoke/0.1",
        },
    )
    started = time.monotonic()
    with urllib.request.urlopen(request, timeout=1_800) as response:
        if response.status != 200:
            raise RuntimeError(f"unexpected HTTP status {response.status}")
        return json.load(response), time.monotonic() - started


def main() -> int:
    try:
        token = keychain("openchatcut", "singularity-x-new-api-token")
    except subprocess.CalledProcessError:
        print("missing Keychain item: singularity-x-new-api-token", file=sys.stderr)
        return 2

    try:
        payload = get_json(f"{NEW_API}/models", token)
    except (urllib.error.URLError, ValueError, RuntimeError) as error:
        print(f"New API model listing failed: {type(error).__name__}", file=sys.stderr)
        return 1

    data = payload.get("data", []) if isinstance(payload, dict) else []
    models = {
        item.get("id")
        for item in data
        if isinstance(item, dict) and isinstance(item.get("id"), str)
    }
    missing = sorted(EXPECTED_ALIASES - models)
    print(f"New API authenticated: yes; model aliases: {len(models)}")
    if missing:
        print("missing aliases: " + ", ".join(missing))
        return 3
    print("all OpenChatCut aliases are authorized")

    try:
        chat, elapsed = post_json(
            "chat/completions",
            token,
            {
                "model": "occ-edit-fast",
                "messages": [
                    {"role": "user", "content": "Reply exactly OPENCHATCUT_OK."}
                ],
                "temperature": 0,
                # GLM 4.7 Flash may spend more than 128 tokens in
                # reasoning_content before emitting its visible response.
                "max_tokens": 512,
                "stream": False,
            },
        )
        choices = chat.get("choices", []) if isinstance(chat, dict) else []
        message = choices[0].get("message", {}) if choices else {}
        content = message.get("content", "") if isinstance(message, dict) else ""
        if "OPENCHATCUT_OK" not in content:
            raise RuntimeError("fast planner returned an unexpected response")
        print(f"fast planner inference: yes ({elapsed:.2f}s)")

        embeddings, elapsed = post_json(
            "embeddings",
            token,
            {"model": "occ-embedding", "input": ["OpenChatCut asset search"]},
        )
        rows = embeddings.get("data", []) if isinstance(embeddings, dict) else []
        vector = rows[0].get("embedding", []) if rows else []
        if not isinstance(vector, list) or len(vector) < 128:
            raise RuntimeError("embedding endpoint returned no useful vector")
        print(f"embedding inference: yes ({len(vector)} dimensions, {elapsed:.2f}s)")

        rerank, elapsed = post_json(
            "rerank",
            token,
            {
                "model": "occ-rerank",
                "query": "local video editing",
                "documents": [
                    "cloud billing platform",
                    "offline video editor with captions",
                ],
                "top_n": 2,
            },
        )
        results = rerank.get("results", []) if isinstance(rerank, dict) else []
        if not results or results[0].get("index") != 1:
            raise RuntimeError("rerank endpoint returned an invalid ordering")
        print(f"rerank inference: yes ({elapsed:.2f}s)")

        speech, elapsed = post_audio(
            "audio/speech",
            token,
            {
                "model": "occ-tts",
                "input": "Open Chat Cut local voice test.",
                "voice": "alloy",
                "language": "English",
                "response_format": "wav",
            },
        )
        with wave.open(BytesIO(speech), "rb") as stream:
            frames = stream.getnframes()
            sample_rate = stream.getframerate()
            channels = stream.getnchannels()
        if frames < sample_rate // 2 or channels != 1:
            raise RuntimeError("TTS endpoint returned invalid or empty audio")
        print(f"TTS inference: yes ({frames / sample_rate:.2f}s, {elapsed:.2f}s)")

        transcription, elapsed = post_transcription(token, speech)
        transcript = (
            transcription.get("text", "") if isinstance(transcription, dict) else ""
        )
        words = (
            transcription.get("words", [])
            if isinstance(transcription, dict)
            else []
        )
        if "open" not in transcript.lower() or not words:
            raise RuntimeError("ASR endpoint returned no transcript or word timestamps")
        print(f"ASR inference: yes ({len(words)} aligned words, {elapsed:.2f}s)")
    except (urllib.error.URLError, ValueError, RuntimeError, IndexError) as error:
        print(f"New API capability smoke failed: {type(error).__name__}", file=sys.stderr)
        return 4
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
