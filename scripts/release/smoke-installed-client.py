#!/usr/bin/env python3
"""Exercise an installed portable client, including restart persistence."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import time
from typing import Any
import urllib.error
import urllib.request
import uuid


URL_OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def launcher_command(app_root: Path, command: str) -> list[str]:
    if os.name == "nt":
        return [
            "pwsh",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(app_root / "openchatcut.ps1"),
            "-Command",
            command,
        ]
    return [str(app_root / "openchatcut"), command]


def run_launcher(app_root: Path, command: str, environment: dict[str, str]) -> None:
    subprocess.run(
        launcher_command(app_root, command),
        check=True,
        env=environment,
        timeout=90,
    )


def request_bytes(
    url: str,
    *,
    token: str | None = None,
    method: str = "GET",
    body: dict[str, Any] | None = None,
    idempotency_key: str | None = None,
) -> bytes:
    headers = {"Accept": "application/json"}
    data = None
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body, separators=(",", ":")).encode("utf-8")
    if idempotency_key:
        headers["Idempotency-Key"] = idempotency_key
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    with URL_OPENER.open(request, timeout=10) as response:
        return response.read()


def wait_ready(url: str, expected: bool) -> None:
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            request_bytes(url)
            ready = True
        except (OSError, urllib.error.URLError, urllib.error.HTTPError):
            ready = False
        if ready == expected:
            return
        time.sleep(0.25)
    state = "ready" if expected else "offline"
    raise RuntimeError(f"{url} did not become {state}")


def authenticated_json(url: str, token: str) -> dict[str, Any]:
    value = json.loads(request_bytes(url, token=token))
    if not isinstance(value, dict):
        raise RuntimeError(f"API returned a non-object for {url}")
    return value


def read_token(home: Path) -> str:
    descriptor = json.loads((home / "runtime.json").read_text(encoding="utf-8"))
    token_path = Path(descriptor["tokenPath"]).resolve()
    if token_path != (home / "daemon.token").resolve():
        raise RuntimeError("runtime descriptor tokenPath escaped the isolated home")
    token = token_path.read_text(encoding="utf-8").strip()
    if not token:
        raise RuntimeError("daemon token is empty")
    return token


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--app-root", type=Path, required=True)
    parser.add_argument("--home", type=Path, required=True)
    parser.add_argument("--web-port", type=int, default=3100)
    arguments = parser.parse_args()
    app_root = arguments.app_root.resolve()
    home = arguments.home.resolve()
    home.mkdir(parents=True, exist_ok=True)
    if not 1 <= arguments.web_port <= 65535 or arguments.web_port == 3210:
        raise RuntimeError("web port is invalid")

    environment = os.environ.copy()
    environment["OPENCHATCUT_HOME"] = str(home)
    environment["OPENCHATCUT_WEB_PORT"] = str(arguments.web_port)
    daemon_health = "http://127.0.0.1:3210/health"
    web_origin = f"http://127.0.0.1:{arguments.web_port}"
    api = "http://127.0.0.1:3210/api/v1"
    project_id = f"portable-ci-{uuid.uuid4()}"
    idempotency_key = f"portable-ci-create-{uuid.uuid4()}"
    expected_hash = ""
    started = False
    try:
        started = True
        run_launcher(app_root, "start", environment)
        wait_ready(daemon_health, True)
        wait_ready(f"{web_origin}/api/health", True)
        page = request_bytes(f"{web_origin}/projects").decode("utf-8", errors="replace")
        if "OpenChatCut" not in page:
            raise RuntimeError("standalone Web editor did not render OpenChatCut")
        token = read_token(home)
        status = authenticated_json(f"{api}/status", token)
        if status.get("status") != "ready":
            raise RuntimeError("daemon status is not ready")
        created = json.loads(
            request_bytes(
                f"{api}/projects",
                token=token,
                method="POST",
                idempotency_key=idempotency_key,
                body={
                    "name": "Portable CI smoke",
                    "projectId": project_id,
                    "idempotencyKey": idempotency_key,
                },
            )
        )
        expected_hash = created["envelope"]["documentHash"]

        run_launcher(app_root, "stop", environment)
        started = False
        wait_ready(daemon_health, False)
        wait_ready(f"{web_origin}/api/health", False)

        started = True
        run_launcher(app_root, "start", environment)
        token = read_token(home)
        restored = authenticated_json(f"{api}/projects/{project_id}", token)["envelope"]
        if restored["documentHash"] != expected_hash or restored["revision"] != 0:
            raise RuntimeError("project changed across portable client restart")
        print(
            json.dumps(
                {
                    "portableClient": "passed",
                    "projectId": project_id,
                    "revision": restored["revision"],
                    "documentHash": restored["documentHash"],
                },
                indent=2,
            )
        )
    finally:
        if started:
            run_launcher(app_root, "stop", environment)


if __name__ == "__main__":
    main()
