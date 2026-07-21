#!/usr/bin/env python3
"""Authenticated OpenAI-compatible Qwen3-TTS speech service."""

from __future__ import annotations

import argparse
from io import BytesIO
import hmac
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import subprocess
import threading
from typing import Any
import wave

import numpy as np
import torch
from qwen_tts import Qwen3TTSModel


MAX_BODY_BYTES = 1024 * 1024
MAX_INPUT_CHARACTERS = 10_000
VOICE_ALIASES = {
    "alloy": "Vivian",
    "ash": "Aiden",
    "coral": "Serena",
    "echo": "Ryan",
    "fable": "Dylan",
    "nova": "Ono_Anna",
    "onyx": "Uncle_Fu",
    "sage": "Eric",
    "shimmer": "Sohee",
}
CONTENT_TYPES = {
    "aac": "audio/aac",
    "flac": "audio/flac",
    "mp3": "audio/mpeg",
    "opus": "audio/ogg",
    "pcm": "application/octet-stream",
    "wav": "audio/wav",
}


def wav_bytes(samples: np.ndarray, sample_rate: int) -> bytes:
    samples = np.asarray(samples, dtype=np.float32).reshape(-1)
    pcm = (np.clip(samples, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()
    output = BytesIO()
    with wave.open(output, "wb") as stream:
        stream.setnchannels(1)
        stream.setsampwidth(2)
        stream.setframerate(sample_rate)
        stream.writeframes(pcm)
    return output.getvalue()


def atempo_filters(speed: float) -> list[str]:
    filters = []
    remaining = speed
    while remaining > 2.0:
        filters.append("atempo=2.0")
        remaining /= 2.0
    while remaining < 0.5:
        filters.append("atempo=0.5")
        remaining /= 0.5
    filters.append(f"atempo={remaining:.8f}")
    return filters


def encode_audio(
    samples: np.ndarray,
    sample_rate: int,
    response_format: str,
    speed: float,
    ffmpeg: Path,
) -> bytes:
    source = wav_bytes(samples, sample_rate)
    if response_format == "wav" and speed == 1.0:
        return source
    if response_format == "pcm" and speed == 1.0:
        with wave.open(BytesIO(source), "rb") as stream:
            return stream.readframes(stream.getnframes())
    formats = {
        "aac": ["-c:a", "aac", "-f", "adts"],
        "flac": ["-c:a", "flac", "-f", "flac"],
        "mp3": ["-c:a", "libmp3lame", "-f", "mp3"],
        "opus": ["-c:a", "libopus", "-f", "opus"],
        "pcm": ["-c:a", "pcm_s16le", "-f", "s16le"],
        "wav": ["-c:a", "pcm_s16le", "-f", "wav"],
    }
    command = [
        str(ffmpeg),
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "wav",
        "-i",
        "pipe:0",
    ]
    if speed != 1.0:
        command.extend(["-af", ",".join(atempo_filters(speed))])
    command.extend([*formats[response_format], "pipe:1"])
    completed = subprocess.run(
        command,
        input=source,
        capture_output=True,
        check=True,
        timeout=120,
    )
    return completed.stdout


class TtsRuntime:
    def __init__(self, model_path: Path, token_file: Path, ffmpeg: Path) -> None:
        token = token_file.read_text(encoding="utf-8").strip()
        if len(token) < 24:
            raise RuntimeError("TTS service token is missing or too short")
        self.token = token
        self.model_path = model_path
        self.ffmpeg = ffmpeg
        self.model: Any | None = None
        self.device = "mps" if torch.backends.mps.is_available() else "cpu"
        self.lock = threading.Lock()

    def _load_model(self) -> Any:
        if self.model is None:
            self.model = Qwen3TTSModel.from_pretrained(
                str(self.model_path),
                device_map=self.device,
                dtype=torch.float32,
                attn_implementation="sdpa",
                local_files_only=True,
            )
        return self.model

    def speech(self, payload: dict[str, Any]) -> tuple[bytes, str]:
        text = payload.get("input")
        if not isinstance(text, str) or not text.strip():
            raise ValueError("input must be a non-empty string")
        if len(text) > MAX_INPUT_CHARACTERS:
            raise ValueError(f"input must contain at most {MAX_INPUT_CHARACTERS} characters")
        requested_voice = payload.get("voice", "alloy")
        if not isinstance(requested_voice, str) or not requested_voice:
            raise ValueError("voice must be a non-empty string")
        speaker = VOICE_ALIASES.get(requested_voice.lower(), requested_voice)
        language = payload.get("language", "Auto")
        if not isinstance(language, str) or not language:
            raise ValueError("language must be a non-empty string")
        instruct = payload.get("instructions", payload.get("instruct", ""))
        if not isinstance(instruct, str) or len(instruct) > 2_000:
            raise ValueError("instructions must contain at most 2000 characters")
        response_format = payload.get("response_format", "mp3")
        if response_format not in CONTENT_TYPES:
            raise ValueError("unsupported response_format")
        speed = payload.get("speed", 1.0)
        if not isinstance(speed, (int, float)) or not 0.25 <= float(speed) <= 4.0:
            raise ValueError("speed must be between 0.25 and 4.0")

        with self.lock:
            model = self._load_model()
            supported = model.get_supported_speakers()
            canonical = next(
                (candidate for candidate in supported if candidate.lower() == speaker.lower()),
                None,
            )
            if canonical is None:
                raise ValueError("unsupported voice")
            waveforms, sample_rate = model.generate_custom_voice(
                text=text,
                speaker=canonical,
                language=language,
                instruct=instruct or None,
                non_streaming_mode=True,
            )
        if not waveforms:
            raise RuntimeError("Qwen3-TTS returned no audio")
        encoded = encode_audio(
            waveforms[0],
            int(sample_rate),
            response_format,
            float(speed),
            self.ffmpeg,
        )
        return encoded, CONTENT_TYPES[response_format]


class Handler(BaseHTTPRequestHandler):
    runtime: TtsRuntime

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/health":
            self._json(
                HTTPStatus.OK,
                {
                    "status": "ok",
                    "model": self.runtime.model_path.name,
                    "device": self.runtime.device,
                    "loaded": self.runtime.model is not None,
                },
            )
            return
        self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/v1/audio/speech":
            self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})
            return
        expected = f"Bearer {self.runtime.token}"
        if not hmac.compare_digest(self.headers.get("Authorization", ""), expected):
            self._json(HTTPStatus.UNAUTHORIZED, {"error": "unauthorized"})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            length = -1
        if not (0 < length <= MAX_BODY_BYTES):
            self._json(HTTPStatus.REQUEST_ENTITY_TOO_LARGE, {"error": "invalid body"})
            return
        try:
            payload = json.loads(self.rfile.read(length))
            if not isinstance(payload, dict):
                raise ValueError("request body must be an object")
            audio, content_type = self.runtime.speech(payload)
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
            self._json(HTTPStatus.BAD_REQUEST, {"error": str(error)})
            return
        except Exception:
            self._json(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "speech synthesis failed"})
            return
        self.send_response(HTTPStatus.OK.value)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(audio)))
        self.end_headers()
        self.wfile.write(audio)

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def _json(self, status: HTTPStatus, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode()
        self.send_response(status.value)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8192)
    parser.add_argument("--model-path", type=Path, required=True)
    parser.add_argument("--token-file", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    arguments = parser.parse_args()

    Handler.runtime = TtsRuntime(
        arguments.model_path,
        arguments.token_file,
        arguments.ffmpeg,
    )
    ThreadingHTTPServer((arguments.host, arguments.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
