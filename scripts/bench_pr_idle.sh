#!/usr/bin/env bash
# PR gate for idle CPU and memory: measure head and base daemons on this runner, interleaved,
# then compare medians beyond noise with compare_bench.py.
#
# Usage: bench_pr_idle.sh <base-worktree>   (run from the head checkout; head binaries are
#        /tmp/tkmd-default and /tmp/tkmd-web; head sample 1 is idle-default.json / idle-web.json)
set -eu
base_src="$1"
head_src="$PWD"
base_target="$head_src/target-base"

(cd "$base_src" && CARGO_TARGET_DIR="$base_target" cargo build --release --locked -p tkmd)
cp "$base_target/release/tkmd" /tmp/tkmd-default-base
(cd "$base_src" && CARGO_TARGET_DIR="$base_target" cargo build --release --locked -p tkmd --features web)
cp "$base_target/release/tkmd" /tmp/tkmd-web-base

scratch="$(mktemp -d)"
daemon=""
stop_daemon() {
  if [ -n "$daemon" ]; then kill "$daemon" 2>/dev/null || true; wait "$daemon" 2>/dev/null || true; daemon=""; fi
}
trap 'stop_daemon; rm -rf "$scratch"' EXIT

measure() { # <binary> <variant> <output json>
  export HOME="$scratch/home" XDG_DATA_HOME="$scratch/data" XDG_CONFIG_HOME="$scratch/config" XDG_RUNTIME_DIR="$scratch/run"
  rm -rf "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$scratch/samples"
  mkdir -p "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR"
  if [ "$2" = web ]; then
    TKM_DECIDE_ENABLED=false TKM_UI_WEB_ENABLED=true TKM_UI_WEB_BIND=127.0.0.1:0 "$1" &
  else
    TKM_DECIDE_ENABLED=false "$1" &
  fi
  daemon=$!
  for _ in $(seq 1 100); do
    [ -S "$XDG_RUNTIME_DIR/tokenimum/tkmd.sock" ] && break
    sleep 0.1
  done
  test -S "$XDG_RUNTIME_DIR/tokenimum/tkmd.sock"
  sleep 5
  for _ in 1 2 3; do
    awk '/^VmRSS:/ {print $2}' "/proc/$daemon/status" >> "$scratch/samples"
    sleep 1
  done
  # Absolute budgets are enforced on the head run in the main step; base is only a reference.
  python3 "$head_src/scripts/idle_metrics.py" "$daemon" "$2" "$scratch/samples" "$3" || true
  stop_daemon
}

for variant in default web; do
  cp "idle-$variant.json" "idle-$variant-head-1.json"
done
for variant in default web; do
  measure "/tmp/tkmd-$variant-base" "$variant" "idle-$variant-base-1.json"
done
for i in 2 3; do
  for variant in default web; do
    if [ $((i % 2)) -eq 0 ]; then
      measure "/tmp/tkmd-$variant" "$variant" "idle-$variant-head-$i.json"
      measure "/tmp/tkmd-$variant-base" "$variant" "idle-$variant-base-$i.json"
    else
      measure "/tmp/tkmd-$variant-base" "$variant" "idle-$variant-base-$i.json"
      measure "/tmp/tkmd-$variant" "$variant" "idle-$variant-head-$i.json"
    fi
  done
done

status=0
for variant in default web; do
  python3 scripts/compare_bench.py --threshold 1.20 --summary "${GITHUB_STEP_SUMMARY:-/dev/null}" \
    --head "idle-$variant-head-1.json" "idle-$variant-head-2.json" "idle-$variant-head-3.json" \
    --base "idle-$variant-base-1.json" "idle-$variant-base-2.json" "idle-$variant-base-3.json" || status=1
done
exit $status
