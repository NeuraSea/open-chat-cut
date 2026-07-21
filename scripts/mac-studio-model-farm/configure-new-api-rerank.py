#!/usr/bin/env python3
"""Idempotently route the OpenChatCut rerank alias through New API."""

from __future__ import annotations

import argparse
import base64
from pathlib import Path
import subprocess
import sys


POSTGRES_POD_PREFIX = "pod/api-postgresql-"


def run(*arguments: str, input_text: str | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        arguments,
        input=input_text,
        text=True,
        check=True,
        capture_output=True,
    )


def find_primary(namespace: str) -> str:
    pods = run(
        "kubectl",
        "-n",
        namespace,
        "get",
        "pods",
        "-o",
        "name",
    ).stdout.splitlines()
    candidates = sorted(pod for pod in pods if pod.startswith(POSTGRES_POD_PREFIX))
    if not candidates:
        raise RuntimeError("New API PostgreSQL pods were not found")

    for pod in candidates:
        probe = run(
            "kubectl",
            "-n",
            namespace,
            "exec",
            pod,
            "--",
            "psql",
            "-d",
            "newapi",
            "-Atc",
            "select pg_is_in_recovery();",
        )
        if probe.stdout.strip() == "f":
            return pod
    raise RuntimeError("New API PostgreSQL primary was not found")


def configure(namespace: str, token_file: Path) -> None:
    token = token_file.read_text(encoding="utf-8").strip()
    if len(token) < 24:
        raise RuntimeError("rerank service token is missing or too short")
    token_base64 = base64.b64encode(token.encode()).decode("ascii")
    primary = find_primary(namespace)

    # Keep the credential out of argv and logs. It is decoded only by PostgreSQL
    # from the psql stdin stream and stored as the channel key.
    sql = f"""
BEGIN;

UPDATE channels
SET models = array_to_string(
        array_remove(string_to_array(models, ','), 'occ-rerank'),
        ','
    ),
    model_mapping = (COALESCE(NULLIF(model_mapping, ''), '{{}}')::jsonb
        - 'occ-rerank')::text
WHERE id = 24;

DELETE FROM abilities
WHERE channel_id = 24 AND model = 'occ-rerank';

INSERT INTO channels (
    type,
    key,
    test_model,
    status,
    name,
    weight,
    created_time,
    base_url,
    models,
    "group",
    model_mapping,
    priority,
    auto_ban,
    remark,
    channel_info,
    setting,
    settings
)
SELECT
    38,
    convert_from(decode('{token_base64}', 'base64'), 'UTF8'),
    'occ-rerank',
    1,
    'OpenChatCut Rerank',
    0,
    extract(epoch FROM now())::bigint,
    'http://host.docker.internal:8190',
    'occ-rerank',
    'default,vip,svip',
    '{{"occ-rerank":"BAAI/bge-reranker-v2-m3"}}',
    0,
    0,
    'Private Mac Studio BGE reranker',
    '{{"is_multi_key":false,"multi_key_size":0,"multi_key_status_list":null,"multi_key_polling_index":0,"multi_key_mode":"random"}}'::json,
    '{{"force_format":false,"thinking_to_content":false,"proxy":"","pass_through_body_enabled":false,"system_prompt":"","system_prompt_override":false}}',
    '{{"allow_service_tier":false,"disable_store":false,"allow_safety_identifier":false,"allow_include_obfuscation":false,"upstream_model_update_check_enabled":false,"upstream_model_update_auto_sync_enabled":false,"upstream_model_update_ignored_models":[],"upstream_model_update_last_detected_models":[],"upstream_model_update_last_check_time":0}}'
WHERE NOT EXISTS (
    SELECT 1 FROM channels WHERE name = 'OpenChatCut Rerank'
);

UPDATE channels
SET type = 38,
    key = convert_from(decode('{token_base64}', 'base64'), 'UTF8'),
    test_model = 'occ-rerank',
    status = 1,
    base_url = 'http://host.docker.internal:8190',
    models = 'occ-rerank',
    "group" = 'default,vip,svip',
    model_mapping = '{{"occ-rerank":"BAAI/bge-reranker-v2-m3"}}',
    auto_ban = 0,
    remark = 'Private Mac Studio BGE reranker'
WHERE name = 'OpenChatCut Rerank';

DELETE FROM abilities
WHERE channel_id IN (
    SELECT id FROM channels WHERE name = 'OpenChatCut Rerank'
) AND model <> 'occ-rerank';

INSERT INTO abilities ("group", model, channel_id, enabled, priority, weight, tag)
SELECT desired_group, 'occ-rerank', channel.id, true, channel.priority,
       channel.weight, channel.tag
FROM channels AS channel
CROSS JOIN (VALUES ('default'), ('vip'), ('svip')) AS groups(desired_group)
WHERE channel.name = 'OpenChatCut Rerank'
ON CONFLICT ("group", model, channel_id)
DO UPDATE SET
    enabled = EXCLUDED.enabled,
    priority = EXCLUDED.priority,
    weight = EXCLUDED.weight,
    tag = EXCLUDED.tag;

COMMIT;
"""
    try:
        result = run(
            "kubectl",
            "-n",
            namespace,
            "exec",
            "-i",
            primary,
            "--",
            "psql",
            "-d",
            "newapi",
            "-v",
            "ON_ERROR_STOP=1",
            input_text=sql,
        )
    finally:
        # Minimize the lifetime of the immutable Python strings containing the
        # credential. This is not a substitute for process-level isolation.
        token = ""
        token_base64 = ""
        sql = ""

    if "COMMIT" not in result.stdout:
        raise RuntimeError("New API did not confirm the rerank transaction")
    print(f"Configured OpenChatCut rerank on {primary}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--namespace",
        default="enterprise-llm-proxy",
    )
    parser.add_argument(
        "--token-file",
        type=Path,
        default=Path("/Volumes/External/openchatcut-models/config/rerank.token"),
    )
    arguments = parser.parse_args()

    try:
        configure(arguments.namespace, arguments.token_file)
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"Failed to configure New API rerank: {error}", file=sys.stderr)
        raise SystemExit(1) from error


if __name__ == "__main__":
    main()
