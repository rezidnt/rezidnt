#!/usr/bin/env bash
# Self-test for vet.sh — the gate that decides this repo's definition of done.
#
# Why this exists: every claim about vet.sh's behaviour was, until now, established by hand-runs
# in temp directories that evaporated. That is the one place where I6's "deterministic and
# interrogable, replayable" did not reach the artifact enforcing I6. This is that replay.
#
#   bash .claude/hooks/vet-selftest.sh          # everything
#   bash .claude/hooks/vet-selftest.sh --quick  # skip the lanes that compile or sleep
#
# It runs the REAL vet.sh, and sources its REAL helpers (VET_LIB_ONLY=1), never a copy. Verdicts
# are the assertion: a change that alters what /vet reports fails here. It is deliberately not
# wired into vet.sh — the gate must not pay for its own test on every inner-loop run.
set -o pipefail
cd "$(dirname "$0")/../.." || exit 1
ROOT=$(pwd); VET="$ROOT/.claude/hooks/vet.sh"; T="$ROOT/.claude/hooks/tests"
QUICK=0; [ "$1" = "--quick" ] && QUICK=1
PASS=0; FAILED=0; SKIP=0
ok(){ PASS=$((PASS+1)); echo "  ok   $1"; }
no(){ FAILED=$((FAILED+1)); echo "  FAIL $1"; echo "         expected: $2"; echo "         actual:   $3"; }
skip(){ SKIP=$((SKIP+1)); echo "  skip $1 ($2)"; }
is(){ [ "$2" = "$3" ] && ok "$1" || no "$1" "$2" "$3"; }
has(){ case "$3" in *"$2"*) ok "$1" ;; *) no "$1" "*$2*" "$3" ;; esac; }
hasnt(){ case "$3" in *"$2"*) no "$1" "NOT *$2*" "$3" ;; *) ok "$1" ;; esac; }

# A scratch workspace that satisfies vet.sh's preconditions (a Cargo.toml, a fixtures script)
# and puts the shim ahead of real cargo on PATH.
SANDBOX=$(mktemp -d 2>/dev/null || mktemp -d -t vetself)
trap 'pkill -f "sleep 600" >/dev/null 2>&1; rm -rf "$SANDBOX"' EXIT
mkdir -p "$SANDBOX/scripts"
printf '[workspace]\n' > "$SANDBOX/Cargo.toml"
printf '#!/usr/bin/env bash\nexit 0\n' > "$SANDBOX/scripts/replay-fixtures.sh"
mkdir -p "$SANDBOX/bin"; cp "$T/shim/cargo" "$SANDBOX/bin/cargo"; chmod +x "$SANDBOX/bin/cargo" "$SANDBOX/scripts/replay-fixtures.sh"
# vet.sh runs `cargo`; the shim answers it. Verdict JSON is the last stdout line.
vet(){ ( cd "$SANDBOX" && PATH="$SANDBOX/bin:$PATH" env "$@" bash "$VET" 2>/dev/null | tail -1 ); }

echo "== parser (roster) — against REAL captured libtest output"
# shellcheck source=/dev/null
VET_LIB_ONLY=1 . "$VET" || { echo "cannot source vet.sh"; exit 1; }
R=$(roster "$T/logs/real-multicrate.log")
is "3 failures across 2 crates, 4 roster blocks, exact set" \
   "tests::alpha_chain_mismatch tests::alpha_rebuild_diverges tests::beta_fold_mismatch " "$R"
hasnt "panic/dump lines are not mistaken for test names" "panicked" "$R"
hasnt "the '---- name stdout ----' header is not a source" "----" "$R"
printf 'failures:\n    only::in::roster\n\ntest result: FAILED. 0 passed; 1 failed;\n' > "$SANDBOX/nohdr.log"
is "a failing test with an EMPTY capture buffer is still named" "only::in::roster " "$(roster "$SANDBOX/nohdr.log")"
printf 'no failures here\ntest result: ok. 1 passed; 0 failed;\n' > "$SANDBOX/green.log"
is "a green log yields no names" "" "$(roster "$SANDBOX/green.log")"

echo "== escaper (esc) — a verdict object must stay parseable"
is 'double quotes stripped'  'ab'   "$(esc 'a"b')"
is 'backslashes stripped'    'ab'   "$(esc 'a\b')"
is 'newlines stripped'       'ab'   "$(esc "$(printf 'a\nb')")"
J="{\"verdict\":\"fail\",\"evidence\":[{\"kind\":\"test\",\"msg\":\"$(esc 'we"ird\path')\"}]}"
if command -v python >/dev/null 2>&1; then
  python -c "import json,sys; json.loads(sys.argv[1])" "$J" 2>/dev/null \
    && ok "a C-quoted path still yields valid JSON" || no "a C-quoted path still yields valid JSON" "parses" "$J"
else skip "JSON validity" "no python"; fi

