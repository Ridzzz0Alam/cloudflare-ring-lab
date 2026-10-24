# cloudflare-ring-lab

Experiments that reproduce and test the claims in Cloudflare's post
[Saving another 100TB of RAM with math (and Rust)](https://blog.cloudflare.com/saving-100-tb-of-ram-with-math/),
using their open-source consistent-hashing crate
[`pingora-ketama`](https://github.com/cloudflare/pingora/tree/main/pingora-ketama) with the `v2` feature.

`pingora-ketama` is pinned to commit `623e163dfab9685c4a49bab08ed78838dd366ba6` in `Cargo.toml`, so results don't drift with upstream.

## Requirements

- Rust stable (tested with 1.99.1)
- Optional: Python 3.10+ for the charts (`pip install -r requirements.txt`)
- About 5 GB of free RAM for the largest collision experiment (`07_collisions`)

## Quick start

```bash
# 1. Check the reference model routes every key exactly like the crate
cargo test --release --test model_matches_crate -- --nocapture --test-threads=1

# 2. Run any experiment
cargo run --release --example 03_virtual_nodes

# 3. Or run everything from a clean build (~25 min on a 4-core laptop)
./scripts/run_all.sh
```

Always build with `--release`: debug builds are much slower, and they panic on the u32 overflow that `10_edge_cases` demonstrates.

## Layout

| Path | What it is |
|---|---|
| `src/model.rs` | Reference re-implementation of the ring (point generation, sort, dedup, lookup). It exposes every point, so shares can be computed from exact arc lengths. |
| `src/lib.rs` | Seeded fleets and keys, a scoped-thread `par_map`, and a writer for `results/<name>_<timestamp>.csv` + `.meta.json` |
| `src/stats.rs` | CV and other helpers, theory formulas, birthday estimate |
| `tests/model_matches_crate.rs` | Model vs `Continuum::node()`: 0 mismatches across V1/V2 configs, including every collided hash (via forged keys) |
| `examples/` | One program per experiment (below) |
| `benches/build_lookup.rs` | Criterion benchmarks for build and lookup, V1 vs V2 |
| `scripts/run_all.sh` | Clean rebuild + every test, example and benchmark |
| `scripts/plot.py` | `results/*.csv` → `figures/*.png` |

## Experiments

| Example | Measures |
|---|---|
| `00_fleet_and_keys` | Sample fleets and keys; CRC32 spread of random vs URL keys |
| `01_first_ring` | A 3-server ring and key routing (`--points N` for more points per server) |
| `02_one_point_unfair` | Load imbalance with 1 point per server vs `√((N−1)/(N+1))` |
| `03_virtual_nodes` | CV vs points per server for N ∈ {10, 100, 1000}, CRC32 chain vs random points, plus a 10M-key cross-check through the crate |
| `04_weights` | Weighted servers: share/weight and error by weight class |
| `05_struct_layout` | `size_of` / `align_of` of `{u32,u32}`, `{u32,u16}`, packed, `[u8; 6]` |
| `06_memory_v1_v2` | dhat heap profile of real rings: final and peak bytes per point |
| `07_collisions` | 32-bit collisions at k up to 200,000; tie-break variants |
| `08_fewer_points` | Memory vs balance trade-off, weight-1 vs storage-style weights |
| `09_churn_migration` | Keys moved by add/remove, `hash % N`, and V1(160) → V2(16) |
| `10_edge_cases` | u32 overflow, u16 index wrap, collision winners, `node_idx` |

Every program takes `--seed` (default 42) and `--out-dir` (default `results`), and prints a short table.

## Main results (seed 42, Intel i7-7500U)

- The CV formula `√((N−1)/(N·k+1))` matches measurement for N up to 1000 and k up to 10,000 (7.97% vs 7.87% at N=100, k=160).
- V2 rings use 6.00 bytes per point vs V1's 8.00 (−25%). During the build, though, V2 peaks at ~12 B/point because the sort allocates a scratch buffer.
- Cutting from 160 to 16 points per weight unit keeps the CV around 1% with storage-sized weights (~625), but gives 25% with weight-1 servers. It moves 89.5% of keys (N=100).
- At very high k the error rises again: V2 resolves hash collisions in favour of the lowest `SocketAddr`, so load correlates with address order (r = −0.97 at N=2048, k=100k). A hashed tie-break follows theory.
- V2's build is faster than V1 only with multiple threads on large rings; single-threaded it is slower.
- `weight × point_multiple` is computed in `u32` and wraps silently in release builds. The server index is `u16` in both versions.
