#!/usr/bin/env python3
"""Run one explicitly approved, real paid-provider generation smoke.

This script never starts a provider implicitly. The daemon must already be
running with a private provider configuration and the caller must set the exact
cost-confirmation environment value documented below. A successful run proves
the remote submit/poll/download path ended in a managed, content-addressed
OpenChatCut asset instead of a short-lived remote URL.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import time
from typing import Any
import urllib.error
import urllib.parse
import urllib.request
import uuid


PAID_CONFIRMATION_ENV = "OPENCHATCUT_RUN_PAID_PROVIDER_SMOKE"
PAID_CONFIRMATION_VALUE = "I_UNDERSTAND_THIS_MAY_INCUR_COSTS"
REMOTE_PROVIDERS = ("seedance", "seedance-compatible", "fal", "suno")
TERMINAL_JOB_STATES = {"succeeded", "failed", "cancelled"}
MAX_RESPONSE_BYTES = 8 * 1024 * 1024


class SmokeFailure(RuntimeError):
    """A user-facing acceptance failure that does not expose credentials."""


def require_paid_confirmation() -> None:
    if os.environ.get(PAID_CONFIRMATION_ENV) != PAID_CONFIRMATION_VALUE:
        raise SmokeFailure(
            "paid provider smoke is disabled; set "
            f"{PAID_CONFIRMATION_ENV}={PAID_CONFIRMATION_VALUE} only after "
            "reviewing the provider prompt, model, options, and expected charge"
        )


def default_runtime_descriptor() -> Path:
    configured = os.environ.get("OPENCHATCUT_RUNTIME_DESCRIPTOR")
    if configured:
        return Path(configured).expanduser()
    home = Path(os.environ.get("OPENCHATCUT_HOME", Path.home() / ".openchatcut"))
    return home / "runtime.json"


def read_private_text(path: Path, description: str) -> str:
    try:
        metadata = path.lstat()
    except FileNotFoundError as error:
        raise SmokeFailure(f"{description} was not found: {path}") from error
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
        raise SmokeFailure(f"{description} must be a regular, non-symlink file")
    if os.name != "nt" and stat.S_IMODE(metadata.st_mode) & 0o077:
        raise SmokeFailure(
            f"{description} permissions must be 0600 or stricter: {path}"
        )
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise SmokeFailure(f"could not read {description}: {path}") from error


def load_runtime(path: Path) -> tuple[str, str]:
    try:
        runtime = json.loads(read_private_text(path, "daemon runtime descriptor"))
    except json.JSONDecodeError as error:
        raise SmokeFailure("daemon runtime descriptor contains invalid JSON") from error
    if not isinstance(runtime, dict):
        raise SmokeFailure("daemon runtime descriptor must contain a JSON object")
    api_base_url = runtime.get("apiBaseUrl")
    token_path_value = runtime.get("tokenPath")
    if not isinstance(api_base_url, str) or not isinstance(token_path_value, str):
        raise SmokeFailure(
            "daemon runtime descriptor is missing apiBaseUrl or tokenPath"
        )
    parsed = urllib.parse.urlparse(api_base_url)
    if (
        parsed.scheme != "http"
        or parsed.hostname not in {"127.0.0.1", "::1", "localhost"}
        or parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
    ):
        raise SmokeFailure(
            "daemon apiBaseUrl must be an unauthenticated loopback HTTP URL"
        )
    token_path = Path(token_path_value)
    if not token_path.is_absolute():
        raise SmokeFailure("daemon tokenPath must be absolute")
    token = read_private_text(token_path, "daemon bearer token").strip()
    if not token or any(character.isspace() for character in token):
        raise SmokeFailure("daemon bearer token is empty or malformed")
    return api_base_url.rstrip("/"), token


def request_json(
    api_base_url: str,
    token: str,
    method: str,
    path: str,
    *,
    body: dict[str, Any] | None = None,
) -> dict[str, Any]:
    encoded = None if body is None else json.dumps(body).encode("utf-8")
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/json",
    }
    if encoded is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(
        f"{api_base_url}{path}", data=encoded, headers=headers, method=method
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            payload = response.read(MAX_RESPONSE_BYTES + 1)
    except urllib.error.HTTPError as error:
        payload = error.read(MAX_RESPONSE_BYTES + 1)
        detail = ""
        try:
            decoded = json.loads(payload[:MAX_RESPONSE_BYTES])
            daemon_error = decoded.get("error", {})
            code = daemon_error.get("code", "unknown")
            message = daemon_error.get("message", "request failed")
            detail = f" {code}: {message}"
        except (UnicodeDecodeError, json.JSONDecodeError, AttributeError):
            pass
        raise SmokeFailure(
            f"daemon {method} {path} returned HTTP {error.code}.{detail}"
        ) from error
    except urllib.error.URLError as error:
        raise SmokeFailure(
            f"could not reach the loopback daemon: {error.reason}"
        ) from error
    if len(payload) > MAX_RESPONSE_BYTES:
        raise SmokeFailure("daemon JSON response exceeded the 8 MiB smoke-test limit")
    try:
        value = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SmokeFailure(f"daemon {method} {path} returned invalid JSON") from error
    if not isinstance(value, dict):
        raise SmokeFailure(f"daemon {method} {path} returned a non-object JSON value")
    return value


def provider_kind(provider: str, explicit_kind: str | None) -> str:
    expected = "music" if provider == "suno" else "video"
    if explicit_kind is not None and explicit_kind != expected:
        raise SmokeFailure(f"provider {provider} smoke kind must be {expected}")
    return expected


def parse_options(value: str, timeout_seconds: int) -> dict[str, Any]:
    try:
        options = json.loads(value)
    except json.JSONDecodeError as error:
        raise SmokeFailure("--options-json must contain valid JSON") from error
    if not isinstance(options, dict):
        raise SmokeFailure("--options-json must contain a JSON object")
    options.setdefault("timeoutSeconds", timeout_seconds)
    return options


def assert_provider_available(
    catalog: dict[str, Any], provider: str, kind: str
) -> dict[str, Any]:
    providers = catalog.get("data", {}).get("providers", [])
    if not isinstance(providers, list):
        raise SmokeFailure("generator catalog contains no provider list")
    descriptor = next(
        (
            candidate
            for candidate in providers
            if isinstance(candidate, dict) and candidate.get("id") == provider
        ),
        None,
    )
    if descriptor is None:
        raise SmokeFailure(f"provider {provider} is not present in the daemon catalog")
    availability = descriptor.get("availability", {})
    if not isinstance(availability, dict) or availability.get("state") != "available":
        state = (
            availability.get("state", "unknown")
            if isinstance(availability, dict)
            else "unknown"
        )
        raise SmokeFailure(f"provider {provider} is not available (state={state})")
    capability = "musicGeneration" if kind == "music" else "videoGeneration"
    adapters = descriptor.get("adapters", [])
    adapter = next(
        (
            candidate
            for candidate in adapters
            if isinstance(candidate, dict)
            and candidate.get("capability") == capability
            and isinstance(candidate.get("transport"), dict)
            and candidate["transport"].get("type") == "http"
        ),
        None,
    )
    if adapter is None or adapter.get("requiresNetwork") is not True:
        raise SmokeFailure(
            f"provider {provider} does not expose the expected paid HTTP {capability} adapter"
        )
    return descriptor


def cancel_job(api_base_url: str, token: str, job_id: str) -> None:
    try:
        request_json(
            api_base_url,
            token,
            "POST",
            f"/jobs/{urllib.parse.quote(job_id, safe='')}/cancel",
        )
    except SmokeFailure as error:
        print(
            f"warning: could not cancel provider job {job_id}: {error}", file=sys.stderr
        )


def wait_for_job(
    api_base_url: str,
    token: str,
    job_id: str,
    timeout_seconds: int,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds + 60
    last_status: tuple[str, int, str] | None = None
    encoded_job_id = urllib.parse.quote(job_id, safe="")
    while time.monotonic() < deadline:
        response = request_json(api_base_url, token, "GET", f"/jobs/{encoded_job_id}")
        job = response.get("job")
        if not isinstance(job, dict):
            raise SmokeFailure("daemon job response is missing the job record")
        state = str(job.get("state", "unknown"))
        raw_progress = job.get("progress", 0)
        progress = (
            int(float(raw_progress) * 100)
            if isinstance(raw_progress, (int, float))
            else 0
        )
        message = str(job.get("message") or "")
        status = (state, progress, message)
        if status != last_status:
            print(f"provider job {job_id}: {state} {progress}% {message}".rstrip())
            last_status = status
        if state in TERMINAL_JOB_STATES:
            return job
        time.sleep(1)
    cancel_job(api_base_url, token, job_id)
    raise SmokeFailure(
        f"provider job {job_id} exceeded {timeout_seconds + 60} seconds and cancellation was requested"
    )


def validate_materialized_asset(
    envelope_response: dict[str, Any],
    job: dict[str, Any],
    provider: str,
    kind: str,
    prompt: str,
) -> tuple[dict[str, Any], int]:
    if job.get("state") != "succeeded":
        error = job.get("error")
        raise SmokeFailure(f"provider job ended in {job.get('state')}: {error}")
    output = job.get("output")
    if not isinstance(output, dict):
        raise SmokeFailure("succeeded provider job has no persisted output")
    output_asset = output.get("asset")
    if not isinstance(output_asset, dict) or not isinstance(
        output_asset.get("id"), str
    ):
        raise SmokeFailure("succeeded provider job has no materialized asset")
    envelope = envelope_response.get("envelope")
    if not isinstance(envelope, dict):
        raise SmokeFailure("project read returned no envelope")
    revision = envelope.get("revision")
    document = envelope.get("document")
    if not isinstance(revision, int) or revision < 1 or not isinstance(document, dict):
        raise SmokeFailure("provider generation did not create a project revision")
    assets = document.get("assets")
    if not isinstance(assets, list):
        raise SmokeFailure("generated project contains no asset collection")
    asset = next(
        (
            candidate
            for candidate in assets
            if isinstance(candidate, dict) and candidate.get("id") == output_asset["id"]
        ),
        None,
    )
    if asset is None:
        raise SmokeFailure(
            "provider job output asset is absent from the project envelope"
        )
    digest = asset.get("contentHash")
    if not isinstance(digest, str) or len(digest) != 64:
        raise SmokeFailure(
            "generated asset is not backed by a SHA-256 managed-media digest"
        )
    expected_asset_kind = "audio" if kind == "music" else "video"
    if asset.get("kind") != expected_asset_kind:
        raise SmokeFailure(
            f"generated asset kind is {asset.get('kind')}, expected {expected_asset_kind}"
        )
    provenance = asset.get("provenance")
    if (
        not isinstance(provenance, dict)
        or provenance.get("type") != "generated"
        or provenance.get("provider") != provider
        or provenance.get("prompt") != prompt
    ):
        raise SmokeFailure(
            "generated asset provenance does not match the approved request"
        )
    managed = asset.get("managedMedia")
    if (
        not isinstance(managed, dict)
        or not isinstance(managed.get("byteSize"), int)
        or managed.get("byteSize", 0) <= 0
        or not isinstance(managed.get("normalization"), str)
    ):
        raise SmokeFailure(
            "generated asset lacks verified managed-media normalization metadata"
        )
    return asset, revision


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run one explicitly approved real paid-provider generation smoke."
    )
    parser.add_argument("--provider", required=True, choices=REMOTE_PROVIDERS)
    parser.add_argument("--kind", choices=("video", "music"))
    parser.add_argument("--model")
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--options-json", default="{}")
    parser.add_argument("--timeout-seconds", type=int, default=1800)
    parser.add_argument(
        "--runtime-descriptor", type=Path, default=default_runtime_descriptor()
    )
    parser.add_argument("--report", type=Path)
    parser.add_argument(
        "--delete-project",
        action="store_true",
        help="Delete the disposable project after validation. Managed bytes remain GC-protected.",
    )
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    require_paid_confirmation()
    if not 30 <= args.timeout_seconds <= 3600:
        raise SmokeFailure("--timeout-seconds must be between 30 and 3600")
    if not args.prompt.strip() or len(args.prompt.encode("utf-8")) > 20_000:
        raise SmokeFailure("--prompt must contain 1 to 20000 UTF-8 bytes")
    kind = provider_kind(args.provider, args.kind)
    options = parse_options(args.options_json, args.timeout_seconds)
    api_base_url, token = load_runtime(args.runtime_descriptor.expanduser())
    catalog = request_json(
        api_base_url,
        token,
        "POST",
        "/tools/list_generators",
        body={"arguments": {"kind": kind}},
    )
    descriptor = assert_provider_available(catalog, args.provider, kind)
    if (
        args.model
        and args.model not in descriptor.get("models", [])
        and descriptor.get("models")
    ):
        raise SmokeFailure(
            f"model {args.model!r} is not advertised by provider {args.provider}"
        )

    suffix = uuid.uuid4().hex
    project_id = f"paid-smoke:{suffix}"
    idempotency_key = f"paid-smoke:create:{suffix}"
    created = request_json(
        api_base_url,
        token,
        "POST",
        "/projects",
        body={
            "projectId": project_id,
            "name": f"Paid provider smoke - {args.provider}",
            "idempotencyKey": idempotency_key,
        },
    )
    revision = created.get("envelope", {}).get("revision")
    if revision != 0:
        raise SmokeFailure(
            "disposable provider-smoke project did not start at revision 0"
        )

    arguments: dict[str, Any] = {
        "projectId": project_id,
        "expectedRevision": 0,
        "kind": kind,
        "provider": args.provider,
        "prompt": args.prompt,
        "confirm": True,
        "options": options,
    }
    if args.model:
        arguments["model"] = args.model
    print(
        f"Submitting explicitly approved paid smoke to {args.provider} "
        f"(kind={kind}, project={project_id})"
    )
    submitted = request_json(
        api_base_url,
        token,
        "POST",
        "/tools/generate_asset",
        body={
            "idempotencyKey": f"paid-smoke:generate:{suffix}",
            "arguments": arguments,
        },
    )
    job_id = submitted.get("jobId")
    if not isinstance(job_id, str) or not job_id:
        raise SmokeFailure("provider submission returned no durable job ID")
    started = time.monotonic()
    try:
        job = wait_for_job(
            api_base_url, token, job_id, timeout_seconds=args.timeout_seconds
        )
    except KeyboardInterrupt:
        cancel_job(api_base_url, token, job_id)
        raise SmokeFailure(
            "provider smoke interrupted; cancellation was requested"
        ) from None

    encoded_project_id = urllib.parse.quote(project_id, safe="")
    project = request_json(
        api_base_url, token, "GET", f"/projects/{encoded_project_id}"
    )
    asset, final_revision = validate_materialized_asset(
        project, job, args.provider, kind, args.prompt
    )
    report = {
        "ok": True,
        "provider": args.provider,
        "kind": kind,
        "model": asset.get("provenance", {}).get("model"),
        "projectId": project_id,
        "revision": final_revision,
        "jobId": job_id,
        "assetId": asset["id"],
        "assetKind": asset["kind"],
        "contentHash": asset["contentHash"],
        "byteSize": asset["managedMedia"]["byteSize"],
        "normalization": asset["managedMedia"]["normalization"],
        "promptSha256": hashlib.sha256(args.prompt.encode("utf-8")).hexdigest(),
        "elapsedSeconds": round(time.monotonic() - started, 3),
    }
    if args.delete_project:
        delete_key = f"paid-smoke:delete:{suffix}"
        request_json(
            api_base_url,
            token,
            "DELETE",
            f"/projects/{encoded_project_id}",
            body={
                "expectedRevision": final_revision,
                "idempotencyKey": delete_key,
            },
        )
        report["projectDeleted"] = True
    else:
        report["projectDeleted"] = False
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    return report


def main() -> int:
    try:
        report = run(build_parser().parse_args())
    except SmokeFailure as error:
        print(f"Paid provider smoke failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    print("Paid provider submit/poll/download/normalize/materialize smoke passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
