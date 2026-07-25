#!/usr/bin/env bash
# The local verifier gauntlet /vet runs. Verdict semantics: pass | fail | inconclusive (never coerce inconclusive to pass).
# --fast: preflight + fmt + clippy only. Verdict is at best INCONCLUSIVE (tests not run) — the inner rework loop,
# never the done gate. A --fast run can fail conclusively; it can never pass.
set -o pipefail
FAST=0; [ "$1" = "--fast" ] && FAST=1
FAIL=0; NOTES=""; EXTRA=""
run(){ L="$1"; shift; echo "== $L"; "$@" || { FAIL=1; NOTES="$NOTES$L;"; }; }
# Test-lane evidence capture. The test lane is the one whose failure carries a NAME, and
# a name that only ever reached the terminal dies with the scrollback (a real test-lane
# failure went undiagnosed for exactly this reason). Output is streamed AND retained under a
# per-run absolute path: kept whenever the lane produced no verdict or a red one, so no later
# run can overwrite it and no CWD is needed to find it; removed on green, which has nothing to
# diagnose and should leave no litter. Failing names reach the lane's own JSON evidence.
# Under CARGO_TARGET_DIR so a host run and a WSL run do not truncate one another's log.
# Two rules this obeys, because this script IS the done gate and answers to its own contract:
#   - capture failure NEVER invents a verdict about the tests (I6). If the log cannot be set
#     up, degrade to run() and report exactly what an uninstrumented run would have.
#   - the pipe is bounded. tee waits for EOF from every inheritor of the write end, and test
#     children spawn with inherited stdio (e.g. bins/rezidentd/tests/s3_exit_demo_e2e.rs:189
#     `.stderr(Stdio::inherit())`), so a leaked child could stall the gate forever. ON POSIX the
#     escalation below terminates the wait unconditionally. On MSYS/Git-Bash the intermediate
#     signals rely on emulated process groups, which do not reliably reach native cargo.exe /
#     rustc.exe descendants - so the guarantee there rests on the final `kill -9` of the direct
#     child, which is an MSYS process. A lane that never finished is INCONCLUSIVE, not fail.
# Artifacts are named per-run to keep concurrent gauntlets sharing an absolute CARGO_TARGET_DIR
# from truncating one another's log. Uniqueness is second+pid+RANDOM, which is strong but not a
# proof: host Git Bash and WSL are different pid namespaces and can in principle collide.
VETLOG="${CARGO_TARGET_DIR:-target}/vet"
VET_TEST_TIMEOUT="${VET_TEST_TIMEOUT:-1800}"     # outer bound: cargo itself wedged
VET_LEAK_GRACE="${VET_LEAK_GRACE:-10}"           # inner bound: cargo DONE, pipe held by a leak
VET_LOG_KEEP_DAYS="${VET_LOG_KEEP_DAYS:-7}"      # retained evidence is reaped by age, not by
                                                 # overwrite - per-run names removed that reaper
# JSON string escape. Every interpolated value goes through this: a path can legitimately carry
# a quote or a backslash (git ls-files C-quotes such names), and one of those in an evidence
# msg emits a malformed verdict object - a gate that cannot be parsed has failed its contract.
esc(){ printf '%s' "$1" | tr -d '"\\\r\n'; }
# Failing-test roster from a cargo-test log. libtest emits the trailing `failures:` list
# unconditionally; the `---- <name> stdout ----` header does NOT appear when a failing test's
# capture buffer is empty, and it shares a pipe with cargo's stderr. Dump/backtrace lines fail
# the `^    [^ ]` guard, so only roster blocks contribute. Its own function so the self-test
# interrogates the parser this gate actually runs, not a transcription of it.
roster(){ awk '/^failures:$/{f=1;next} f&&/^    [^ ]/{s=$0;sub(/^    /,"",s);print s;next} f&&/^$/{next} {f=0}' \
  "$1" | sort -u | tr '\n' ' '; }
