//! Section "Weighted Ketama" (E3): a weight-w server should get w× the traffic.
//!
//! cargo run --release --example 04_weights
use clap::Parser;
use rand::Rng;
use ring_lab::model::{PointSource, Ring, Version};
use ring_lab::stats::{self, mean, rms, theory_cv_weighted};
use ring_lab::{fleet, par_map, rng, CommonArgs, Run};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Servers in the fleet
    #[arg(long, default_value_t = 100)]
    n: usize,
    /// Random fleets per version
    #[arg(long, default_value_t = 30)]
    reps: u64,
}

#[derive(Serialize)]
struct ServerRow {
    version: String,
    rep: u64,
    server: usize,
    weight: u32,
    points: usize,
    expected_share: f64,
    actual_share: f64,
}

#[derive(Serialize)]
struct WeightRow {
    version: String,
    weight: u32,
    servers: usize,
    points_per_server: f64,
    mean_ratio: f64,
    rel_error_rms: f64,
    theory_rel_error: f64,
    worst_overload: f64,
}

const WEIGHTS: [u32; 4] = [1, 2, 4, 8];

fn main() {
    let args = Args::parse();
    let (seed, n) = (args.common.seed, args.n);
    let params = serde_json::json!({"n": n, "reps": args.reps, "weights": WEIGHTS});
    let mut per_server = Run::start(&args.common, "e3_weights_servers", params.clone()).unwrap();
    let mut summary = Run::start(&args.common, "e3_weights", params).unwrap();
    let reps: Vec<u64> = (0..args.reps).collect();

    println!("N={n} servers, weights drawn from {WEIGHTS:?}, {} fleets per version", args.reps);
    println!("{:<8} {:>2} | {:>7} | {:>11} | {:>10} {:>8} | {:>9}", "version", "w", "points", "share/expct", "rel. error", "theory", "worst");
    for version in [Version::V1, Version::V2 { point_multiple: 16 }] {
        let vname = match version { Version::V1 => "V1(160)".to_string(), _ => "V2(16)".to_string() };
        // 1. Random fleets with random weights; exact shares from the model.
        let rings = par_map(&reps, |&rep| {
            let mut r = rng(seed, 500 + rep);
            let buckets: Vec<_> = fleet(n, seed, rep).into_iter().map(|a| (a, WEIGHTS[r.random_range(0..4)])).collect();
            let ring = Ring::build(&buckets, version, PointSource::CrcChain);
            (ring.weights.clone(), ring.points_per_server(), ring.expected_shares(), ring.exact_shares())
        });
        for (rep, (w, pts, exp, act)) in rings.iter().enumerate() {
            for i in 0..n {
                per_server.row(&ServerRow { version: vname.clone(), rep: rep as u64, server: i, weight: w[i], points: pts[i], expected_share: exp[i], actual_share: act[i] });
            }
        }
        // 2. Group servers by weight: is share/expected ≈ 1, and how big is the error?
        for weight in WEIGHTS {
            let (mut ratios, mut points, mut theory) = (vec![], vec![], vec![]);
            for (w, pts, exp, act) in &rings {
                let total: u32 = w.iter().sum();
                for i in (0..n).filter(|&i| w[i] == weight) {
                    ratios.push(act[i] / exp[i]);
                    points.push(pts[i] as f64);
                    theory.push(theory_cv_weighted(weight as f64, total as f64, version.point_multiple() as f64));
                }
            }
            let row = WeightRow {
                version: vname.clone(), weight, servers: ratios.len(),
                points_per_server: mean(&points),
                mean_ratio: mean(&ratios),
                rel_error_rms: rms(&ratios.iter().map(|r| r - 1.0).collect::<Vec<_>>()),
                theory_rel_error: rms(&theory),
                worst_overload: stats::max(&ratios) - 1.0,
            };
            println!(
                "{:<8} {:>2} | {:>7.0} | {:>11.3} | {:>9.1}% {:>7.1}% | {:>+8.0}%",
                row.version, weight, row.points_per_server, row.mean_ratio,
                row.rel_error_rms * 100.0, row.theory_rel_error * 100.0, row.worst_overload * 100.0
            );
            summary.row(&row);
        }
    }
    per_server.finish();
    summary.finish();
}
