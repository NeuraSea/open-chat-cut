#!/usr/bin/env python3
"""Authenticated OpenAI-compatible Qwen Image service backed by ComfyUI."""

from __future__ import annotations

import argparse
import base64
import hmac
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import secrets
import threading
import time
from typing import Any
import urllib.error
import urllib.parse
import urllib.request
import uuid


MAX_REQUEST_BYTES = 128 * 1024
MAX_IMAGE_BYTES = 64 * 1024 * 1024
MAX_PROMPT_BYTES = 20_000
ALLOWED_MODELS = {"occ-image", "Qwen-Image-2512"}
ALLOWED_SIZES = {
    "512x512": (512, 512),
    "768x768": (768, 768),
    "1024x1024": (1024, 1024),
    "1024x768": (1024, 768),
    "768x1024": (768, 1024),
}


class ImageRuntime:
    def __init__(self, token_file: Path, comfy_url: str, output_root: Path) -> None:
        token = token_file.read_text(encoding="utf-8").strip()
        if len(token) < 24:
            raise RuntimeError("image service token is missing or too short")
        self.token = token
        self.comfy_url = comfy_url.rstrip("/")
        self.output_root = output_root.resolve()
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.lock = threading.Lock()

    def _json_request(
        self,
        path: str,
        payload: dict[str, Any] | None = None,
        timeout: float = 60,
    ) -> Any:
        request = urllib.request.Request(
            f"{self.comfy_url}{path}",
            data=None if payload is None else json.dumps(payload).encode(),
            headers={"Content-Type": "application/json", "Accept": "application/json"},
            method="GET" if payload is None else "POST",
        )
        with self.opener.open(request, timeout=timeout) as response:
            if response.status != HTTPStatus.OK:
                raise RuntimeError(f"ComfyUI returned HTTP {response.status}")
            return json.load(response)

    def _workflow(
        self,
        prompt: str,
        negative_prompt: str,
        width: int,
        height: int,
        seed: int,
        request_id: str,
    ) -> dict[str, Any]:
        return {
            "1": {"class_type": "UNETLoader", "inputs": {
                "unet_name": "qwen_image_2512_fp8_e4m3fn.safetensors",
                "weight_dtype": "default",
            }},
            "2": {"class_type": "LoraLoaderModelOnly", "inputs": {
                "model": ["1", 0],
                "lora_name": "Qwen-Image-2512-Lightning-4steps-V1.0-fp32.safetensors",
                "strength_model": 1.0,
            }},
            "3": {"class_type": "ModelSamplingAuraFlow", "inputs": {
                "model": ["2", 0], "shift": 3.1,
            }},
            "4": {"class_type": "CLIPLoader", "inputs": {
                "clip_name": "qwen_2.5_vl_7b_fp8_scaled.safetensors",
                "type": "qwen_image", "device": "default",
            }},
            "5": {"class_type": "CLIPTextEncode", "inputs": {
                "text": prompt, "clip": ["4", 0],
            }},
            "6": {"class_type": "CLIPTextEncode", "inputs": {
                "text": negative_prompt, "clip": ["4", 0],
            }},
            "7": {"class_type": "VAELoader", "inputs": {
                "vae_name": "qwen_image_vae.safetensors",
            }},
            "8": {"class_type": "EmptySD3LatentImage", "inputs": {
                "width": width, "height": height, "batch_size": 1,
            }},
            "9": {"class_type": "KSampler", "inputs": {
                "model": ["3", 0], "seed": seed, "steps": 4, "cfg": 1.0,
                "sampler_name": "euler", "scheduler": "simple",
                "positive": ["5", 0], "negative": ["6", 0],
                "latent_image": ["8", 0], "denoise": 1.0,
            }},
            "10": {"class_type": "VAEDecode", "inputs": {
                "samples": ["9", 0], "vae": ["7", 0],
            }},
            "11": {"class_type": "SaveImage", "inputs": {
                "images": ["10", 0],
                "filename_prefix": f"openchatcut-api/{request_id}",
            }},
        }

    def generate(self, payload: dict[str, Any]) -> tuple[bytes, str]:
        if payload.get("model", "occ-image") not in ALLOWED_MODELS:
            raise ValueError("unsupported model")
        prompt = payload.get("prompt")
        if not isinstance(prompt, str) or not prompt.strip():
            raise ValueError("prompt must be a non-empty string")
        if len(prompt.encode()) > MAX_PROMPT_BYTES or "\x00" in prompt:
            raise ValueError("prompt is too large or invalid")
        negative = payload.get("negative_prompt", "")
        if not isinstance(negative, str) or len(negative.encode()) > MAX_PROMPT_BYTES:
            raise ValueError("negative_prompt is invalid")
        if payload.get("n", 1) != 1:
            raise ValueError("only n=1 is supported")
        size = payload.get("size", "1024x1024")
        if size not in ALLOWED_SIZES:
            raise ValueError("unsupported size")
        if payload.get("response_format", "b64_json") != "b64_json":
            raise ValueError("only response_format=b64_json is supported")
        seed = payload.get("seed", secrets.randbits(63))
        if isinstance(seed, bool) or not isinstance(seed, int) or not 0 <= seed < 2**63:
            raise ValueError("seed must be a non-negative 63-bit integer")
        width, height = ALLOWED_SIZES[size]
        request_id = uuid.uuid4().hex
        workflow = self._workflow(
            prompt.strip(), negative.strip(), width, height, seed, request_id
        )

        with self.lock:
            submitted = self._json_request(
                "/prompt", {"prompt": workflow, "client_id": f"openchatcut-{request_id}"}
            )
            prompt_id = submitted.get("prompt_id") if isinstance(submitted, dict) else None
            if not isinstance(prompt_id, str) or not prompt_id:
                raise RuntimeError("ComfyUI did not return a prompt ID")
            deadline = time.monotonic() + 30 * 60
            image_info: dict[str, Any] | None = None
            while time.monotonic() < deadline:
                history = self._json_request(f"/history/{prompt_id}", timeout=30)
                item = history.get(prompt_id) if isinstance(history, dict) else None
                if isinstance(item, dict):
                    status = item.get("status") or {}
                    if status.get("status_str") == "error":
                        raise RuntimeError("ComfyUI image workflow failed")
                    images = (item.get("outputs") or {}).get("11", {}).get("images") or []
                    if images:
                        image_info = images[0]
                        break
                time.sleep(0.5)
            if image_info is None:
                raise RuntimeError("ComfyUI image workflow timed out")
            filename = image_info.get("filename")
            subfolder = image_info.get("subfolder", "")
            image_type = image_info.get("type", "output")
            if (
                not isinstance(filename, str)
                or Path(filename).name != filename
                or not isinstance(subfolder, str)
                or Path(subfolder).is_absolute()
                or ".." in Path(subfolder).parts
                or image_type != "output"
            ):
                raise RuntimeError("ComfyUI returned an unsafe output path")
            query = urllib.parse.urlencode(
                {"filename": filename, "subfolder": subfolder, "type": image_type}
            )
            with self.opener.open(f"{self.comfy_url}/view?{query}", timeout=60) as response:
                content_type = response.headers.get_content_type()
                image = response.read(MAX_IMAGE_BYTES + 1)
            if len(image) > MAX_IMAGE_BYTES or content_type not in {"image/png", "image/jpeg"}:
                raise RuntimeError("ComfyUI returned an invalid image")
            output = (self.output_root / subfolder / filename).resolve()
            if output.is_relative_to(self.output_root):
                output.unlink(missing_ok=True)
        return image, prompt.strip()


