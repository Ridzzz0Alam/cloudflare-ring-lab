//! Section "When More Hashes Make Things Worse" (E2): 32-bit collisions at high k.
//!
//! cargo run --release --example 07_collisions
use clap::Parser;
use ring_lab::model::{crc_chain, PointSource, Ring, TieBreak};
use ring_lab::stats::{birthday_dropped, cv, mean, rms, sd, theory_cv, RING};
use ring_lab::{fleet, par_map, CommonArgs, Run};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Rings per (N, k) when N·k ≤ 50M points (bigger rings use 1)
    #[arg(long, default_value_t = 3)]
    reps: u64,
}

#[derive(Serialize)]
struct Row {
    n: usize,
    k: u32,
    variant: &'static str,
    reps: u64,
    generated: usize,
    unique: usize,
    dropped: usize,
    dropped_frac: f64,
    birthday_dropped: f64,
    self_repeating_servers: usize,
    cv: f64,
    theory_cv: f64,
    theory_cv_with_tie_bias: f64,
    cv_vs_theory: f64,
    /// Correlation between a server's address rank and its share (V2 favours low addresses).
    rank_corr: f64,
}

/// Pearson correlation of share with server index, after sorting servers by address.
fn rank_corr(ring: &Ring) -> f64 {
    let mut order: Vec<usize> = (0..ring.addrs.len()).collect();
    order.sort_by_key(|&i| ring.addrs[i]);
    let s = ring.exact_shares();
    let y: Vec<f64> = order.iter().map(|&i| s[i]).collect();
    let x: Vec<f64> = (0..y.len()).map(|r| r as f64).collect();
    let (mx, my) = (mean(&x), mean(&y));
    let cov = x.iter().zip(&y).map(|(a, b)| (a - mx) * (b - my)).sum::<f64>() / x.len() as f64;
    cov / (sd(&x) * sd(&y))
}

// (name, point source is CRC?, tie-break). "crc" is exactly what pingora-ketama V2 does.
const VARIANTS: [(&str, bool, TieBreak); 4] = [
    ("crc", true, TieBreak::LowestAddr),
    ("random", false, TieBreak::LowestAddr),
    ("crc_unstable_tie", true, TieBreak::Unstable),
    ("crc_hashed_tie", true, TieBreak::Hashed),
];

const KS: [u32; 8] = [1_000, 2_000, 5_000, 10_000, 20_000, 50_000, 100_000, 200_000];

fn main() {
    let args = Args::parse();
    let seed = args.common.seed;
    let mut run = Run::start(&args.common, "e2_collisions", serde_json::json!({"ks": KS, "reps": args.reps})).unwrap();
    println!(
        "{:>5} {:>7} | {:>7} {:>7} | {:>7} {:>7} | {:>7} {:>7} {:>7} | {:>6}",
        "N", "k", "dropped", "bday", "theory", "+bias", "V2 crc", "random", "fair", "corr"
    );
    for n in [256, 2048] {
        // 1. Each server's CRC chain is a permutation cycle: does it come back to 0 within k steps?
        let max_k = *KS.last().unwrap() as usize;
        let periods: Vec<Option<usize>> = par_map(&fleet(n, seed, 0), |a| crc_chain(a).take(max_k).position(|h| h == 0).map(|p| p + 1));
        for k in KS {
            let m = n * k as usize;
            let reps = if m <= 50_000_000 { args.reps } else { 1 };
            // Each point collides with probability q; V2 hands every collision to the lower
            // address, so shares drift linearly with address rank: an extra CV of q/sqrt(12).
            let q = 1.0 - (-(m as f64) / RING).exp();
            let thy = theory_cv(n as f64, k as f64);
            let mut out = vec![];
            for (variant, crc, tie) in VARIANTS {
                // 2. Build the ring, count what dedup removed, measure CV from exact arcs.
                let (mut dropped, mut cvs, mut corrs) = (0, vec![], vec![]);
                for rep in 0..reps {
                    let source = if crc { PointSource::CrcChain } else { PointSource::Random { seed: seed + 1_000 * rep } };
                    let buckets: Vec<_> = fleet(n, seed, rep).into_iter().map(|a| (a, 1)).collect();
                    let ring = Ring::build_with(&buckets, k, source, tie);
                    dropped += ring.generated - ring.len();
                    cvs.push(cv(&ring.exact_shares()));
                    corrs.push(rank_corr(&ring));
                }
                let dropped = dropped / reps as usize;
                let row = Row {
                    n, k, variant, reps,
                    generated: m, unique: m - dropped, dropped,
                    dropped_frac: dropped as f64 / m as f64,
                    birthday_dropped: birthday_dropped(m as f64),
                    self_repeating_servers: periods.iter().filter(|p| p.is_some_and(|p| p < k as usize)).count(),
                    cv: rms(&cvs),
                    theory_cv: thy,
                    theory_cv_with_tie_bias: (thy * thy + q * q / 12.0).sqrt(),
                    cv_vs_theory: rms(&cvs) / thy,
                    rank_corr: mean(&corrs),
                };
                run.row(&row);
                out.push(row);
            }
            let (c, r, f) = (&out[0], &out[1], &out[3]);
            println!(
                "{n:>5} {k:>7} | {:>7} {:>7.0} | {:>6.3}% {:>6.3}% | {:>6.3}% {:>6.3}% {:>6.3}% | {:>+6.2}",
                c.dropped, c.birthday_dropped, c.theory_cv * 100.0, c.theory_cv_with_tie_bias * 100.0,
                c.cv * 100.0, r.cv * 100.0, f.cv * 100.0, c.rank_corr
            );
        }
        let cycles: Vec<usize> = periods.iter().flatten().cloned().collect();
        println!("      N={n}: {} of {n} servers' CRC chains cycle back to 0 within {max_k} points {:?}", cycles.len(), &cycles[..cycles.len().min(5)]);
    }
    run.finish();
}
