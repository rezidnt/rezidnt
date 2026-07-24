#!/usr/bin/env bash
# The local verifier gauntlet /vet runs. Verdict semantics: pass | fail | inconclusive (never coerce inconclusive to pass).
# --fast: preflight + fmt + clippy only. Verdict is at best INCONCLUSIVE (tests not run) — the inner rework loop,
# never the done gate. A --fast run can fail conclusively; it can never pass.
set -o pipefail
FAST=0; [ "$1" = "--fast" ] && FAST=1
FAIL=0; NOTES=""
run(){ L="$1"; shift; echo "== $L"; "$@" || { FAIL=1; NOTES="$NOTES$L;"; }; }
[ -f Cargo.toml ] || { echo '{"verdict":"inconclusive","evidence":[{"kind":"env","msg":"no Cargo.toml at repo root - run from workspace root"}]}'; exit 0; }
# Fail-fast static trap check: only catches failures the later stages would hit anyway, minutes earlier.
if [ -x .claude/hooks/preflight.sh ] || [ -f .claude/hooks/preflight.sh ]; then
  bash .claude/hooks/preflight.sh || { echo '{"verdict":"fail","evidence":[{"kind":"gauntlet","msg":"preflight;"}]}'; exit 2; }
fi
run "fmt"    cargo fmt --all -- --check
run "clippy" cargo clippy --workspace --all-targets -- -D warnings
if [ $FAST -eq 1 ]; then
  if [ $FAIL -eq 0 ]; then echo '{"verdict":"inconclusive","evidence":[{"kind":"gap","msg":"fast-mode: tests+fixtures not run"}]}';
  else echo "{\"verdict\":\"fail\",\"evidence\":[{\"kind\":\"gauntlet\",\"msg\":\"$NOTES\"}]}"; exit 2; fi
  exit 0
fi
run "test"   cargo test --workspace --quiet
if [ -x scripts/replay-fixtures.sh ]; then run "fixtures" bash scripts/replay-fixtures.sh; else NOTES="${NOTES}fixtures-absent;"; fi
if [ $FAIL -eq 0 ] && [ "${NOTES#*absent}" = "$NOTES" ]; then echo '{"verdict":"pass","evidence":[]}';
elif [ $FAIL -eq 0 ]; then echo "{\"verdict\":\"inconclusive\",\"evidence\":[{\"kind\":\"gap\",\"msg\":\"$NOTES\"}]}";
else echo "{\"verdict\":\"fail\",\"evidence\":[{\"kind\":\"gauntlet\",\"msg\":\"$NOTES\"}]}"; exit 2; fi
