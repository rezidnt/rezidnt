#!/usr/bin/env bash
# The local verifier gauntlet /vet runs. Verdict semantics: pass | fail | inconclusive (never coerce inconclusive to pass).
set -o pipefail
FAIL=0; NOTES=""
run(){ L="$1"; shift; echo "== $L"; "$@" || { FAIL=1; NOTES="$NOTES$L;"; }; }
[ -f Cargo.toml ] || { echo '{"verdict":"inconclusive","evidence":[{"kind":"env","msg":"no Cargo.toml at repo root - run from workspace root"}]}'; exit 0; }
run "fmt"    cargo fmt --all -- --check
run "clippy" cargo clippy --workspace --all-targets -- -D warnings
run "test"   cargo test --workspace --quiet
if [ -x scripts/replay-fixtures.sh ]; then run "fixtures" bash scripts/replay-fixtures.sh; else NOTES="${NOTES}fixtures-absent;"; fi
if [ $FAIL -eq 0 ] && [ "${NOTES#*absent}" = "$NOTES" ]; then echo '{"verdict":"pass","evidence":[]}';
elif [ $FAIL -eq 0 ]; then echo "{\"verdict\":\"inconclusive\",\"evidence\":[{\"kind\":\"gap\",\"msg\":\"$NOTES\"}]}";
else echo "{\"verdict\":\"fail\",\"evidence\":[{\"kind\":\"gauntlet\",\"msg\":\"$NOTES\"}]}"; exit 2; fi
