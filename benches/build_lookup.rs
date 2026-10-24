//! E7: build and lookup speed, V1 vs V2.
//!
//! cargo bench --bench build_lookup
//! RAYON_NUM_THREADS=1 cargo bench --bench build_lookup   (single-threaded V2 sort)
//!
//! After criterion finishes, medians and 95% CIs are copied into results/e7_bench_*.csv.
use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion};
use pingora_ketama::{Bucket, Continuum, Version};
use ring_lab::{fleet, random_keys, CommonArgs, Run};
use serde::Serialize;

const VERSIONS: [(&str, Version); 3] = [
    ("V1(160)", Version::V1),
    ("V2(160)", Version::V2 { point_multiple: 160 }),
    ("V2(16)", Version::V2 { point_multiple: 16 }),
];
// (servers, weight): small, the upstream memory bench's fleet, and a big data centre.
const SIZES: [(usize, u32); 3] = [(100, 1), (253, 10), (2048, 10)];

fn buckets(n: usize, w: u32) -> Vec<Bucket> {
    fleet(n, 42, 0).into_iter().map(|a| Bucket::new(a, w)).collect()
}

fn bench_build(c: &mut Criterion) {
    let mut g = c.benchmark_group("build");
    g.sample_size(20).measurement_time(Duration::from_secs(5));
    for (n, w) in SIZES {
        let b = buckets(n, w);
        for (name, v) in VERSIONS {
            g.bench_with_input(BenchmarkId::new(name, format!("{n}x{w}")), &b, |bench, b| {
                bench.iter(|| Continuum::new_with_version(black_box(b), v))
            });
        }
    }
    g.finish();
}

fn bench_lookup(c: &mut Criterion) {
    let mut g = c.benchmark_group("lookup");
    let keys = random_keys(1024, 42, 1);
    for (n, w) in SIZES {
        let b = buckets(n, w);
        for (name, v) in VERSIONS {
            let ring = Continuum::new_with_version(&b, v);
            // Time 1024 lookups per iteration; divide by 1024 for per-lookup time.
            g.bench_function(BenchmarkId::new(name, format!("{n}x{w}")), |bench| {
                bench.iter(|| {
                    for k in &keys {
                        black_box(ring.node(black_box(&k[..])));
                    }
                })
            });
        }
    }
    g.finish();
}

#[derive(Serialize)]
struct Row {
    group: String,
    version: String,
    size: String,
    per: &'static str,
    median_ns: f64,
    ci_low_ns: f64,
    ci_high_ns: f64,
    rayon_threads: String,
}

/// Copy criterion's median estimates into our results/ format.
/// Only estimates written by this run (newer than `started`) are copied.
fn report(started: std::time::SystemTime) {
    let threads = std::env::var("RAYON_NUM_THREADS").unwrap_or_else(|_| "default".into());
    let args = CommonArgs { seed: 42, out_dir: "results".into() };
    let name = if threads == "default" { "e7_bench".to_string() } else { format!("e7_bench_rayon{threads}") };
    let mut run = Run::start(&args, &name, serde_json::json!({"rayon_threads": threads})).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/criterion");
    for group in ["build", "lookup"] {
        for (vname, _) in VERSIONS {
            for (n, w) in SIZES {
                let path = root.join(group).join(vname).join(format!("{n}x{w}")).join("new/estimates.json");
                let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).is_ok_and(|t| t >= started);
                let Ok(s) = std::fs::read_to_string(&path) else { continue };
                if !fresh {
                    continue;
                }
                let e: serde_json::Value = serde_json::from_str(&s).unwrap();
                let m = &e["median"];
                let scale = if group == "lookup" { 1024.0 } else { 1.0 };
                run.row(&Row {
                    group: group.into(), version: vname.into(), size: format!("{n}x{w}"),
                    per: if group == "lookup" { "lookup" } else { "build" },
                    median_ns: m["point_estimate"].as_f64().unwrap() / scale,
                    ci_low_ns: m["confidence_interval"]["lower_bound"].as_f64().unwrap() / scale,
                    ci_high_ns: m["confidence_interval"]["upper_bound"].as_f64().unwrap() / scale,
                    rayon_threads: threads.clone(),
                });
            }
        }
    }
    run.finish();
}

fn main() {
    let started = std::time::SystemTime::now();
    let mut c = Criterion::default().configure_from_args();
    bench_build(&mut c);
    bench_lookup(&mut c);
    c.final_summary();
    report(started);
}
