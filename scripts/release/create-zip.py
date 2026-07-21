#!/usr/bin/env python3
"""Create a Windows release zip with exactly one regular-file root."""

from __future__ import annotations

import argparse
from pathlib import Path, PurePosixPath
import shutil
import zipfile


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("stage", type=Path)
    parser.add_argument("archive", type=Path)
    arguments = parser.parse_args()
    stage = arguments.stage.resolve()
    archive_path = arguments.archive.resolve()
    if not stage.is_dir() or not stage.name:
        raise RuntimeError("release stage must be a named directory")
    paths = sorted(stage.rglob("*"), key=lambda path: path.relative_to(stage).as_posix())
    if any(path.is_symlink() for path in paths):
        raise RuntimeError("release stage contains a symbolic link")
    files = [path for path in paths if path.is_file()]
    if not files:
        raise RuntimeError("release stage contains no files")
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    archive_path.unlink(missing_ok=True)
    with zipfile.ZipFile(
        archive_path,
        mode="w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
    ) as archive:
        for path in files:
            relative = PurePosixPath(stage.name) / path.relative_to(stage).as_posix()
            info = zipfile.ZipInfo(relative.as_posix(), date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            with path.open("rb") as source, archive.open(info, "w") as destination:
                shutil.copyfileobj(source, destination, length=1024 * 1024)


if __name__ == "__main__":
    main()
