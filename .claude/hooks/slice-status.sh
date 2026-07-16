#!/usr/bin/env bash
# SessionStart context: current slice + open gates. Keep short — this lands in the context window.
S=$(cat .claude/state/current-slice 2>/dev/null || echo "S0")
echo "rezidnt harness: current slice = $S (run /slice for acceptance criteria)."
echo "Done means: slice criteria pass /vet and /debrief. Ontology via /subject only. Competitor material via /intel only."
exit 0
