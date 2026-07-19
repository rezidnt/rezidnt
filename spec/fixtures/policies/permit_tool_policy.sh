#!/bin/sh
# Reference permit policy program (SP3, DR-015 §Decision 4 / design §5).
#
# THE DETERMINISTIC JUDGE, NOT A VENDORED ENGINE (I7). This is the tiny local
# argv the oracle uses to prove "an external policy file decides a permit"
# WITHOUT bundling OPA/Cedar. It reads the §8 `VerifierInput` document on stdin
# and emits a §8 `VerifierOutput` document on stdout, speaking the exact
# contract `ExecVerifier` dispatches (crates/rezidnt-gate/src/lib.rs — stdin
# VerifierInput → stdout VerifierOutput, nonzero-exit/malformed/timeout →
# inconclusive, scrubbed env + network-off).
#
# Policy (deterministic, replayable — same stdin → same verdict, I6):
#   - the requested `params.tool` is `Bash`  → DENY  (emit verdict "fail").
#   - anything else                          → ALLOW (emit verdict "pass").
# Bash is the stand-in "forced-breach" tool the headline test forces; the
# verdict is the EXTERNAL policy's, dispatched through the exec seam — no
# hardcoded native decided it.
#
# POSIX sh + grep only. No network, no ambient env (the runner clears it), no
# non-determinism. `grep` matches the compact-JSON `"tool":"Bash"` the §8
# serializer emits (serde_json compact, no spaces) — kept literal so the judge
# is auditable at a glance.

stdin=$(cat)

case "$stdin" in
  *'"tool":"Bash"'*)
    # The external policy DENIES the forced breach. Evidence names the tool so
    # the deny stays interrogable (I6); no CAS ref (the exec contract lets the
    # engine record its own stdout evidence verbatim).
    printf '{"verdict":"fail","evidence":[{"kind":"policy","msg":"reference policy denied tool Bash"}],"cost_ms":3}\n'
    ;;
  *)
    printf '{"verdict":"pass","evidence":[],"cost_ms":2}\n'
    ;;
esac
