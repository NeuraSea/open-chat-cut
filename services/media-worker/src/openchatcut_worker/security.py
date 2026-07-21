from __future__ import annotations

import os
from pathlib import Path

from .errors import WorkerError


def resolve_under_root(*, value: str, root: Path, must_exist: bool = True) -> Path:
    """Resolve a daemon-provided path without allowing traversal or symlink escape."""

    canonical_root = root.expanduser().resolve(strict=True)
    candidate = Path(value).expanduser()
    if not candidate.is_absolute():
        candidate = canonical_root / candidate
    resolved = candidate.resolve(strict=must_exist)
    try:
        resolved.relative_to(canonical_root)
    except ValueError as error:
        raise WorkerError(
            "PATH_OUTSIDE_AUTHORIZED_ROOT",
            "Media path is outside the daemon-authorized data directory",
        ) from error
    return resolved


def safe_output_path(*, output_dir: Path, file_name: str) -> Path:
    if not file_name or Path(file_name).name != file_name:
        raise WorkerError("INVALID_OUTPUT_NAME", "Output name must be a single file name")
    output_dir.mkdir(parents=True, exist_ok=True)
    result = (output_dir / file_name).resolve(strict=False)
    try:
        result.relative_to(output_dir.resolve(strict=True))
    except ValueError as error:
        raise WorkerError("PATH_OUTSIDE_AUTHORIZED_ROOT", "Invalid output path") from error
    return result


def sanitized_environment() -> dict[str, str]:
    """Subprocess environment without provider keys or Python injection variables."""

    allowed = ("PATH", "HOME", "TMPDIR", "TEMP", "SystemRoot", "WINDIR")
    return {key: os.environ[key] for key in allowed if key in os.environ}
