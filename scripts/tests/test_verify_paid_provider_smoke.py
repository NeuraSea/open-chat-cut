from __future__ import annotations

import argparse
import importlib.util
import os
from pathlib import Path
import tempfile
import types
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "verify-paid-provider-smoke.py"
SPEC = importlib.util.spec_from_file_location("verify_paid_provider_smoke", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("could not load paid provider smoke module")
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)


def arguments(report: Path | None = None) -> argparse.Namespace:
    return argparse.Namespace(
        provider="seedance-compatible",
        kind=None,
        model="smoke-model",
        prompt="A tiny blue circle",
        options_json="{}",
        timeout_seconds=30,
        runtime_descriptor=Path("/unused/runtime.json"),
        report=report,
        delete_project=False,
    )


def generated_asset() -> dict:
    return {
        "id": "asset:generated:job-1",
        "name": "Generated video",
        "kind": "video",
        "contentHash": "a" * 64,
        "provenance": {
            "type": "generated",
            "provider": "seedance-compatible",
            "model": "smoke-model",
            "prompt": "A tiny blue circle",
        },
        "managedMedia": {
            "byteSize": 1024,
            "mimeType": "video/mp4",
            "normalization": "ffmpeg-h264-aac-v1",
        },
    }


class PaidProviderSmokeTest(unittest.TestCase):
    def test_cost_confirmation_is_mandatory(self) -> None:
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop(smoke.PAID_CONFIRMATION_ENV, None)
            with self.assertRaisesRegex(smoke.SmokeFailure, "expected charge"):
                smoke.require_paid_confirmation()

    def test_success_requires_a_managed_generated_asset(self) -> None:
        asset = generated_asset()
        calls: list[tuple[str, str, dict | None]] = []

        def request_json(
            _api_base_url: str,
            _token: str,
            method: str,
            path: str,
            *,
            body: dict | None = None,
        ) -> dict:
            calls.append((method, path, body))
            if path == "/tools/list_generators":
                return {
                    "data": {
                        "providers": [
                            {
                                "id": "seedance-compatible",
                                "availability": {"state": "available"},
                                "models": ["smoke-model"],
                                "adapters": [
                                    {
                                        "capability": "videoGeneration",
                                        "transport": {
                                            "type": "http",
                                            "baseUrl": "https://provider.invalid/v1",
                                        },
                                        "requiresNetwork": True,
                                    }
                                ],
                            }
                        ]
                    }
                }
            if path == "/projects" and method == "POST":
                return {"envelope": {"revision": 0}}
            if path == "/tools/generate_asset":
                return {"jobId": "job-1"}
            if path.startswith("/projects/") and method == "GET":
                return {
                    "envelope": {
                        "revision": 1,
                        "document": {"assets": [asset]},
                    }
                }
            raise AssertionError(f"unexpected request {method} {path}")

        job = {
            "id": "job-1",
            "state": "succeeded",
            "output": {"asset": {"id": asset["id"]}},
        }
        with tempfile.TemporaryDirectory() as directory:
            report_path = Path(directory) / "report.json"
            with (
                mock.patch.dict(
                    os.environ,
                    {
                        smoke.PAID_CONFIRMATION_ENV: smoke.PAID_CONFIRMATION_VALUE,
                    },
                ),
                mock.patch.object(
                    smoke,
                    "load_runtime",
                    return_value=("http://127.0.0.1:3210/api/v1", "token"),
                ),
                mock.patch.object(smoke, "request_json", side_effect=request_json),
                mock.patch.object(smoke, "wait_for_job", return_value=job),
                mock.patch.object(
                    smoke.uuid,
                    "uuid4",
                    return_value=types.SimpleNamespace(hex="abc123"),
                ),
            ):
                result = smoke.run(arguments(report_path))

            self.assertTrue(result["ok"])
            self.assertEqual(result["contentHash"], "a" * 64)
            self.assertEqual(result["normalization"], "ffmpeg-h264-aac-v1")
            self.assertTrue(report_path.is_file())

        submission = next(
            body for _, path, body in calls if path == "/tools/generate_asset"
        )
        self.assertIsNotNone(submission)
        assert submission is not None
        self.assertTrue(submission["arguments"]["confirm"])
        self.assertEqual(submission["arguments"]["options"]["timeoutSeconds"], 30)

    def test_unavailable_provider_never_creates_a_project(self) -> None:
        calls: list[str] = []

        def request_json(
            _api_base_url: str,
            _token: str,
            _method: str,
            path: str,
            *,
            body: dict | None = None,
        ) -> dict:
            del body
            calls.append(path)
            return {
                "data": {
                    "providers": [
                        {
                            "id": "seedance-compatible",
                            "availability": {"state": "needsConfiguration"},
                            "adapters": [],
                        }
                    ]
                }
            }

        with (
            mock.patch.dict(
                os.environ,
                {smoke.PAID_CONFIRMATION_ENV: smoke.PAID_CONFIRMATION_VALUE},
            ),
            mock.patch.object(
                smoke,
                "load_runtime",
                return_value=("http://127.0.0.1:3210/api/v1", "token"),
            ),
            mock.patch.object(smoke, "request_json", side_effect=request_json),
        ):
            with self.assertRaisesRegex(smoke.SmokeFailure, "not available"):
                smoke.run(arguments())
        self.assertEqual(calls, ["/tools/list_generators"])

    @unittest.skipIf(os.name == "nt", "creating symlinks is not reliable on Windows CI")
    def test_private_file_reader_rejects_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target.json"
            target.write_text("{}", encoding="utf-8")
            target.chmod(0o600)
            link = root / "runtime.json"
            link.symlink_to(target)
            with self.assertRaisesRegex(smoke.SmokeFailure, "non-symlink"):
                smoke.read_private_text(link, "runtime")


if __name__ == "__main__":
    unittest.main()
