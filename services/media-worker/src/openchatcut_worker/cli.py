from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from .errors import WorkerError
from .hardware import capabilities_json
from .protocol import JobRequest, WorkerEvent
from .runner import JobRunner


def emit_json(event: WorkerEvent) -> None:
    print(json.dumps(event.to_dict(), ensure_ascii=False), flush=True)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="OpenChatCut native media worker")
    parser.add_argument(
        "--data-root",
        default=os.environ.get("OPENCHATCUT_DATA_DIR"),
        help="Authorized daemon data directory",
    )
    parser.add_argument(
        "--request-file",
        help="Read one JSON job request from a file instead of stdin",
    )
    parser.add_argument(
        "--probe-capabilities",
        action="store_true",
        help="Probe FFmpeg and hardware encoders, print one JSON document, and exit",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.probe_capabilities:
        print(capabilities_json(), flush=True)
        return 0
    if not args.data_root:
        print("OPENCHATCUT_DATA_DIR or --data-root is required", file=sys.stderr)
        return 2

    try:
        raw = (
            Path(args.request_file).read_text(encoding="utf-8")
            if args.request_file
            else sys.stdin.read()
        )
        request = JobRequest.from_dict(json.loads(raw))
        runner = JobRunner(data_root=Path(args.data_root), emit=emit_json)
        runner.run(request)
        return 0
    except WorkerError as error:
        job_id = request.job_id if "request" in locals() else "unknown"
        emit_json(
            WorkerEvent(
                job_id,
                "error",
                {
                    "error": {
                        "code": error.code,
                        "message": str(error),
                        "details": error.details,
                    }
                },
            )
        )
        return 1
    except (ValueError, json.JSONDecodeError) as error:
        emit_json(
            WorkerEvent(
                "unknown",
                "error",
                {"error": {"code": "INVALID_JOB_REQUEST", "message": str(error)}},
            )
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
