#!/usr/bin/env python3
"""Authenticated OpenAI-compatible WhisperX transcription service."""

from __future__ import annotations

import argparse
from email import policy
from email.parser import BytesParser
import hmac
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sys
import tempfile
import threading
from typing import Any

import whisperx


MAX_BODY_BYTES = 512 * 1024 * 1024
MAX_PROMPT_BYTES = 32 * 1024
ALLOWED_RESPONSE_FORMATS = {"json", "text", "verbose_json", "srt", "vtt"}


def parse_multipart(content_type: str, body: bytes) -> tuple[dict[str, str], bytes, str]:
    message = BytesParser(policy=policy.default).parsebytes(
        (
            f"Content-Type: {content_type}\r\n"
            "MIME-Version: 1.0\r\n\r\n"
        ).encode()
        + body
    )
    if not message.is_multipart():
        raise ValueError("request must be multipart/form-data")

    fields: dict[str, str] = {}
    audio: bytes | None = None
    filename = "upload.audio"
    for part in message.iter_parts():
        name = part.get_param("name", header="content-disposition")
        if not isinstance(name, str):
            continue
        payload = part.get_payload(decode=True) or b""
        part_filename = part.get_filename()
        if name == "file" and part_filename is not None:
            audio = payload
            filename = Path(part_filename).name
            continue
        try:
            fields[name] = payload.decode(part.get_content_charset() or "utf-8")
        except UnicodeDecodeError as error:
            raise ValueError(f"field {name} is not valid UTF-8") from error
    if not audio:
        raise ValueError("file must contain non-empty audio")
    return fields, audio, filename


def cue_time(seconds: float, separator: str) -> str:
    milliseconds = max(0, round(seconds * 1000))
    hours, milliseconds = divmod(milliseconds, 3_600_000)
    minutes, milliseconds = divmod(milliseconds, 60_000)
    whole_seconds, milliseconds = divmod(milliseconds, 1_000)
    return f"{hours:02}:{minutes:02}:{whole_seconds:02}{separator}{milliseconds:03}"


class WhisperRuntime:
    def __init__(
        self,
        model: str,
        model_root: Path,
        alignment_root: Path,
        token_file: Path,
        threads: int,
    ) -> None:
        token = token_file.read_text(encoding="utf-8").strip()
        if len(token) < 24:
            raise RuntimeError("ASR service token is missing or too short")
        self.token = token
        self.model_name = model
        self.model_root = model_root
        self.alignment_root = alignment_root
        self.threads = threads
        self.model: Any | None = None
        self.aligners: dict[str, tuple[Any, dict[str, Any]]] = {}
        self.lock = threading.Lock()

    def _load_model(self) -> Any:
        if self.model is None:
            self.model_root.mkdir(parents=True, exist_ok=True)
            self.model = whisperx.load_model(
                self.model_name,
                "cpu",
                compute_type="int8",
                vad_method="silero",
                download_root=str(self.model_root),
                threads=self.threads,
            )
        return self.model

    def transcribe(self, path: Path, fields: dict[str, str]) -> dict[str, Any]:
        language = fields.get("language") or None
        task = "translate" if fields.get("task") == "translate" else "transcribe"
        with self.lock:
            audio = whisperx.load_audio(str(path))
            result = self._load_model().transcribe(
                audio,
                batch_size=4,
                language=language,
                task=task,
            )
            detected_language = str(result.get("language") or language or "und")
            segments = result.get("segments") or []
            alignment_warning: str | None = None
            if segments:
                try:
                    aligner = self.aligners.get(detected_language)
                    if aligner is None:
                        self.alignment_root.mkdir(parents=True, exist_ok=True)
                        aligner = whisperx.load_align_model(
                            language_code=detected_language,
                            device="cpu",
                            model_dir=str(self.alignment_root),
                        )
                        self.aligners[detected_language] = aligner
                    result = whisperx.align(
                        segments,
                        aligner[0],
                        aligner[1],
                        audio,
                        "cpu",
                        return_char_alignments=False,
                    )
                except (OSError, RuntimeError, ValueError):
                    # Transcription remains useful if a language-specific,
                    # optional alignment model is unavailable. Never fabricate
                    # word timestamps from segment boundaries.
                    alignment_warning = "word alignment model unavailable"

        normalized_segments = []
        words = []
        for index, segment in enumerate(result.get("segments") or segments):
            start = float(segment.get("start") or 0.0)
            end = float(segment.get("end") or start)
            text = str(segment.get("text") or "").strip()
            normalized_segments.append(
                {
                    "id": index,
                    "seek": 0,
                    "start": start,
                    "end": end,
                    "text": text,
                    "tokens": [],
                    "temperature": 0.0,
                    "avg_logprob": float(segment.get("avg_logprob") or 0.0),
                    "compression_ratio": 0.0,
                    "no_speech_prob": 0.0,
                }
            )
            for word in segment.get("words") or []:
                if "start" not in word or "end" not in word:
                    continue
                words.append(
                    {
                        "word": str(word.get("word") or ""),
                        "start": float(word["start"]),
                        "end": float(word["end"]),
                    }
                )
        if not words:
            for word in result.get("word_segments") or []:
                if "start" not in word or "end" not in word:
                    continue
                words.append(
                    {
                        "word": str(word.get("word") or ""),
                        "start": float(word["start"]),
                        "end": float(word["end"]),
                    }
                )
        text = " ".join(segment["text"] for segment in normalized_segments).strip()
        duration = max((segment["end"] for segment in normalized_segments), default=0.0)
        payload: dict[str, Any] = {
            "task": task,
            "language": detected_language,
            "duration": duration,
            "text": text,
            "segments": normalized_segments,
            "words": words,
        }
        if alignment_warning:
            payload["alignment_warning"] = alignment_warning
        return payload


