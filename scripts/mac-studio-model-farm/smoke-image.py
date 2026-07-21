#!/usr/bin/env python3
"""Run one real authenticated Qwen Image inference without printing secrets."""

import argparse
import base64
import json
from pathlib import Path
import subprocess
import urllib.request


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://127.0.0.1:8193/v1/images/generations")
    credentials = parser.add_mutually_exclusive_group(required=True)
    credentials.add_argument("--token-file", type=Path)
    credentials.add_argument("--keychain-service")
    parser.add_argument("--keychain-account", default="openchatcut")
    parser.add_argument("--size", default="512x512")
    arguments = parser.parse_args()
    if arguments.token_file is not None:
        token = arguments.token_file.read_text(encoding="utf-8").strip()
    else:
        token = subprocess.check_output(
            [
                "/usr/bin/security", "find-generic-password",
                "-a", arguments.keychain_account,
                "-s", arguments.keychain_service,
                "-w",
            ],
            text=True,
            stderr=subprocess.DEVNULL,
        ).rstrip("\r\n")
    request = urllib.request.Request(
        arguments.url,
        data=json.dumps({
            "model": "occ-image",
            "prompt": "A clean cinematic clapboard icon on a deep blue background, no text",
            "size": arguments.size,
            "n": 1,
            "response_format": "b64_json",
            "seed": 20260720,
        }).encode(),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        },
    )
    with urllib.request.urlopen(request, timeout=1_800) as response:
        payload = json.load(response)
    image = base64.b64decode(payload["data"][0]["b64_json"], validate=True)
    if len(image) < 10_000 or not image.startswith(b"\x89PNG\r\n\x1a\n"):
        raise RuntimeError("image endpoint returned invalid PNG data")
    print(f"Image inference: yes ({len(image)} bytes)")


if __name__ == "__main__":
    main()
