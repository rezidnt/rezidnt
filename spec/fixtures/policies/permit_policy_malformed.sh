#!/bin/sh
# Reference NON-CONFORMING permit policy (SP3, DR-015 verdict-map / crit 4).
#
# The never-coerce trap: this policy emits UNPARSEABLE stdout (prose, not a §8
# VerifierOutput) and exits 0. Under the §8 contract unparseable stdout is
# `inconclusive {malformed_output}` — never coerced to pass (I6). The permit
# axis must map it to ESCALATE / `ask`, never allow.
printf 'LGTM, allow it\n'