echo "== verdicts — the real vet.sh, driven by the shim"
is "green tests            -> pass, evidence empty" '{"verdict":"pass","evidence":[]}' "$(vet MODE=green)"
V=$(vet MODE=red)
has "failing tests         -> fail"            '"verdict":"fail"' "$V"
has "  ...and both names are reported"         'fabric::chain_verifies state::fold_equals_snapshot' "$V"
has "  ...and cites an absolute log path"      '/vet/test-' "$V"
has "empty capture buffer  -> still named"     'gate::inconclusive_never_coerced' "$(vet MODE=red-empty-capture)"

echo "== could-not-run is never coerced to fail (I6)"
# Driven on run_test directly: a whole-script run cannot isolate this, because fmt/clippy would
# also fail and decide the verdict. Scope is the test lane, which is where the claim was made.
CNR=$( FAIL=0; NOTES=""; EXTRA=""; VETLOG="$SANDBOX/cnr"; VET_TEST_TIMEOUT=20
       run_test "test" definitely-not-a-real-binary-xyz >/dev/null 2>&1
       printf '%s|%s' "$FAIL" "$NOTES" )
is  "command not found (127) does not set FAIL" "0|test-noverdict-absent;" "$CNR"
CNR=$( FAIL=0; NOTES=""; EXTRA=""; VETLOG="$SANDBOX/cnr"; VET_TEST_TIMEOUT=20
       printf 'x' > "$SANDBOX/noexec"; chmod -x "$SANDBOX/noexec" 2>/dev/null
       run_test "test" "$SANDBOX/noexec" >/dev/null 2>&1
       printf '%s|%s' "$FAIL" "$NOTES" )
is  "non-executable command does not set FAIL"  "0|test-noverdict-absent;" "$CNR"

echo "== capture failure never invents a verdict about the tests"
printf 'x' > "$SANDBOX/blocker"
V=$(vet MODE=green CARGO_TARGET_DIR="$SANDBOX/blocker/nope")
has "unwritable log dir + green tests -> still pass" '"verdict":"pass"' "$V"
has "  ...and the pass discloses the capture gap"    'test-uncaptured;' "$V"
V=$(vet MODE=red CARGO_TARGET_DIR="$SANDBOX/blocker/nope")
has "unwritable log dir + red tests   -> fail, degraded honestly" 'test-uncaptured;test;' "$V"
hasnt "  ...and cites no log path it did not write" '/vet/test-' "$V"
rm -f "$SANDBOX/blocker"

echo "== concurrent gauntlets sharing one CARGO_TARGET_DIR"
SHARED="$SANDBOX/shared"; rm -rf "$SHARED"
( vet MODE=red CARGO_TARGET_DIR="$SHARED" >/dev/null ) & ( vet MODE=red-empty-capture CARGO_TARGET_DIR="$SHARED" >/dev/null ) & wait
is "two red runs leave two distinct logs" "2" "$(find "$SHARED/vet" -name '*.log' 2>/dev/null | wc -l | tr -d ' ')"

if [ $QUICK -eq 1 ]; then echo "== (skipping bounded-wait and real-cargo lanes: --quick)"; SKIP=$((SKIP+2));
else
echo "== bounded waits (these sleep; ~30s)"
V=$(vet MODE=wedged-partial VET_TEST_TIMEOUT=3)
has "wedged command      -> inconclusive, not fail" '"verdict":"inconclusive"' "$V"
has "  ...and says no verdict exists"               'no verdict' "$V"
CITED=$(printf '%s' "$V" | sed -n 's/.*log: \([^"]*\)".*/\1/p')
if [ -s "$CITED" ]; then
  vet MODE=green >/dev/null; vet MODE=red >/dev/null
  [ -s "$CITED" ] && ok "  ...and its partial evidence survives later vet runs" \
                  || no "  ...and its partial evidence survives later vet runs" "still readable" "gone"
else no "  ...cites a readable partial log" "non-empty file" "$CITED"; fi
pkill -f "sleep 600" >/dev/null 2>&1
S=$(date +%s)
V=$(vet MODE=leak VET_TEST_TIMEOUT=600 VET_LEAK_GRACE=5)
E=$(( $(date +%s) - S ))
has "leaked child, green tests -> pass (tests spoke; only capture stalled)" '"verdict":"pass"' "$V"
has "  ...and the pass carries its own note"                                'capture-degraded' "$V"
[ "$E" -lt 120 ] && ok "  ...bounded by the leak grace, not the 600s outer bound (${E}s)" \
                || no "  ...bounded by the leak grace" "<120s" "${E}s"
pkill -f "sleep 600" >/dev/null 2>&1

echo "== end-to-end against REAL cargo (compiles the fixture workspace)"
if command -v cargo >/dev/null 2>&1; then
  V=$( cd "$T/fixture-ws" && bash "$VET" 2>/dev/null | tail -1 )
  has "deliberately-red fixture workspace -> fail" '"verdict":"fail"' "$V"
  has "  ...names the real failing tests"          'tests::alpha_chain_mismatch tests::alpha_rebuild_diverges' "$V"
else skip "real-cargo end-to-end" "no cargo"; fi
fi

echo
echo "-- $PASS passed, $FAILED failed, $SKIP skipped"
[ "$FAILED" -eq 0 ] || exit 1
