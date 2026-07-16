#!/usr/bin/env bash
# IP-memo gate: block `git push` until the employer memo + carve-out are executed.
command -v python3 >/dev/null || { echo "pushgate: python3 missing, NOT enforced" >&2; exit 0; }
REZ_IN="$(cat)"; export REZ_IN
python3 -c '
import json,os,re,sys
try: d=json.loads(os.environ.get("REZ_IN","") or "{}")
except Exception: sys.exit(0)
cmd=str((d.get("tool_input") or {}).get("command",""))
if re.search(r"\bgit\b[^\n|;&]*\bpush\b", cmd):
    if os.environ.get("REZIDNT_ALLOW_PUSH")=="1" or os.path.exists(".claude/state/ip-memo-cleared"): sys.exit(0)
    print(json.dumps({"decision":"block","reason":"Push blocked: the employer IP memo and carve-out letter are not marked executed. Repo stays private-only until then (standing gate). When signed: touch .claude/state/ip-memo-cleared"}))
'
exit 0
