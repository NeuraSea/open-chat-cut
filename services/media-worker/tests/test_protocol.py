from pathlib import Path

import pytest

from openchatcut_worker.errors import WorkerError
from openchatcut_worker.protocol import JobRequest
from openchatcut_worker.security import resolve_under_root, safe_output_path


def test_job_request_uses_wire_case() -> None:
    request = JobRequest.from_dict(
        {
            "jobId": "job-1",
            "kind": "inspect_media",
            "projectId": "project-1",
            "inputPath": "media/source.mp4",
            "outputDir": "derived",
            "options": {"frames": 3},
        }
    )
    assert request.job_id == "job-1"
    assert request.options == {"frames": 3}


def test_resolve_under_root_rejects_traversal(tmp_path: Path) -> None:
    root = tmp_path / "data"
    root.mkdir()
    outside = tmp_path / "secret"
    outside.write_text("no", encoding="utf-8")

    with pytest.raises(WorkerError) as captured:
        resolve_under_root(value="../secret", root=root)

    assert captured.value.code == "PATH_OUTSIDE_AUTHORIZED_ROOT"


def test_resolve_under_root_rejects_symlink_escape(tmp_path: Path) -> None:
    root = tmp_path / "data"
    root.mkdir()
    outside = tmp_path / "secret"
    outside.write_text("no", encoding="utf-8")
    (root / "linked").symlink_to(outside)

    with pytest.raises(WorkerError) as captured:
        resolve_under_root(value="linked", root=root)

    assert captured.value.code == "PATH_OUTSIDE_AUTHORIZED_ROOT"


def test_safe_output_rejects_nested_name(tmp_path: Path) -> None:
    output = tmp_path / "output"
    with pytest.raises(WorkerError):
        safe_output_path(output_dir=output, file_name="../escape.wav")
