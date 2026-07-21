#!/usr/bin/env python3
"""Run the native worker directly from a source checkout for local development."""

from __future__ import annotations

import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "services/media-worker/src"))

from openchatcut_worker.cli import main  # noqa: E402


if __name__ == "__main__":
    raise SystemExit(main())