run_test(){ L="$1"; shift
  # Capture prerequisites, all three required. Any miss degrades to the uninstrumented lane.
  # The probe tests the SEMANTICS this lane depends on, not the name and not the vendor:
  # System32\timeout.exe answers to `timeout` but rejects `-k`, which would pin the lane at
  # inconclusive forever instead of degrading. uutils and toybox pass because they do support
  # it. Probed before the log is created, so a failed probe leaves nothing behind.
  timeout -k 1 1 true </dev/null >/dev/null 2>&1 || { NOTES="${NOTES}${L}-uncaptured;"
    F0=$FAIL; run "$L" "$@"; [ "$FAIL" = "$F0" ] && return 0; return 1; }
  RUN="$(date -u +%Y%m%dT%H%M%SZ)-$$-${RANDOM}"
  D=$(mkdir -p "$VETLOG" 2>/dev/null && cd "$VETLOG" 2>/dev/null && pwd) || D=""
  LOG="$D/$L-$RUN.log"; SF="$D/$L-$RUN.status"
  # Age out old evidence. Retention exists to survive the NEXT run, not forever; without this
  # a rework loop leaves one full cargo-test log per red vet with nothing ever reaping them.
  [ -z "$D" ] || find "$D" -maxdepth 1 \( -name "$L-*.log" -o -name "$L-*.status" \) \
    -mtime +"$VET_LOG_KEEP_DAYS" -delete 2>/dev/null
  [ -z "$D" ] || : > "$LOG" 2>/dev/null || D=""
  if [ -z "$D" ]; then NOTES="${NOTES}${L}-uncaptured;"; F0=$FAIL; run "$L" "$@"
    [ "$FAIL" = "$F0" ] && return 0; return 1; fi
  echo "== $L"
  # The bound wraps the WHOLE pipeline, not just the command: a leaked child can outlive
  # cargo while still holding the pipe, and bounding cargo alone would let timeout return
  # on cargo's normal exit while tee hung on with no waiter. timeout's child is a subshell
  # that forks both pipeline legs and cannot exec away, so no pipeline leg outlives the bound
  # (a native grandchild still may - see above; the gate stops waiting either way).
  # The command's own status travels out by file, so tee's status can never be mistaken
  # for a test verdict: ST is the lane's word, TS is the capture's health.
  rm -f "$SF"
  VET_SF="$SF" VET_LOG="$LOG" timeout -k 10 "$VET_TEST_TIMEOUT" \
    bash -c '{ "$@" 2>&1; echo $? >"$VET_SF"; } | tee "$VET_LOG"' _ "$@" &
  PID=$!
  # Secondary bound. $SF existing PROVES the command finished, so every second the gate waits
  # after that is pure leak - the outer bound must stay generous for a cold build, but it must
  # not be what a leak costs. Armed only once $SF appears; grace covers tee's drain.
  # It escalates rather than sending one signal and leaving: TERM reaches `timeout`, which
  # forwards to its group and arms -k 10. If that emulated group signal never reaches tee,
  # timeout waits on a bash waiting on tee - forever - and nothing else would be left to act.
  # The final KILL targets the direct child, an MSYS process, so the wait ends on Windows too.
  # stdout is closed on the watchdog deliberately. It outlives nothing it should, but killing it
  # mid-`sleep` orphans that sleep, and an orphan holding vet.sh's stdout makes every caller that
  # reads this gate through a pipe - /gauntlet, CI, any `$(...)` - wait on the sleep rather than
  # on the gate. Measured at +1s per green run before this redirect.
  ( while kill -0 "$PID" 2>/dev/null; do
      if [ -s "$SF" ]; then sleep "$VET_LEAK_GRACE"; kill "$PID" 2>/dev/null
        sleep 15; kill -9 "$PID" 2>/dev/null; break; fi
      sleep 1
    done ) >/dev/null 2>&1 &
  WD=$!
  wait "$PID"; TS=$?
  kill "$WD" 2>/dev/null; wait "$WD" 2>/dev/null
  ST=$(cat "$SF" 2>/dev/null); rm -f "$SF"
  # No status, or a status meaning the command could not be INVOKED (126 not executable,
  # 127 not found), is could-not-run - never a test failure. Route to this script's existing
  # inconclusive channel ("absent"). The log is per-run and is kept, not overwritten, so the
  # evidence this branch cites still exists after the next vet invocation.
  if [ -z "$ST" ] || [ "$ST" = "126" ] || [ "$ST" = "127" ]; then
    NOTES="${NOTES}${L}-noverdict-absent;"
    W="did not report a status (TS=$TS)"
    [ "$TS" -eq 124 ] || [ "$TS" -eq 137 ] && W="exceeded ${VET_TEST_TIMEOUT}s and was killed"
    [ "$ST" = "126" ] && W="could not be executed (126)"; [ "$ST" = "127" ] && W="not found (127)"
    echo "-- $L $W - NO test verdict" >&2; echo "-- retained: $LOG" >&2
    EXTRA="$EXTRA,{\"kind\":\"$(esc "$L")\",\"msg\":\"no verdict: $(esc "$W"); log: $(esc "$LOG")\"}"
    return 1; fi
  [ "$TS" -eq 0 ] || NOTES="${NOTES}${L}-capture-degraded;"
  # Green lane: nothing to preserve, so the per-run log is not left behind as litter.
  [ "$ST" = "0" ] && { rm -f "$LOG"; return 0; }
  FAIL=1; NOTES="$NOTES$L;"
  KEPT="$LOG"
  NAMES=$(roster "$LOG")
  [ -n "$NAMES" ] || NAMES="(no roster in log - read it)"
  echo "-- $L failed: $NAMES" >&2; echo "-- retained: $KEPT" >&2
  EXTRA="$EXTRA,{\"kind\":\"$(esc "$L")\",\"msg\":\"failing: $(esc "$NAMES")| log: $(esc "$KEPT")\"}"
  # Proptest seeds from the OS RNG each run, so a rare-input property fails once and never
  # reproduces; its only reproduction handle is a crate-local .proptest-regressions. Naming
  # it is free. RESCUING it is not vet.sh's job: this script cannot tell a disposable lane
  # worktree from the main checkout, and the copy must outlive teardown. That is teardown's.
  # Claim stays inside the evidence: this is a dirty-file check, not a wrote-it-this-run proof.
  SEEDS=$(git ls-files --others --exclude-standard --modified -- '*.proptest-regressions' 2>/dev/null | tr '\n' ' ')
  if [ -n "$SEEDS" ]; then echo "-- dirty proptest seed(s) present - COPY OUT before any worktree teardown: $SEEDS" >&2
    EXTRA="$EXTRA,{\"kind\":\"proptest\",\"msg\":\"dirty seed file(s) present, may predate this run: $(esc "$SEEDS")\"}"; fi
  return 1; }
