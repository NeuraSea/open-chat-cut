#!/usr/bin/env python3
"""Create a deterministic checksum manifest for an OpenChatCut bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            value.update(chunk)
    return value.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    arguments = parser.parse_args()
    root = arguments.root.resolve()
    files = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.name == "release-manifest.json":
            continue
        relative = path.relative_to(root).as_posix()
        files.append(
            {
                "path": relative,
                "sha256": digest(path),
                "size": path.stat().st_size,
                "executable": bool(path.stat().st_mode & 0o111),
            }
        )
    payload = {
        "schemaVersion": 1,
        "product": "OpenChatCut",
        "version": arguments.version,
        "target": arguments.target,
        "files": files,
    }
    (root / "release-manifest.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
