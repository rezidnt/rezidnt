#!/usr/bin/env bash
# Ontology gate: spec/ontology.md is edited only inside a /subject session (warden).
command -v python3 >/dev/null || { echo "ontology-gate: python3 missing, NOT enforced" >&2; exit 0; }
REZ_IN="$(cat)"; export REZ_IN
python3 -c '
import json,os,sys
try: d=json.loads(os.environ.get("REZ_IN","") or "{}")
except Exception: sys.exit(0)
fp=str((d.get("tool_input") or {}).get("file_path","")).replace("\\","/")
if fp.endswith("spec/ontology.md") and not os.path.exists(".claude/state/ontology-session"):
    print(json.dumps({"decision":"block","reason":"Ontology is BINDING surface. Route through /subject: the warden opens a session (creates .claude/state/ontology-session), applies the subject checklist, then closes it. Subjects are never renamed; payload changes are additive or bump v."}))
'
exit 0
