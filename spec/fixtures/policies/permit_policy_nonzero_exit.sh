#!/bin/sh
# Reference NON-CONFORMING permit policy (SP3, DR-015 verdict-map / crit 4).
#
# The never-coerce trap: this policy PRINTS a well-formed `pass` document but
# EXITS NONZERO. Under the §8 contract a nonzero exit is `inconclusive
# {nonzero_exit}` — NEVER a delivered verdict, even one that says pass (I6). The
# permit axis must therefore map this to ESCALATE / `ask`, never allow.
printf '{"verdict":"pass","evidence":[],"cost_ms":1}\n'
exit 7
