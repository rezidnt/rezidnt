#!/usr/bin/env bash
# Deterministic hygiene: rustfmt any edited .rs file. Always exit 0.
command -v python3 >/dev/null || exit 0
REZ_IN="$(cat)"; export REZ_IN
FP=$(python3 -c 'import json,os; d=json.loads(os.environ.get("REZ_IN","") or "{}"); print((d.get("tool_input") or {}).get("file_path",""))' 2>/dev/null)
case "$FP" in *.rs) command -v rustfmt >/dev/null && rustfmt --edition 2024 "$FP" 2>/dev/null || true;; esac
exit 0
