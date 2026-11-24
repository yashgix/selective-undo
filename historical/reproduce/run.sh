#!/bin/bash
# Reproduces the undo correctness benchmark (678 of 1,300 histories break today, 1 with the change); it does not measure latency.
# Usage: bash historical/reproduce/run.sh <path to a Tilo checkout at 2fcb9946fe with the three patches applied>
# Needs a Rust toolchain that builds the workspace (the run used 1.97.1) and the crates already fetched.
# Prints the property-P1 counts and fails if a cargo run fails or a count differs from the expected one.
set -euo pipefail
cd "${1:?path to a checkout of submit/selective-undo}"
out=$(mktemp)
trap 'rm -f "$out"' EXIT

run() { # name, cargo arguments: runs the measurement and keeps its whole output in $out
  echo "== $1"; shift
  cargo test --release "$@" -- --ignored --nocapture > "$out" 2>&1 || { tail -n 40 "$out"; echo "FAILED: cargo test exited non-zero" >&2; exit 1; }
}
expect() { # description, regex: the line must be in the output
  local got; got=$(grep -cE "$2" "$out" || true)
  [ "$got" -ge 1 ] || { echo "FAILED: no $1 line in the output" >&2; exit 1; }
  grep -oE "$2" "$out" | head -n 2 | sed 's/^/   /'
}

run "Today's Store::undo (2,000 seeded histories)" -p tilo-resource-store --test selective_undo
expect "baseline" '"histories":2000,"independent":1300,"dependent":700,"replay_errors":0,"p1_violations":678'
run "With the revert, distinct actors and a single actor" -p collaboration --test selective_revert
expect "distinct-actors" '"distinct_actors":\{"histories":2000,"independent":1300,"p1_violations":1,"replay_errors":0'
expect "single-actor" '"single_actor":\{"histories":2000,"independent":1300,"p1_violations":1,"replay_errors":0'
echo "OK: 678 of 1,300 independent histories break today, 1 with the revert (the list-order case)."
