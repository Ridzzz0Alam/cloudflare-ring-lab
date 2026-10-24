//! Section "Adding Virtual Nodes" (E1: CV vs points per server).
//!
//! cargo run --release --example 03_virtual_nodes
use std::collections::HashMap;

use clap::Parser;
use ring_lab::model::{crate_ring, PointSource, Ring, Version};
use ring_lab::stats::{self, balance, mean, rms, theory_cv};
use ring_lab::{fleet, par_map, random_keys, url_keys, CommonArgs, Run};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Random rings per (N, k, variant)
    #[arg(long, default_value_t = 30)]
    reps: u64,
    /// Keys sampled through the real crate for the cross-check
    #[arg(long, default_value_t = 10_000_000)]
    keys: usize,
    /// Which N to print (all go to the CSV)
    #[arg(long, default_value_t = 100)]
    show_n: usize,
}

#[derive(Serialize)]
struct Row {
    n: usize,
    k: u32,
    variant: &'static str,
    reps: u64,
    theory_cv: f64,
    cv_rms: f64,
    cv_min: f64,
    cv_max: f64,
    max_ratio_mean: f64,
    max_ratio_worst: f64,
    min_ratio_mean: f64,
}

#[derive(Serialize)]
struct Check {
    n: usize,
    k: u32,
    key_type: &'static str,
    keys: usize,
    exact_cv: f64,
    counted_cv: f64,
    sampling_cv: f64,
    corrected_cv: f64,
}

const KS: [u32; 12] = [1, 2, 5, 10, 16, 50, 100, 160, 500, 1000, 1600, 10_000];

fn main() {
    let args = Args::parse();
    let seed = args.common.seed;
    let params = serde_json::json!({"reps": args.reps, "ks": KS, "ns": [10, 100, 1000], "keys": args.keys});
    let mut run = Run::start(&args.common, "e1_balance", params).unwrap();
    let reps: Vec<u64> = (0..args.reps).collect();

    println!("N={}: CV of server load vs points per server ({} rings each)", args.show_n, args.reps);
    println!("{:>6} | {:>7} | {:>7} {:>7} | {:>14}", "k", "theory", "CRC32", "random", "busiest server");
    for n in [10, 100, 1000] {
        for k in KS {
            let mut rows = vec![];
            for (variant, crc) in [("crc", true), ("random", false)] {
                // Measure `reps` independent rings with k points per server.
                let b = par_map(&reps, |&rep| {
                    let source = if crc { PointSource::CrcChain } else { PointSource::Random { seed: seed + 1_000 * rep } };
                    let ring = Ring::equal(&fleet(n, seed, rep), Version::V2 { point_multiple: k }, source);
                    balance(&ring.exact_shares(), &ring.expected_shares())
                });
                let cvs: Vec<f64> = b.iter().map(|x| x.cv).collect();
                let maxes: Vec<f64> = b.iter().map(|x| x.max_ratio).collect();
                rows.push(Row {
                    n, k, variant, reps: args.reps,
                    theory_cv: theory_cv(n as f64, k as f64),
                    cv_rms: rms(&cvs), cv_min: stats::min(&cvs), cv_max: stats::max(&cvs),
                    max_ratio_mean: mean(&maxes), max_ratio_worst: stats::max(&maxes),
                    min_ratio_mean: mean(&b.iter().map(|x| x.min_ratio).collect::<Vec<_>>()),
                });
            }
            if n == args.show_n {
                let (c, r) = (&rows[0], &rows[1]);
                println!(
                    "{k:>6} | {:>6.2}% | {:>6.2}% {:>6.2}% | {:>+11.1}% over",
                    c.theory_cv * 100.0, c.cv_rms * 100.0, r.cv_rms * 100.0, (c.max_ratio_mean - 1.0) * 100.0
                );
            }
            rows.iter().for_each(|row| run.row(row));
        }
    }
    run.finish();

    // Cross-check against the real crate: count 10M keys, then remove the
    // multinomial sampling noise: CV_ring² ≈ CV_counted² − (N−1)/keys.
    let mut chk = Run::start(&args.common, "e1_crosscheck", serde_json::json!({"keys": args.keys})).unwrap();
    println!("\nCross-check with pingora-ketama (N=100, {} random keys):", args.keys);
    println!("{:>5} {:>8} | {:>9} {:>11} {:>10}", "k", "keys", "exact CV", "counted CV", "corrected");
    let addrs = fleet(100, seed, 0);
    let buckets: Vec<_> = addrs.iter().map(|&a| (a, 1)).collect();
    let idx: HashMap<_, _> = addrs.iter().enumerate().map(|(i, &a)| (a, i)).collect();
    let rkeys = random_keys(args.keys, seed, 10);
    let ukeys = url_keys(args.keys / 5, seed, 11);
    for k in [1u32, 16, 160] {
        let version = Version::V2 { point_multiple: k };
        let exact = Ring::build(&buckets, version, PointSource::CrcChain);
        let exact_cv = stats::cv(&exact.exact_shares());
        let real = crate_ring(&buckets, version);
        let count = |keys: Vec<&[u8]>| {
            let parts = par_map(&keys.chunks(1 << 16).collect::<Vec<_>>(), |c| {
                let mut cnt = vec![0u64; 100];
                for key in c.iter() {
                    cnt[idx[&real.node(key).unwrap()]] += 1;
                }
                cnt
            });
            let mut cnt = vec![0f64; 100];
            for p in parts { for (a, b) in cnt.iter_mut().zip(p) { *a += b as f64; } }
            cnt
        };
        for (key_type, counts) in [
            ("random16", count(rkeys.iter().map(|k| &k[..]).collect())),
            ("url", count(ukeys.iter().map(|k| k.as_bytes()).collect())),
        ] {
            let keys = counts.iter().sum::<f64>() as usize;
            let counted_cv = stats::cv(&counts);
            let sampling_cv = stats::sampling_cv(100.0, keys as f64);
            let corrected_cv = (counted_cv.powi(2) - sampling_cv.powi(2)).max(0.0).sqrt();
            println!("{k:>5} {key_type:>8} | {:>8.2}% {:>10.2}% {:>9.2}%", exact_cv * 100.0, counted_cv * 100.0, corrected_cv * 100.0);
            chk.row(&Check { n: 100, k, key_type, keys, exact_cv, counted_cv, sampling_cv, corrected_cv });
        }
    }
    chk.finish();
}
