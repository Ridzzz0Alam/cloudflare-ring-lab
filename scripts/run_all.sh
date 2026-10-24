#!/usr/bin/env bash
# Reproduce every experiment from a clean build.
#   ./scripts/run_all.sh            (≈ 25 min on a 4-core laptop, needs ~5 GB free RAM)
# CSV + .meta.json per run go to results/; program stdout goes to results/stdout/<name>.txt;
# charts go to figures/ (needs the Python deps in requirements.txt).
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=results/stdout
mkdir -p "$OUT"

{
  echo "date:      $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "commit:    $(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
  echo "rustc:     $(rustc --version)"
  echo "cpu:       $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs) ($(nproc) threads)"
  echo "os:        $(. /etc/os-release && echo "$PRETTY_NAME"), $(uname -sr)"
} | tee "$OUT/run_info.txt"

cargo clean
cargo build --release --examples --tests --benches

cargo test --release --test model_matches_crate -- --nocapture --test-threads=1 2>&1 \
  | grep -vE '^\s*(Running|Finished|Compiling)' | tee "$OUT/00_test.txt"

for ex in 00_fleet_and_keys 01_first_ring 02_one_point_unfair 03_virtual_nodes 04_weights \
          05_struct_layout 06_memory_v1_v2 07_collisions 08_fewer_points 09_churn_migration \
          10_edge_cases; do
  echo "=== $ex"
  cargo run -q --release --example "$ex" | tee "$OUT/$ex.txt"
done
cargo run -q --release --example 01_first_ring -- --points 4 | tee "$OUT/01_first_ring_points4.txt"

cargo bench -q --bench build_lookup 2>&1 | grep -E '^(build|lookup)/|time:|wrote' | tee "$OUT/bench.txt"
RAYON_NUM_THREADS=1 cargo bench -q --bench build_lookup -- build 2>&1 \
  | grep -E '^build/|time:|wrote' | tee "$OUT/bench_rayon1.txt"

if [ -x .venv/bin/python ]; then .venv/bin/python scripts/plot.py; fi
