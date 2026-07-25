#!/usr/bin/env bash
# DR-002 copyleft firewall: block reads/fetches/clones of herdr (AGPL) sources.
command -v python3 >/dev/null || { echo "firewall: python3 missing, NOT enforced" >&2; exit 0; }
REZ_IN="$(cat)"; export REZ_IN
python3 -c '
import json,os,re,sys
try: d=json.loads(os.environ.get("REZ_IN","") or "{}")
except Exception: sys.exit(0)
ti=d.get("tool_input",{}) or {}
hay=" ".join(str(ti.get(k,"")) for k in ("url","file_path","command","query","prompt"))
pats=[r"github\.com/[^ ]*herdr", r"codeberg\.org/[^ ]*herdr", r"git\s+clone[^\n]*herdr",
      r"(^|/)herdr[^ ]*\.(rs|zig|c|h)\b", r"raw\.githubusercontent\.com/[^ ]*herdr"]
if any(re.search(p,hay,re.I) for p in pats):
    print(json.dumps({"decision":"block","reason":"DR-002 copyleft firewall: herdr (AGPL) sources are never read, fetched, or cloned for implementation. Black-box behavior via the benchmark is permitted; source is not. False positive? Amend patterns via a DR."}))
'
exit 0
