#!/usr/bin/env python3
"""Idempotently expose the loopback Qwen Image service through New API."""

from __future__ import annotations

import argparse
import base64
import os
from pathlib import Path
import shutil
import subprocess
import sys


KUBECTL = (
    os.environ.get("KUBECTL_BIN")
    or shutil.which("kubectl")
    or "/Applications/OrbStack.app/Contents/MacOS/xbin/kubectl"
)


def run(*arguments: str, input_text: str | None = None) -> subprocess.CompletedProcess[str]:
    command = list(arguments)
    if command and command[0] == "kubectl":
        command[0] = KUBECTL
    return subprocess.run(
        command, input=input_text, text=True, check=True, capture_output=True
    )


def find_primary(namespace: str) -> str:
    pods = run("kubectl", "-n", namespace, "get", "pods", "-o", "name").stdout.splitlines()
    for pod in sorted(pod for pod in pods if pod.startswith("pod/api-postgresql-")):
        probe = run(
            "kubectl", "-n", namespace, "exec", pod, "--", "psql", "-d", "newapi",
            "-Atc", "select pg_is_in_recovery();",
        )
        if probe.stdout.strip() == "f":
            return pod
    raise RuntimeError("New API PostgreSQL primary was not found")


def configure(namespace: str, token_file: Path) -> None:
    token = token_file.read_text(encoding="utf-8").strip()
    if len(token) < 24:
        raise RuntimeError("image service token is missing or too short")
    encoded = base64.b64encode(token.encode()).decode("ascii")
    primary = find_primary(namespace)
    sql = f"""
BEGIN;
INSERT INTO channels (
    type, key, test_model, status, name, weight, created_time, base_url,
    models, "group", model_mapping, priority, auto_ban, remark,
    channel_info, setting, settings
)
SELECT 1, convert_from(decode('{encoded}', 'base64'), 'UTF8'),
    'occ-image', 1, 'OpenChatCut Image', 0, extract(epoch FROM now())::bigint,
    'http://host.docker.internal:8193', 'occ-image', 'default,vip,svip',
    '{{"occ-image":"Qwen-Image-2512"}}', 0, 0,
    'Private Mac Studio Qwen Image 2512 service',
    '{{"is_multi_key":false,"multi_key_size":0,"multi_key_status_list":null,"multi_key_polling_index":0,"multi_key_mode":"random"}}'::json,
    '{{"force_format":false,"thinking_to_content":false,"proxy":"","pass_through_body_enabled":false,"system_prompt":"","system_prompt_override":false}}',
    '{{"allow_service_tier":false,"disable_store":false,"allow_safety_identifier":false,"allow_include_obfuscation":false,"upstream_model_update_check_enabled":false,"upstream_model_update_auto_sync_enabled":false,"upstream_model_update_ignored_models":[],"upstream_model_update_last_detected_models":[],"upstream_model_update_last_check_time":0}}'
WHERE NOT EXISTS (SELECT 1 FROM channels WHERE name = 'OpenChatCut Image');
UPDATE channels SET type=1,
    key=convert_from(decode('{encoded}', 'base64'), 'UTF8'), test_model='occ-image',
    status=1, base_url='http://host.docker.internal:8193', models='occ-image',
    "group"='default,vip,svip', model_mapping='{{"occ-image":"Qwen-Image-2512"}}',
    auto_ban=0, remark='Private Mac Studio Qwen Image 2512 service'
WHERE name='OpenChatCut Image';
DELETE FROM abilities WHERE channel_id IN (
    SELECT id FROM channels WHERE name='OpenChatCut Image'
) AND model <> 'occ-image';
INSERT INTO abilities ("group", model, channel_id, enabled, priority, weight, tag)
SELECT desired_group, 'occ-image', channel.id, true, channel.priority, channel.weight, channel.tag
FROM channels AS channel
CROSS JOIN (VALUES ('default'), ('vip'), ('svip')) AS groups(desired_group)
WHERE channel.name='OpenChatCut Image'
ON CONFLICT ("group", model, channel_id)
DO UPDATE SET enabled=EXCLUDED.enabled, priority=EXCLUDED.priority,
              weight=EXCLUDED.weight, tag=EXCLUDED.tag;
COMMIT;
"""
    try:
        result = run(
            "kubectl", "-n", namespace, "exec", "-i", primary, "--",
            "psql", "-d", "newapi", "-v", "ON_ERROR_STOP=1", input_text=sql,
        )
    finally:
        token = encoded = sql = ""
    if "COMMIT" not in result.stdout:
        raise RuntimeError("New API did not confirm the image transaction")
    print(f"Configured OpenChatCut Image on {primary}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--namespace", default="enterprise-llm-proxy")
    parser.add_argument(
        "--token-file", type=Path,
        default=Path("/Volumes/External/openchatcut-models/config/image.token"),
    )
    arguments = parser.parse_args()
    try:
        configure(arguments.namespace, arguments.token_file)
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"Failed to configure New API Image: {error}", file=sys.stderr)
        raise SystemExit(1) from error


if __name__ == "__main__":
    main()
