#!/usr/bin/env bash
# Golden-fixture replay — run by the /vet gauntlet and by every release
# (testing-oracles skill). Folds each committed fixture and asserts the
# expected graph / chain verdict. Exits nonzero on any divergence.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "replay: folding spec/fixtures/*.jsonl against *.expected.json (rezidnt-state)"
cargo test -q -p rezidnt-state --test fixture_replay

echo "replay: verifying golden chain fixtures (rezidnt-fabric)"
cargo test -q -p rezidnt-fabric --test chain_fixtures

echo "replay: all golden fixtures green"
