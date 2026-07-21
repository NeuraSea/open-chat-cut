#!/usr/bin/env python3
"""Verify bundle layout, traversal safety, and every release checksum."""

from __future__ import annotations

import argparse
from collections.abc import Iterable
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile
import tempfile
import zipfile


REQUIRED = {
    "VERSION",
    "LICENSE",
    "NOTICE.md",
    "release-manifest.json",
    "plugins/open-chat-cut/.codex-plugin/plugin.json",
    ".agents/plugins/marketplace.json",
}
FORBIDDEN_CACHE_PARTS = {"__pycache__", ".pytest_cache"}
SAFE_RELEASE_VALUE = re.compile(r"^[0-9A-Za-z._+-]+$")


def forbidden_cache_artifact(relative: str) -> bool:
    path = PurePosixPath(relative)
    return bool(FORBIDDEN_CACHE_PARTS.intersection(path.parts)) or path.suffix in {
        ".pyc",
        ".pyo",
    }


def safe_name(name: str) -> bool:
    path = PurePosixPath(name.replace("\\", "/"))
    return not path.is_absolute() and ".." not in path.parts


def sha256(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            value.update(chunk)
    return value.hexdigest()


def verify_root(root: Path) -> None:
    missing = sorted(path for path in REQUIRED if not (root / path).is_file())
    if missing:
        raise RuntimeError("bundle is missing: " + ", ".join(missing))
    manifest = json.loads((root / "release-manifest.json").read_text(encoding="utf-8"))
    if manifest.get("schemaVersion") != 1 or manifest.get("product") != "OpenChatCut":
        raise RuntimeError("release manifest identity is invalid")
    version = manifest.get("version")
    target = manifest.get("target")
    if (
        not isinstance(version, str)
        or not SAFE_RELEASE_VALUE.fullmatch(version)
        or not isinstance(target, str)
        or not SAFE_RELEASE_VALUE.fullmatch(target)
        or (root / "VERSION").read_text(encoding="utf-8").strip() != version
    ):
        raise RuntimeError("release manifest version or target is invalid")
    entries = manifest.get("files")
    if not isinstance(entries, list) or not entries:
        raise RuntimeError("release manifest contains no files")
    listed = set()
    for entry in entries:
        relative = entry.get("path")
        if not isinstance(relative, str) or not safe_name(relative):
            raise RuntimeError("release manifest contains an unsafe path")
        if forbidden_cache_artifact(relative):
            raise RuntimeError(f"release bundle contains a generated cache artifact: {relative}")
        path = root / relative
        if not path.is_file() or path.is_symlink():
            raise RuntimeError(f"manifest file is missing or symbolic: {relative}")
        if path.stat().st_size != entry.get("size") or sha256(path) != entry.get("sha256"):
            raise RuntimeError(f"checksum mismatch: {relative}")
        listed.add(relative)
    actual = {
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_file() and path.name != "release-manifest.json"
    }
    if listed != actual:
        raise RuntimeError("manifest file set differs from bundle contents")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("bundle", type=Path)
    arguments = parser.parse_args()
    bundle = arguments.bundle.resolve()
    if bundle.is_dir():
        verify_root(bundle)
        print(f"Verified release bundle: {bundle}")
        return
    with tempfile.TemporaryDirectory(prefix="openchatcut-release-") as temporary:
        destination = Path(temporary)
        if bundle.suffix == ".zip":
            with zipfile.ZipFile(bundle) as archive:
                entries = archive.infolist()
                names = [entry.filename for entry in entries]
                if any(not safe_name(name) for name in names):
                    raise RuntimeError("zip contains an unsafe path")
                if any((entry.external_attr >> 16) & 0o170000 == 0o120000 for entry in entries):
                    raise RuntimeError("zip contains a symbolic link")
                require_single_archive_root(names)
                archive.extractall(destination)
        else:
            with tarfile.open(bundle, "r:gz") as archive:
                members = archive.getmembers()
                if any(
                    not safe_name(member.name)
                    or not (member.isfile() or member.isdir())
                    for member in members
                ):
                    raise RuntimeError("tar archive contains an unsafe path or special entry")
                require_single_archive_root(member.name for member in members)
                try:
                    archive.extractall(destination, filter="data")
                except TypeError:  # Python 3.11 before extraction filters were backported.
                    archive.extractall(destination)
        roots = [path for path in destination.iterdir() if path.is_dir()]
        if len(roots) != 1:
            raise RuntimeError("release archive must contain exactly one root directory")
        verify_root(roots[0])
    print(f"Verified release archive: {bundle}")


def require_single_archive_root(names: Iterable[str]) -> None:
    roots = {
        PurePosixPath(str(name).replace("\\", "/")).parts[0]
        for name in names
        if PurePosixPath(str(name).replace("\\", "/")).parts
    }
    if len(roots) != 1:
        raise RuntimeError("release archive must contain exactly one root directory")


if __name__ == "__main__":
    main()
