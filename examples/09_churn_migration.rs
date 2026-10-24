//! Section "Migrating Without Melting the Cache" (E6): how many keys move?
//!
//! cargo run --release --example 09_churn_migration
use std::net::SocketAddr;

use clap::Parser;
use pingora_ketama::{Bucket, Continuum, Version};
use ring_lab::model::crc_chain;
use ring_lab::{fleet, par_map, random_keys, rng, CommonArgs, Run};
use rand::Rng;
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Keys sampled per scenario
    #[arg(long, default_value_t = 1_000_000)]
    keys: usize,
}

#[derive(Serialize)]
struct Row {
    scenario: &'static str,
    n: usize,
    keys: usize,
    moved_frac: f64,
    expected_frac: f64,
    /// Moved keys that did not have to move (e.g. not on the removed server).
    unnecessary_moves: usize,
}

fn ring(addrs: &[SocketAddr], v: Version) -> Continuum {
    let b: Vec<Bucket> = addrs.iter().map(|&a| Bucket::new(a, 1)).collect();
    Continuum::new_with_version(&b, v)
}

/// A router closure for a ring.
fn at(c: &Continuum) -> impl Fn(&[u8]) -> SocketAddr + Sync + '_ {
    move |k| c.node(k).unwrap()
}

/// Route every key through two routers and count how many land on a different server.
/// `must_move(old, new)` says whether a move was required.
fn moved(keys: &[[u8; 16]], a: impl Fn(&[u8]) -> SocketAddr + Sync, b: impl Fn(&[u8]) -> SocketAddr + Sync,
         must_move: impl Fn(SocketAddr, SocketAddr) -> bool + Sync) -> (f64, usize) {
    let parts = par_map(&keys.chunks(1 << 15).collect::<Vec<_>>(), |c| {
        let (mut m, mut bad) = (0usize, 0usize);
        for k in c.iter() {
            let (x, y) = (a(k), b(k));
            if x != y {
                m += 1;
                bad += usize::from(!must_move(x, y));
            }
        }
        (m, bad)
    });
    let (m, bad) = parts.iter().fold((0, 0), |s, p| (s.0 + p.0, s.1 + p.1));
    (m as f64 / keys.len() as f64, bad)
}

const V1: Version = Version::V1;
const fn v2(pm: u32) -> Version { Version::V2 { point_multiple: pm } }

fn main() {
    let args = Args::parse();
    let seed = args.common.seed;
    let mut run = Run::start(&args.common, "e6_churn", serde_json::json!({"keys": args.keys})).unwrap();
    let keys = random_keys(args.keys, seed, 20);
    // scenario → (moved fraction per N, expected per N, unnecessary moves per N)
    type Table = Vec<(&'static str, Vec<f64>, Vec<f64>, Vec<usize>)>;
    let mut table: Table = vec![];
    let mut add = |row: Row, table: &mut Table| {
        match table.iter_mut().find(|t| t.0 == row.scenario) {
            Some(t) => { t.1.push(row.moved_frac); t.2.push(row.expected_frac); t.3.push(row.unnecessary_moves) }
            None => table.push((row.scenario, vec![row.moved_frac], vec![row.expected_frac], vec![row.unnecessary_moves])),
        }
        run.row(&row);
    };

    for n in [10, 100, 1000] {
        let addrs = fleet(n + 1, seed, 0);
        let (old, extra) = (&addrs[..n], addrs[n]);
        let base = ring(old, V1);

        // 1. Remove one server: only its keys should move (≈ 1/N).
        // 2. Add one server: only keys that now go to the new server should move (≈ 1/(N+1)).
        //    Done for V1 and V2; `unnecessary_moves` counts keys that moved without needing to.
        let victim = old[rng(seed, 30).random_range(0..n)];
        let without: Vec<_> = old.iter().cloned().filter(|&a| a != victim).collect();
        for (rm, ad, v) in [("remove 1 server", "add 1 server", V1), ("remove 1 server (V2)", "add 1 server (V2)", v2(160))] {
            let before = ring(old, v);
            let (f, bad) = moved(&keys, at(&before), at(&ring(&without, v)), |o, _| o == victim);
            add(Row { scenario: rm, n, keys: args.keys, moved_frac: f, expected_frac: 1.0 / n as f64, unnecessary_moves: bad }, &mut table);
            let (f, bad) = moved(&keys, at(&before), at(&ring(&addrs, v)), |_, nw| nw == extra);
            add(Row { scenario: ad, n, keys: args.keys, moved_frac: f, expected_frac: 1.0 / (n + 1) as f64, unnecessary_moves: bad }, &mut table);
        }

        // 3. Naive hash(key) % N, growing N → N+1: almost everything moves.
        let all = &addrs;
        let modn = |m: usize| move |k: &[u8]| all[crc32fast::hash(k) as usize % m];
        let (f, _) = moved(&keys, modn(n), modn(n + 1), |_, _| true);
        add(Row { scenario: "naive hash % N, add 1", n, keys: args.keys, moved_frac: f, expected_frac: n as f64 / (n + 1) as f64, unnecessary_moves: 0 }, &mut table);

        // 4. The article's migration, same servers: V1(160) → V2(160) and V1(160) → V2(16).
        let r160 = ring(old, v2(160));
        let (f, _) = moved(&keys, at(&base), at(&r160), |_, _| true);
        add(Row { scenario: "V1(160) → V2(160)", n, keys: args.keys, moved_frac: f, expected_frac: 0.0, unnecessary_moves: 0 }, &mut table);
        let r16 = ring(old, v2(16));
        let (f, _) = moved(&keys, at(&base), at(&r16), |_, _| true);
        // V2(16)'s points are the first 16 links of each V1 chain (a subset), so a key keeps its
        // server only if its V1 point survives (1/10) or it lands on the same server by chance.
        let expect = 1.0 - (0.1 + 0.9 / n as f64);
        add(Row { scenario: "V1(160) → V2(16)", n, keys: args.keys, moved_frac: f, expected_frac: expect, unnecessary_moves: 0 }, &mut table);

        // 5. Stepping down gradually: each step moves fewer keys, but the steps add up.
        let mut prev = ring(old, V1);
        for (scenario, pm) in [("step 160 → 80", 80), ("step 80 → 40", 40), ("step 40 → 16", 16)] {
            let next = ring(old, v2(pm));
            let (f, _) = moved(&keys, at(&prev), at(&next), |_, _| true);
            add(Row { scenario, n, keys: args.keys, moved_frac: f, expected_frac: f64::NAN, unnecessary_moves: 0 }, &mut table);
            prev = next;
        }
    }
    run.finish();

    // Sanity check of the subset claim: the first 16 chain links equal the first 16 of 160.
    let a = fleet(1, seed, 0)[0];
    assert!(crc_chain(&a).take(16).eq(crc_chain(&a).take(160).take(16)));

    println!("% of {} keys that change server", args.keys);
    println!("{:<22} | {:>7} {:>7} {:>7} | {:>9} | {:>11}", "scenario", "N=10", "N=100", "N=1000", "expected", "needless");
    for (s, m, e, u) in &table {
        let pct = |x: f64| if x.is_nan() { "-".to_string() } else { format!("{:.2}%", x * 100.0) };
        let applies = s.starts_with("remove") || s.starts_with("add");
        let needless = if applies { u.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("/") } else { "-".into() };
        println!("{s:<22} | {:>7} {:>7} {:>7} | {:>9} | {needless:>11}", pct(m[0]), pct(m[1]), pct(m[2]), pct(e[1]));
    }
    println!("(expected is for N=100; needless = keys that moved but did not have to, for N=10/100/1000)");
}
