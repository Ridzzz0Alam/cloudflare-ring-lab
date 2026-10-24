//! Section "Building Our First Ring": three servers, a few keys.
//!
//! cargo run --release --example 01_first_ring
use std::net::SocketAddr;

use clap::Parser;
use pingora_ketama::{Bucket, Continuum, Version};
use ring_lab::model::{self, PointSource, Ring};
use ring_lab::{CommonArgs, Run};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Points per server
    #[arg(long, default_value_t = 1)]
    points: u32,
}

#[derive(Serialize)]
struct Row {
    kind: &'static str,
    label: String,
    hash: u32,
    ring_pct: f64,
    server: &'static str,
}

const NAMES: [&str; 3] = ["A", "B", "C"];

fn main() {
    let args = Args::parse();
    let mut run = Run::start(&args.common, "e0_first_ring", serde_json::json!({"points": args.points})).unwrap();
    let pct = |h: u32| h as f64 / ring_lab::stats::RING * 100.0;

    // 1. Three servers, each with weight 1. V2 lets us choose the points per server.
    let addrs: Vec<SocketAddr> = ["10.0.0.1:443", "10.0.0.2:443", "10.0.0.3:443"]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    let buckets: Vec<Bucket> = addrs.iter().map(|&a| Bucket::new(a, 1)).collect();
    let ring = Continuum::new_with_version(&buckets, Version::V2 { point_multiple: args.points });
    let name_of = |a: SocketAddr| NAMES[addrs.iter().position(|&x| x == a).unwrap()];

    // 2. The crate keeps its points private, so peek at them with our reference model.
    let peek = Ring::equal(&addrs, model::Version::V2 { point_multiple: args.points }, PointSource::CrcChain);
    println!("Ring points (sorted by position on the 0..2^32 circle):");
    for (hash, owner) in peek.points() {
        println!("  server {}  hash {:>10}  at {:>5.1}% of the ring", NAMES[owner], hash, pct(hash));
        run.row(&Row { kind: "point", label: addrs[owner].to_string(), hash, ring_pct: pct(hash), server: NAMES[owner] });
    }

    // 3. Route some keys: hash with CRC32, walk clockwise to the next point.
    println!("\nKeys:");
    for key in ["/index.html", "/logo.png", "/app.js", "/style.css", "/video.mp4", "/api/users"] {
        let hash = crc32fast::hash(key.as_bytes());
        let server = name_of(ring.node(key.as_bytes()).unwrap());
        println!("  {key:<12} hash {hash:>10}  at {:>5.1}%  → server {server}", pct(hash));
        run.row(&Row { kind: "key", label: key.into(), hash, ring_pct: pct(hash), server });
    }
    run.finish();
}
