#!/bin/sh
# Reference ROLE-KEYED permit policy program (SP4a, DR-016 §Decision 2 /
# design permit-roles-delegation-sp4 §4).
#
# THE DETERMINISTIC JUDGE FOR SP4a's HEADLINE, NOT A VENDORED RBAC ENGINE (I7).
# This is the tiny local argv the oracle uses to prove "a role-keyed policy
# decides a permit DIFFERENTLY by role" WITHOUT bundling an RBAC/policy engine.
# It reads the §8 `VerifierInput` document on stdin and emits a §8
# `VerifierOutput` document on stdout, speaking the exact contract
# `ExecVerifier` dispatches (crates/rezidnt-gate/src/lib.rs — stdin
# VerifierInput → stdout VerifierOutput; nonzero-exit/malformed/timeout →
# inconclusive; scrubbed env + network-off).
#
# Policy (deterministic, replayable — same stdin → same verdict, I6):
#   The action under test is a WRITE (the request forces `tool = "Edit"`).
#   The decision keys ONLY on the injected `params.role` axis (SP4a):
#     - role is `reviewer`      → DENY  the write   (emit verdict "fail").
#     - role is `contributor`   → ALLOW the write   (emit verdict "pass").
#     - NO role declared (absent `params.role`)     → ESCALATE, never a
#       synthesized allow (emit verdict "inconclusive"). Absence is honest
#       (DR-012 declared-vs-absent) — a role-less agent is NOT a contributor.
#
# This is the load-bearing SP4a proof: the SAME policy + SAME `Edit` request
# yields a DIFFERENT decision purely because `params.role` differs — role
# actually changed the outcome end-to-end through the live PDP.
#
# POSIX sh + grep only. No network, no ambient env (the runner clears it), no
# non-determinism. `grep`/`case` match the compact-JSON `"role":"reviewer"` the
# §8 serializer emits (serde_json compact, no spaces) — kept literal so the
# judge is auditable at a glance. If `role` is injected as a bare param key
# (`params.role`), the compact document contains `"role":"<value>"`.

stdin=$(cat)

case "$stdin" in
  *'"role":"reviewer"'*)
    # The role-keyed policy DENIES a reviewer's write. Evidence names the role
    # so the deny stays interrogable (I6); no CAS ref (the exec contract lets
    # the engine record its own stdout evidence verbatim).
    printf '{"verdict":"fail","evidence":[{"kind":"policy","msg":"reference role policy denied write for role reviewer"}],"cost_ms":3}\n'
    ;;
  *'"role":"contributor"'*)
    # A contributor MAY write.
    printf '{"verdict":"pass","evidence":[{"kind":"policy","msg":"reference role policy allowed write for role contributor"}],"cost_ms":2}\n'
    ;;
  *)
    # No role reached the policy — undecidable, escalate. NEVER coerced to a
    # pass (I6): a role-less run is not silently treated as a contributor.
    printf '{"verdict":"inconclusive","evidence":[{"kind":"policy","msg":"reference role policy saw no role axis"}],"cost_ms":2}\n'
    ;;
esac
