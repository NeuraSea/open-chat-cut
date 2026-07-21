#!/usr/bin/env python3
"""Small authenticated Jina-compatible rerank service for the model farm."""

from __future__ import annotations

import argparse
import hmac
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import threading
from typing import Any
import uuid

import numpy as np
import torch
from sentence_transformers import CrossEncoder


MAX_BODY_BYTES = 4 * 1024 * 1024
MAX_DOCUMENTS = 1_000
DEFAULT_MAX_TOKENS_PER_DOCUMENT = 4_096


class RerankRuntime:
    def __init__(self, model_path: Path, token_file: Path) -> None:
        token = token_file.read_text(encoding="utf-8").strip()
        if len(token) < 24:
            raise RuntimeError("rerank service token is missing or too short")
        self.token = token
        self.device = "mps" if torch.backends.mps.is_available() else "cpu"
        self.model = CrossEncoder(
            str(model_path),
            device=self.device,
            trust_remote_code=True,
            local_files_only=True,
            max_length=8_192,
        )
        self.inference_lock = threading.Lock()

    def rerank(self, payload: dict[str, Any]) -> dict[str, Any]:
        query = payload.get("query")
        raw_documents = payload.get("documents")
        if not isinstance(query, str) or not query.strip():
            raise ValueError("query must be a non-empty string")
        if not isinstance(raw_documents, list) or not raw_documents:
            raise ValueError("documents must be a non-empty array")
        if len(raw_documents) > MAX_DOCUMENTS:
            raise ValueError(f"documents must contain at most {MAX_DOCUMENTS} items")

        documents: list[str] = []
        for document in raw_documents:
            if isinstance(document, str):
                text = document
            elif isinstance(document, dict) and isinstance(document.get("text"), str):
                text = document["text"]
            else:
                raise ValueError("every document must be a string or an object with text")
            documents.append(text)

        max_tokens = payload.get(
            "max_tokens_per_doc", DEFAULT_MAX_TOKENS_PER_DOCUMENT
        )
        if not isinstance(max_tokens, int) or not (1 <= max_tokens <= 8_192):
            raise ValueError("max_tokens_per_doc must be between 1 and 8192")
        # This is an input-safety cap; the model tokenizer performs the exact truncation.
        documents = [document[: max_tokens * 8] for document in documents]

        pairs = [(query, document) for document in documents]
        with self.inference_lock:
            raw_scores = self.model.predict(
                pairs,
                batch_size=min(16, len(pairs)),
                show_progress_bar=False,
                convert_to_numpy=True,
            )
        logits = np.asarray(raw_scores, dtype=np.float64).reshape(-1)
        scores = 1.0 / (1.0 + np.exp(-np.clip(logits, -60.0, 60.0)))

        ranked = sorted(
            enumerate(scores.tolist()), key=lambda item: item[1], reverse=True
        )
        top_n = payload.get("top_n", len(ranked))
        if not isinstance(top_n, int) or not (1 <= top_n <= len(ranked)):
            raise ValueError("top_n must be between 1 and the number of documents")
        return_documents = bool(payload.get("return_documents", False))
        results = []
        for index, score in ranked[:top_n]:
            result: dict[str, Any] = {
                "index": index,
                "relevance_score": score,
            }
            if return_documents:
                result["document"] = {"text": documents[index]}
            results.append(result)

        return {
            "id": f"rerank-{uuid.uuid4()}",
            "results": results,
            "meta": {
                "api_version": {"version": "2"},
                "billed_units": {"search_units": 0},
            },
        }


class Handler(BaseHTTPRequestHandler):
    runtime: RerankRuntime

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/health":
            self._json(
                HTTPStatus.OK,
                {"status": "ok", "device": self.runtime.device},
            )
            return
        self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        if self.path not in {"/v1/rerank", "/rerank"}:
            self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})
            return
        authorization = self.headers.get("Authorization", "")
        expected = f"Bearer {self.runtime.token}"
        if not hmac.compare_digest(authorization, expected):
            self._json(HTTPStatus.UNAUTHORIZED, {"error": "unauthorized"})
            return
        try:
            content_length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            content_length = -1
        if not (0 < content_length <= MAX_BODY_BYTES):
            self._json(HTTPStatus.REQUEST_ENTITY_TOO_LARGE, {"error": "invalid body"})
            return
        try:
            payload = json.loads(self.rfile.read(content_length))
            if not isinstance(payload, dict):
                raise ValueError("request body must be an object")
            response = self.runtime.rerank(payload)
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
            self._json(HTTPStatus.BAD_REQUEST, {"error": str(error)})
            return
        except Exception:
            self._json(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "inference failed"})
            return
        self._json(HTTPStatus.OK, response)

    def log_message(self, _format: str, *_args: object) -> None:
        # Do not log queries, documents, authorization headers, or URL parameters.
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
    parser.add_argument("--port", type=int, default=8190)
    parser.add_argument("--model-path", type=Path, required=True)
    parser.add_argument("--token-file", type=Path, required=True)
    arguments = parser.parse_args()

    runtime = RerankRuntime(arguments.model_path, arguments.token_file)
    Handler.runtime = runtime
    server = ThreadingHTTPServer((arguments.host, arguments.port), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
