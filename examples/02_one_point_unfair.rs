//! Section "Why One Point Per Server Is Unfair" (E1 at k = 1).
//!
//! cargo run --release --example 02_one_point_unfair
use clap::Parser;
use ring_lab::model::{PointSource, Ring, Version};
use ring_lab::stats::{self, balance, mean, theory_cv};
use ring_lab::{fleet, par_map, CommonArgs, Run};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Random rings per N
    #[arg(long, default_value_t = 200)]
    reps: u64,
}

#[derive(Serialize)]
struct Row {
    n: usize,
    variant: &'static str,
    rep: u64,
    cv: f64,
    max_ratio: f64,
    min_ratio: f64,
    theory_cv: f64,
}

fn main() {
    let args = Args::parse();
    let seed = args.common.seed;
    let mut run = Run::start(&args.common, "e1_k1", serde_json::json!({"reps": args.reps})).unwrap();
    let one_point = Version::V2 { point_multiple: 1 };

    println!("One point per server: CV of server load (exact arc lengths, {} rings each)", args.reps);
    println!("{:>5} | {:>9} | {:>9} {:>9} | {:>12} {:>12}", "N", "theory", "CRC32", "random", "busiest/avg", "idlest/avg");
    for n in [10, 100, 1000] {
        let reps: Vec<u64> = (0..args.reps).collect();
        let mut res = vec![];
        for (variant, src) in [("crc", true), ("random", false)] {
            // Build `reps` rings with random fleets (CRC) or random points, and measure each.
            let b = par_map(&reps, |&rep| {
                let addrs = fleet(n, seed, rep);
                let source = if src { PointSource::CrcChain } else { PointSource::Random { seed: seed + 1_000 * rep } };
                let ring = Ring::equal(&addrs, one_point, source);
                balance(&ring.exact_shares(), &ring.expected_shares())
            });
            for (rep, x) in b.iter().enumerate() {
                run.row(&Row { n, variant, rep: rep as u64, cv: x.cv, max_ratio: x.max_ratio, min_ratio: x.min_ratio, theory_cv: theory_cv(n as f64, 1.0) });
            }
            res.push(b);
        }
        let cv = |b: &Vec<stats::Balance>| stats::rms(&b.iter().map(|x| x.cv).collect::<Vec<_>>());
        println!(
            "{n:>5} | {:>8.1}% | {:>8.1}% {:>8.1}% | {:>11.2}x {:>11.3}x",
            theory_cv(n as f64, 1.0) * 100.0,
            cv(&res[0]) * 100.0,
            cv(&res[1]) * 100.0,
            mean(&res[0].iter().map(|x| x.max_ratio).collect::<Vec<_>>()),
            mean(&res[0].iter().map(|x| x.min_ratio).collect::<Vec<_>>()),
        );
    }

    // One concrete N=100 ring, to make the numbers tangible.
    let ring = Ring::equal(&fleet(100, seed, 0), one_point, PointSource::CrcChain);
    let s = ring.exact_shares();
    println!(
        "\nOne N=100 ring: fair share 1.00%, busiest server {:.2}%, idlest {:.4}%",
        stats::max(&s) * 100.0,
        stats::min(&s) * 100.0
    );
    run.finish();
}
