//! Section "Measuring the 25% Saving" (E4): heap use of real crate rings, via dhat.
//!
//! cargo run --release --example 06_memory_v1_v2
//!
//! Each configuration runs in a fresh child process so dhat's peak counter
//! starts from zero every time.
use std::mem::size_of;
use std::net::SocketAddr;
use std::process::Command;

use clap::Parser;
use pingora_ketama::{Bucket, Continuum, Version};
use ring_lab::model::{self, PointSource, Ring};
use ring_lab::{fleet, CommonArgs, Run};
use serde::{Deserialize, Serialize};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Internal: measure one configuration "<n>,<weight>,<v1|v2>,<point_multiple>"
    #[arg(long)]
    child: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Row {
    n: usize,
    weight: u32,
    version: String,
    point_multiple: u32,
    points: usize,
    final_bytes: usize,
    peak_bytes: usize,
    addr_bytes: usize,
    bytes_per_point: f64,
    peak_bytes_per_point: f64,
}

fn measure(spec: &str, seed: u64) -> Row {
    let p: Vec<&str> = spec.split(',').collect();
    let (n, weight, pm): (usize, u32, u32) = (p[0].parse().unwrap(), p[1].parse().unwrap(), p[3].parse().unwrap());
    let version = if p[2] == "v1" { Version::V1 } else { Version::V2 { point_multiple: pm } };
    let addrs = fleet(n, seed, 0);
    let buckets: Vec<Bucket> = addrs.iter().map(|&a| Bucket::new(a, weight)).collect();

    // 1. Start dhat, note the baseline, build the ring, read heap stats.
    let _prof = dhat::Profiler::builder().testing().build();
    let before = dhat::HeapStats::get();
    let ring = Continuum::new_with_version(&buckets, version);
    let after = dhat::HeapStats::get();
    let final_bytes = after.curr_bytes - before.curr_bytes;
    let peak_bytes = after.max_bytes - before.curr_bytes;
    drop(ring);

    // 2. Count surviving points with the model (after measuring).
    let mv = if p[2] == "v1" { model::Version::V1 } else { model::Version::V2 { point_multiple: pm } };
    let b: Vec<_> = addrs.iter().map(|&a| (a, weight)).collect();
    let points = Ring::build(&b, mv, PointSource::CrcChain).len();

    // 3. Subtract the address array (one SocketAddr per server) to get bytes per point.
    let addr_bytes = n * size_of::<SocketAddr>();
    Row {
        n, weight, version: p[2].to_uppercase(), point_multiple: pm, points, final_bytes, peak_bytes, addr_bytes,
        bytes_per_point: (final_bytes - addr_bytes) as f64 / points as f64,
        peak_bytes_per_point: (peak_bytes - addr_bytes) as f64 / points as f64,
    }
}

fn main() {
    let args = Args::parse();
    if let Some(spec) = &args.child {
        println!("{}", serde_json::to_string(&measure(spec, args.common.seed)).unwrap());
        return;
    }
    let mut run = Run::start(&args.common, "e4_memory", serde_json::json!({})).unwrap();
    println!("{:>5} {:>4} {:<7} | {:>10} | {:>9} {:>9} | {:>6} {:>6}", "N", "w", "version", "points", "final MB", "peak MB", "B/pt", "peak/pt");
    for n in [253, 2048] {
        for weight in [1, 10, 100] {
            for (v, pm) in [("v1", 160), ("v2", 160), ("v2", 16)] {
                // Re-run ourselves in child mode for a clean dhat profile.
                let out = Command::new(std::env::current_exe().unwrap())
                    .args(["--seed", &args.common.seed.to_string(), "--child", &format!("{n},{weight},{v},{pm}")])
                    .output()
                    .unwrap();
                let row: Row = serde_json::from_slice(&out.stdout).expect("child output");
                println!(
                    "{n:>5} {weight:>4} {:<7} | {:>10} | {:>9.2} {:>9.2} | {:>6.2} {:>6.2}",
                    format!("{}({pm})", row.version), row.points,
                    row.final_bytes as f64 / 1e6, row.peak_bytes as f64 / 1e6,
                    row.bytes_per_point, row.peak_bytes_per_point
                );
                run.row(&row);
            }
        }
    }
    run.finish();
}
