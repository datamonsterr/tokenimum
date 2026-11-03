#!/usr/bin/env bash
# PR gate for hook startup, hook round trip and shim latency: measure head and base on this
# runner, interleaved, then compare medians beyond noise with compare_bench.py.
#
# Usage: bench_pr_hooks.sh <base-worktree>   (run from the head checkout, release binaries built;
#        head sample 1 is hook-benchmark.json, roundtrip-benchmark.json, shim-benchmark.json)
set -eu
base_src="$1"
head_src="$PWD"
head_bin="$head_src/target/release"
base_target="$head_src/target-base"
base_bin="$base_target/release"
fixture="$head_src/fixtures/claude/hooks/PostToolUse-bash.json"

(cd "$base_src" && CARGO_TARGET_DIR="$base_target" cargo build --release --locked -p tkmd -p tkm-hook)

scratch="$(mktemp -d)"
daemon=""
stop_daemon() {
  if [ -n "$daemon" ]; then kill "$daemon" 2>/dev/null || true; wait "$daemon" 2>/dev/null || true; daemon=""; fi
}
trap 'stop_daemon; rm -rf "$scratch"' EXIT

start_daemon() { # <bin dir>
  export HOME="$scratch/home" XDG_DATA_HOME="$scratch/data" XDG_CONFIG_HOME="$scratch/config" XDG_RUNTIME_DIR="$scratch/run"
  rm -rf "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
  mkdir -p "$HOME" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR"
  TKM_DECIDE_ENABLED=false "$1/tkmd" &
  daemon=$!
  for _ in $(seq 1 100); do
    [ -S "$XDG_RUNTIME_DIR/tokenimum/tkmd.sock" ] && break
    sleep 0.1
  done
  test -S "$XDG_RUNTIME_DIR/tokenimum/tkmd.sock"
}

sample() { # <label> <bin dir> <source dir> <round>
  label="$1"; bin="$2"; src="$3"; round="$4"
  hyperfine --warmup 20 --runs 200 --input null --export-json "hf-$label-$round.json" "$bin/tkm-hook"
  python3 "$head_src/scripts/hyperfine_to_benchmark.py" "hf-$label-$round.json" "hook-$label-$round.json"

  start_daemon "$bin"
  hyperfine --warmup 20 --runs 300 --shell=none --input "$fixture" --export-json "rt-$label-$round.json" \
    "$bin/tkm-hook --host claude --event tool_done"
  (cd "$head_src/scripts" && python3 roundtrip_to_benchmark.py "../rt-$label-$round.json" "../roundtrip-$label-$round.json")
  stop_daemon

  start_daemon "$bin"
  for host in opencode pi; do
    (cd "$src" && PATH="$bin:$PATH" hyperfine --warmup 20 --runs 100 --shell=none \
      --export-json "$head_src/shim-$label-$round-$host.json" "bun scripts/bench_shim.ts $host")
    # Absolute shim budgets are enforced on the head run in the main job; base may not meet them.
    python3 "$head_src/scripts/shim_benchmark.py" "$host" "shim-$label-$round-$host.json" "shim-$label-$round-$host-metrics.json" >/dev/null || true
  done
  stop_daemon
  python3 -c 'import json,sys; print(json.dumps(json.load(open(sys.argv[1])) + json.load(open(sys.argv[2]))))' \
    "shim-$label-$round-opencode-metrics.json" "shim-$label-$round-pi-metrics.json" > "shim-$label-$round.json"
}

cp hook-benchmark.json hook-head-1.json
cp roundtrip-benchmark.json roundtrip-head-1.json
cp shim-benchmark.json shim-head-1.json
sample base "$base_bin" "$base_src" 1
for i in 2 3 4; do
  if [ $((i % 2)) -eq 0 ]; then
    sample head "$head_bin" "$head_src" "$i"
    sample base "$base_bin" "$base_src" "$i"
  else
    sample base "$base_bin" "$base_src" "$i"
    sample head "$head_bin" "$head_src" "$i"
  fi
done

status=0
for kind in hook roundtrip shim; do
  python3 scripts/compare_bench.py --threshold 1.20 --summary "${GITHUB_STEP_SUMMARY:-/dev/null}" \
    --head "$kind-head-1.json" "$kind-head-2.json" "$kind-head-3.json" "$kind-head-4.json" \
    --base "$kind-base-1.json" "$kind-base-2.json" "$kind-base-3.json" "$kind-base-4.json" || status=1
done
exit $status