class Handler(BaseHTTPRequestHandler):
    runtime: ImageRuntime

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/health":
            self._json(HTTPStatus.OK, {
                "status": "ok", "model": "Qwen-Image-2512", "backend": "ComfyUI",
            })
            return
        self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/v1/images/generations":
            self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})
            return
        if not hmac.compare_digest(
            self.headers.get("Authorization", ""), f"Bearer {self.runtime.token}"
        ):
            self._json(HTTPStatus.UNAUTHORIZED, {"error": "unauthorized"})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            length = -1
        if not 0 < length <= MAX_REQUEST_BYTES:
            self._json(HTTPStatus.REQUEST_ENTITY_TOO_LARGE, {"error": "invalid body"})
            return
        try:
            payload = json.loads(self.rfile.read(length))
            if not isinstance(payload, dict):
                raise ValueError("request must be a JSON object")
            image, revised_prompt = self.runtime.generate(payload)
        except (json.JSONDecodeError, UnicodeDecodeError, ValueError) as error:
            self._json(HTTPStatus.BAD_REQUEST, {"error": str(error)})
            return
        except (OSError, RuntimeError, urllib.error.URLError):
            self._json(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": "image generation failed"})
            return
        self._json(HTTPStatus.OK, {
            "created": int(time.time()),
            "data": [{
                "b64_json": base64.b64encode(image).decode("ascii"),
                "revised_prompt": revised_prompt,
            }],
        })

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
    parser.add_argument("--port", type=int, default=8193)
    parser.add_argument("--token-file", type=Path, required=True)
    parser.add_argument("--comfy-url", default="http://127.0.0.1:8188")
    parser.add_argument("--output-root", type=Path, required=True)
    arguments = parser.parse_args()
    Handler.runtime = ImageRuntime(
        arguments.token_file, arguments.comfy_url, arguments.output_root
    )
    ThreadingHTTPServer((arguments.host, arguments.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
