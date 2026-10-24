//! The reference model must route every key exactly like `Continuum::node()`.
//! Run with: cargo test --release

use ring_lab::model::{crate_ring, PointSource, Ring, Version};
use ring_lab::{fleet, random_keys, url_keys};

const KEYS: usize = 100_000;

fn check(n: usize, version: Version, weights: impl Fn(usize) -> u32) {
    let addrs = fleet(n, 42, n as u64);
    let buckets: Vec<_> = addrs.iter().enumerate().map(|(i, &a)| (a, weights(i))).collect();
    let model = Ring::build(&buckets, version, PointSource::CrcChain);
    let real = crate_ring(&buckets, version);

    let mut mismatches = 0;
    for k in random_keys(KEYS, 42, 1) {
        if Some(model.node(&k)) != real.node(&k) {
            mismatches += 1;
        }
    }
    for k in url_keys(KEYS / 10, 42, 2) {
        if Some(model.node(k.as_bytes())) != real.node(k.as_bytes()) {
            mismatches += 1;
        }
    }
    println!(
        "N={n:<5} {version:<28} points={:<8} dropped={:<3} mismatches={mismatches}",
        model.len(),
        model.generated - model.len(),
        version = format!("{version:?}"),
    );
    assert_eq!(mismatches, 0, "model disagrees with crate for N={n} {version:?}");
}

#[test]
fn model_matches_crate_equal_weights() {
    for n in [3, 100, 1000] {
        check(n, Version::V1, |_| 1);
        for pm in [1, 16, 160] {
            check(n, Version::V2 { point_multiple: pm }, |_| 1);
        }
    }
}

#[test]
fn model_matches_crate_mixed_weights() {
    for n in [3, 100, 1000] {
        check(n, Version::V1, |i| [1, 2, 4, 8][i % 4]);
        check(n, Version::V2 { point_multiple: 16 }, |i| [1, 2, 4, 8][i % 4]);
    }
}

/// The exact shares must sum to 1 and the point index must agree with the crate.
#[test]
fn exact_shares_sum_to_one_and_idx_matches() {
    let addrs = fleet(100, 42, 7);
    let b: Vec<_> = addrs.iter().map(|&a| (a, 1)).collect();
    let model = Ring::build(&b, Version::V2 { point_multiple: 16 }, PointSource::CrcChain);
    let s: f64 = model.exact_shares().iter().sum();
    assert!((s - 1.0).abs() < 1e-12);

    let real = crate_ring(&b, Version::V2 { point_multiple: 16 });
    for k in random_keys(10_000, 42, 3) {
        assert_eq!(model.point_for_hash(crc32fast::hash(&k)), real.node_idx(&k));
    }
}

/// Random keys rarely land on a collided point, so aim keys at every
/// collided hash directly and check the model picks the same surviving server.
#[test]
fn collision_winners_match_crate() {
    use ring_lab::model::{crc_chain, forge_key};
    for (n, version, w) in [
        (1000, Version::V1, 4u32),
        (1000, Version::V2 { point_multiple: 160 }, 4),
        (2048, Version::V2 { point_multiple: 1000 }, 1),
    ] {
        let addrs = fleet(n, 42, 99);
        let buckets: Vec<_> = addrs.iter().map(|&a| (a, w)).collect();
        let k = (w * version.point_multiple()) as usize;
        let mut all: Vec<u32> = addrs.iter().flat_map(|a| crc_chain(a).take(k)).collect();
        all.sort_unstable();
        let dups: Vec<u32> = all.windows(2).filter(|p| p[0] == p[1]).map(|p| p[0]).collect();
        assert!(!dups.is_empty());

        let model = Ring::build(&buckets, version, PointSource::CrcChain);
        let real = crate_ring(&buckets, version);
        for &h in &dups {
            let key = forge_key(h);
            assert_eq!(crc32fast::hash(&key), h);
            assert_eq!(Some(model.node(&key)), real.node(&key), "winner differs at {h:#x}");
        }
        println!("N={n} {version:?}: {} collided hashes, all winners match", dups.len());
    }
}
