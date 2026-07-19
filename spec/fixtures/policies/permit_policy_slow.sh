#!/bin/sh
# Reference NON-CONFORMING permit policy (SP3, DR-015 verdict-map / crit 4).
#
# The never-coerce trap: this policy OVERRUNS the wall-clock `timeout_ms`. Under
# the §8 contract a timeout is `inconclusive {timeout}`, killed in bounded time
# and never a delivered verdict (I6). The permit axis must map it to ESCALATE /
# `ask`, never allow. (It would print `pass` if it ever finished; it must not
# get the chance.)
sleep 30
printf '{"verdict":"pass","evidence":[],"cost_ms":1}\n'
