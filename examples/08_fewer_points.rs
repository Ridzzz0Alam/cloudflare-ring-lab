//! Section "Trick 2: Cutting 90% of the Points" (E5): memory vs balance trade-off.
//!
//! cargo run --release --example 08_fewer_points
use clap::Parser;
use rand::Rng;
use ring_lab::model::{PointSource, Ring, Version};
use ring_lab::stats::{self, balance, mean, rms, theory_cv};
use ring_lab::{fleet, par_map, rng, CommonArgs, Run};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Random rings per configuration
    #[arg(long, default_value_t = 30)]
    reps: u64,
}

#[derive(Serialize)]
struct Row {
    fleet: &'static str,
    n: usize,
    k: u32,
    points_per_server: f64,
    bytes_per_server_v1: f64,
    bytes_per_server_v2: f64,
    theory_cv: f64,
    cv: f64,
    max_overload_mean: f64,
    max_overload_worst: f64,
}

const KS: [u32; 11] = [1, 2, 5, 10, 16, 50, 100, 160, 500, 1000, 1600];

fn main() {
    let args = Args::parse();
    let seed = args.common.seed;
    let mut run = Run::start(&args.common, "e5_tradeoff", serde_json::json!({"reps": args.reps, "ks": KS})).unwrap();
    let reps: Vec<u64> = (0..args.reps).collect();

    // `weights(rep)` gives each server's weight; "storage" mimics disk-size weights around 625.
    let fleets: [(&str, fn(u64, usize, u64) -> Vec<u32>); 2] = [
        ("weight1", |_, n, _| vec![1; n]),
        ("storage", |seed, n, rep| {
            let mut r = rng(seed, 900 + rep);
            (0..n).map(|_| r.random_range(400..=850)).collect()
        }),
    ];
    let mut rows = vec![];
    for (fname, weights) in fleets {
        for n in [100, 1000] {
            for k in KS {
                if fname == "storage" && ![1, 2, 5, 10, 16, 50, 100, 160].contains(&k) {
                    continue; // storage fleets already have 60k+ points per server at k=100
                }
                // 1. Many random fleets with k points per weight unit.
                let b = par_map(&reps, |&rep| {
                    let buckets: Vec<_> = fleet(n, seed, rep).into_iter().zip(weights(seed, n, rep)).collect();
                    let ring = Ring::build(&buckets, Version::V2 { point_multiple: k }, PointSource::CrcChain);
                    (balance(&ring.exact_shares(), &ring.expected_shares()), ring.len() as f64 / n as f64)
                });
                // 2. Memory per server is points × 8 bytes (V1) or × 6 bytes (V2).
                let pps = mean(&b.iter().map(|x| x.1).collect::<Vec<_>>());
                let maxes: Vec<f64> = b.iter().map(|x| x.0.max_ratio - 1.0).collect();
                let row = Row {
                    fleet: fname, n, k, points_per_server: pps,
                    bytes_per_server_v1: pps * 8.0, bytes_per_server_v2: pps * 6.0,
                    theory_cv: if fname == "weight1" { theory_cv(n as f64, k as f64) } else { f64::NAN },
                    cv: rms(&b.iter().map(|x| x.0.cv).collect::<Vec<_>>()),
                    max_overload_mean: mean(&maxes), max_overload_worst: stats::max(&maxes),
                };
                run.row(&row);
                rows.push(row);
            }
        }
    }
    run.finish();

    // 3. The headline comparison: 160 vs 16 points per weight unit.
    println!("{:<8} {:>5} {:>4} | {:>9} {:>9} | {:>7} | {:>13} {:>8}", "fleet", "N", "k", "pts/srv", "V2 bytes", "CV", "busiest (avg)", "(worst)");
    for r in rows.iter().filter(|r| r.k == 16 || r.k == 160) {
        println!(
            "{:<8} {:>5} {:>4} | {:>9.0} {:>9.0} | {:>6.2}% | {:>+12.1}% {:>+7.1}%",
            r.fleet, r.n, r.k, r.points_per_server, r.bytes_per_server_v2, r.cv * 100.0,
            r.max_overload_mean * 100.0, r.max_overload_worst * 100.0
        );
    }
}