# Interrogability seam. `VET_LIB_ONLY=1 . vet.sh` loads the helpers and stops, so the self-test
# (.claude/hooks/vet-selftest.sh) exercises THE PARSER THIS GATE USES rather than a copy of it.
# Unset in every normal invocation, where the && short-circuits and `return` is never reached.
[ -n "$VET_LIB_ONLY" ] && return 0
[ -f Cargo.toml ] || { echo '{"verdict":"inconclusive","evidence":[{"kind":"env","msg":"no Cargo.toml at repo root - run from workspace root"}]}'; exit 0; }
# Fail-fast static trap check: only catches failures the later stages would hit anyway, minutes earlier.
if [ -x .claude/hooks/preflight.sh ] || [ -f .claude/hooks/preflight.sh ]; then
  bash .claude/hooks/preflight.sh || { echo '{"verdict":"fail","evidence":[{"kind":"gauntlet","msg":"preflight;"}]}'; exit 2; }
fi
run "fmt"    cargo fmt --all -- --check
run "clippy" cargo clippy --workspace --all-targets -- -D warnings
if [ $FAST -eq 1 ]; then
  if [ $FAIL -eq 0 ]; then echo '{"verdict":"inconclusive","evidence":[{"kind":"gap","msg":"fast-mode: tests+fixtures not run"}]}';
  else echo "{\"verdict\":\"fail\",\"evidence\":[{\"kind\":\"gauntlet\",\"msg\":\"$(esc "$NOTES")\"}]}"; exit 2; fi
  exit 0
fi
run_test "test" cargo test --workspace --quiet
if [ -x scripts/replay-fixtures.sh ]; then run "fixtures" bash scripts/replay-fixtures.sh; else NOTES="${NOTES}fixtures-absent;"; fi
# A pass may carry evidence. A run that stalled to a bound, killed a pipeline, or lost its
# capture is still a pass when the tests said so - blocking a genuine green on a broken log
# would be coercion in the other direction - but the note must not depend on some unrelated
# lane failing to be seen. Byte-identical `evidence:[]` when there is genuinely nothing to say.
if [ $FAIL -eq 0 ] && [ "${NOTES#*absent}" = "$NOTES" ]; then
  if [ -n "$NOTES" ] || [ -n "$EXTRA" ]; then echo "{\"verdict\":\"pass\",\"evidence\":[{\"kind\":\"note\",\"msg\":\"$(esc "$NOTES")\"}$EXTRA]}";
  else echo '{"verdict":"pass","evidence":[]}'; fi
elif [ $FAIL -eq 0 ]; then echo "{\"verdict\":\"inconclusive\",\"evidence\":[{\"kind\":\"gap\",\"msg\":\"$(esc "$NOTES")\"}$EXTRA]}";
else echo "{\"verdict\":\"fail\",\"evidence\":[{\"kind\":\"gauntlet\",\"msg\":\"$(esc "$NOTES")\"}$EXTRA]}"; exit 2; fi
