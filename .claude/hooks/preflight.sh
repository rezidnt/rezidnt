#!/usr/bin/env bash
# Fast static trap check (<1s, no cargo). Catches known host-vet failure classes
# before the expensive stages, so a rework loop fails in seconds, not minutes.
# Each check mirrors a documented prior failure; only deterministic checks fail.
set -o pipefail
FAIL=0; EV=""
trap_hit(){ FAIL=1; EV="${EV}${EV:+,}{\"kind\":\"preflight\",\"msg\":\"$1\"}"; echo "preflight: $1" >&2; }
note(){ echo "preflight (note): $1" >&2; }

# 1. Windows elevation trap: a test/bin source named *update* builds a binary
#    Windows refuses to spawn unelevated (os error 740).
HITS=$(git ls-files '*.rs' | grep -iE '(^|/)[^/]*update[^/]*\.rs$' | grep -E '(^|/)(tests|bins)/' || true)
[ -n "$HITS" ] && trap_hit "rs file named *update* under tests/bins triggers os error 740 on host: $(echo "$HITS" | tr '\n' ' ')"

# 2. CRLF in golden/fixture files: host render comparison fails byte-identity.
#    .gitattributes pins the known extensions; this catches new unpinned patterns.
CRLF=$(git ls-files --eol -- '*golden*' '*.expected.json' '*.jsonl' 2>/dev/null | grep 'w/crlf' || true)
[ -n "$CRLF" ] && trap_hit "golden/fixture file has CRLF in working tree (add its extension to .gitattributes eol=lf): $(echo "$CRLF" | awk '{print $NF}' | tr '\n' ' ')"

# 3. clippy doc_lazy_continuation heuristic: doc-comment continuation starting
#    with '+' is almost always wrapped prose, not an intended bullet. Note-only.
PLUS=$(grep -rnE '^\s*//[/!] \+ ' --include='*.rs' crates/ bins/ 2>/dev/null || true)
[ -n "$PLUS" ] && note "doc line starts with '+ ' — likely doc_lazy_continuation bait: $(echo "$PLUS" | head -3 | cut -d: -f1-2 | tr '\n' ' ')"

if [ $FAIL -ne 0 ]; then echo "{\"verdict\":\"fail\",\"evidence\":[$EV]}"; exit 2; fi
echo '{"verdict":"pass","evidence":[]}'
