//! Section "Edge Cases I Found": u32 overflow, u16 index wrap, collision tie-breaks.
//!
//! cargo run --release --example 10_edge_cases
use std::net::SocketAddr;
use std::panic;

use clap::Parser;
use pingora_ketama::{Bucket, Continuum, Version};
use ring_lab::model::{self, crc_chain, forge_key, PointSource, Ring};
use ring_lab::{fleet, random_keys, CommonArgs, Run};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(Serialize)]
struct Row {
    case: &'static str,
    metric: &'static str,
    value: f64,
}

/// Fraction of `keys` that the ring sends to `target`.
fn share_of(c: &Continuum, target: SocketAddr, keys: &[[u8; 16]]) -> f64 {
    keys.iter().filter(|k| c.node(&k[..]) == Some(target)).count() as f64 / keys.len() as f64
}

fn main() {
    let args = Args::parse();
    let seed = args.common.seed;
    let mut run = Run::start(&args.common, "e8_edge_cases", serde_json::json!({})).unwrap();
    let keys = random_keys(200_000, seed, 40);
    let [a, b]: [SocketAddr; 2] = fleet(2, seed, 41).try_into().unwrap();

    // 1. u32 overflow: points = weight × point_multiple is computed in u32.
    //    Weight 2^16 with 2^16 points per unit is 2^32 points, which wraps to 0 in release.
    let c = Continuum::new_with_version(
        &[Bucket::new(a, 1), Bucket::new(b, 1 << 16)],
        Version::V2 { point_multiple: 1 << 16 },
    );
    let got = share_of(&c, b, &keys);
    println!("1. weight 65536 × 65536 points wraps to 0: heavy server gets {:.1}% of keys (expected ~100%)", got * 100.0);
    run.row(&Row { case: "u32 overflow (V2 2^16 x 2^16)", metric: "heavy_share", value: got });

    //    With V1 (160 per unit) the wrap starts at weight 26,843,546 (≈ 2^32 / 160).
    let w = 26_843_546;
    let c = Continuum::new(&[Bucket::new(a, 1), Bucket::new(b, w)]);
    let got = share_of(&c, b, &keys);
    println!("   V1, weight {w}: {}×160 wraps to {} points, heavy server gets {:.1}%", w, (w as u32).wrapping_mul(160), got * 100.0);
    run.row(&Row { case: "u32 overflow (V1 weight 26843546)", metric: "heavy_share", value: got });

    // 2. u16 server index: with more than 65,536 servers, server 65,536 is stored as index 0.
    let big = fleet(65_537, seed, 42);
    let buckets: Vec<Bucket> = big.iter().map(|&x| Bucket::new(x, 1)).collect();
    for (name, v) in [("V1", Version::V1), ("V2", Version::V2 { point_multiple: 1 })] {
        let c = Continuum::new_with_version(&buckets, v);
        // Aim a key exactly at server 65,536's first point.
        let key = forge_key(crc_chain(&big[65_536]).next().unwrap());
        let got = c.node(&key).unwrap();
        println!("2. {name}: key aimed at server #65536 ({}) is routed to {} (server #0 = {})", big[65_536], got, big[0]);
        run.row(&Row { case: "u16 index wrap", metric: if name == "V1" { "v1_routed_to_server0" } else { "v2_routed_to_server0" }, value: f64::from(u8::from(got == big[0])) });
    }

    // 3. Collision winners: V1 keeps whichever duplicate std's unstable sort leaves first;
    //    V2 keeps the lower address. Measure how much key space the two disagree on.
    let fleet2k: Vec<_> = fleet(2048, seed, 43).into_iter().map(|x| (x, 10)).collect();
    let r1 = Ring::build(&fleet2k, model::Version::V1, PointSource::CrcChain);
    let r2 = Ring::build(&fleet2k, model::Version::V2 { point_multiple: 160 }, PointSource::CrcChain);
    let (mut differ, mut space) = (0, 0u64);
    for i in 0..r1.len() {
        if r1.owner(i) != r2.owner(i) {
            differ += 1;
            // Point i owns (hash[i-1], hash[i]]; point 0 also owns the wrap-around.
            let prev = if i == 0 { r1.hash(r1.len() - 1) as u64 } else { r1.hash(i - 1) as u64 };
            space += (r1.hash(i) as u64 + (1 << 32) - prev) % (1 << 32);
        }
    }
    println!(
        "3. N=2048×w10: {} collided points; V1 and V2(160) pick different winners for {differ}, \
         i.e. {:.4}% of keys route differently",
        r1.generated - r1.len(), space as f64 / 2f64.powi(32) * 100.0
    );
    run.row(&Row { case: "V1 vs V2(160) collision winners", metric: "collided_points", value: (r1.generated - r1.len()) as f64 });
    run.row(&Row { case: "V1 vs V2(160) collision winners", metric: "differing_winners", value: differ as f64 });
    run.row(&Row { case: "V1 vs V2(160) collision winners", metric: "keyspace_frac", value: space as f64 / 2f64.powi(32) });

    // 4. node_idx() is a point index, not a server index.
    let c = Continuum::new(&fleet(3, seed, 44).into_iter().map(|x| Bucket::new(x, 1)).collect::<Vec<_>>());
    println!("4. 3 servers, node_idx(\"/a\") = {} (an index into 480 points, not into 3 servers)", c.node_idx(b"/a"));
    run.row(&Row { case: "node_idx is point index", metric: "idx", value: c.node_idx(b"/a") as f64 });

    // 5. Empty rings return None; weight 0 panics.
    let empty = Continuum::new(&[]);
    panic::set_hook(Box::new(|_| {}));
    let zero = panic::catch_unwind(|| Bucket::new(a, 0)).is_err();
    let _ = panic::take_hook();
    println!("5. empty ring → node() = {:?}; Bucket::new(addr, 0) panics: {zero}", empty.node(b"/a"));
    run.row(&Row { case: "weight 0 panics", metric: "panics", value: f64::from(u8::from(zero)) });
    run.finish();
}