class Handler(BaseHTTPRequestHandler):
    runtime: WhisperRuntime
    temporary_root: Path

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/health":
            self._json(
                HTTPStatus.OK,
                {
                    "status": "ok",
                    "model": self.runtime.model_name,
                    "loaded": self.runtime.model is not None,
                },
            )
            return
        self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        if self.path not in {"/v1/audio/transcriptions", "/v1/audio/translations"}:
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
            fields, audio, filename = parse_multipart(
                self.headers.get("Content-Type", ""),
                self.rfile.read(length),
            )
            if len(fields.get("prompt", "").encode()) > MAX_PROMPT_BYTES:
                raise ValueError("prompt is too large")
            response_format = fields.get("response_format", "json")
            if response_format not in ALLOWED_RESPONSE_FORMATS:
                raise ValueError("unsupported response_format")
            suffix = Path(filename).suffix[:12] or ".audio"
            self.temporary_root.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(
                dir=self.temporary_root,
                suffix=suffix,
                delete=False,
            ) as stream:
                stream.write(audio)
                temporary = Path(stream.name)
            try:
                if self.path.endswith("translations"):
                    fields["task"] = "translate"
                result = self.runtime.transcribe(temporary, fields)
            finally:
                temporary.unlink(missing_ok=True)
        except (UnicodeDecodeError, ValueError) as error:
            self._json(HTTPStatus.BAD_REQUEST, {"error": str(error)})
            return
        except Exception as error:
            message = str(error).replace("\r", " ").replace("\n", " ")[:500]
            print(
                f"ASR request failed: {type(error).__name__}: {message}",
                file=sys.stderr,
                flush=True,
            )
            self._json(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "transcription failed"})
            return

        if response_format == "text":
            self._bytes(HTTPStatus.OK, result["text"].encode(), "text/plain; charset=utf-8")
        elif response_format in {"srt", "vtt"}:
            output = ["WEBVTT\n"] if response_format == "vtt" else []
            for index, segment in enumerate(result["segments"], start=1):
                separator = "." if response_format == "vtt" else ","
                output.extend(
                    [
                        str(index),
                        f"{cue_time(segment['start'], separator)} --> {cue_time(segment['end'], separator)}",
                        segment["text"],
                        "",
                    ]
                )
            self._bytes(
                HTTPStatus.OK,
                "\n".join(output).encode(),
                "text/vtt; charset=utf-8"
                if response_format == "vtt"
                else "application/x-subrip; charset=utf-8",
            )
        elif response_format == "verbose_json":
            self._json(HTTPStatus.OK, result)
        else:
            self._json(HTTPStatus.OK, {"text": result["text"]})

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def _json(self, status: HTTPStatus, payload: dict[str, Any]) -> None:
        self._bytes(
            status,
            json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode(),
            "application/json; charset=utf-8",
        )

    def _bytes(self, status: HTTPStatus, body: bytes, content_type: str) -> None:
        self.send_response(status.value)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8191)
    parser.add_argument("--model", default="large-v3")
    parser.add_argument("--model-root", type=Path, required=True)
    parser.add_argument("--alignment-root", type=Path, required=True)
    parser.add_argument("--temporary-root", type=Path, required=True)
    parser.add_argument("--token-file", type=Path, required=True)
    parser.add_argument("--threads", type=int, default=16)
    arguments = parser.parse_args()

    Handler.runtime = WhisperRuntime(
        arguments.model,
        arguments.model_root,
        arguments.alignment_root,
        arguments.token_file,
        arguments.threads,
    )
    Handler.temporary_root = arguments.temporary_root
    ThreadingHTTPServer((arguments.host, arguments.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
