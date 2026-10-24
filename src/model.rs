//! A reference re-implementation of `pingora_ketama::Continuum`.
//!
//! The crate does not expose its points, so we rebuild the ring ourselves
//! (point generation, sort, dedup, lookup) and validate it against
//! `Continuum::node()` in `tests/model_matches_crate.rs`. With the points in
//! hand we can compute each server's *exact* share of the 2^32 ring.

use std::cmp::Ordering;
use std::io::Write;
use std::net::SocketAddr;

use crc32fast::Hasher;
use rand::Rng;

use crate::stats::RING;

/// Which crate version to mirror.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// 160 points per weight unit, 8-byte points, unstable sort by hash.
    V1,
    /// `point_multiple` points per weight unit, sort by (hash, address).
    V2 { point_multiple: u32 },
}

impl Version {
    pub fn point_multiple(&self) -> u32 {
        match self {
            Version::V1 => pingora_ketama::DEFAULT_POINT_MULTIPLE,
            Version::V2 { point_multiple } => *point_multiple,
        }
    }

    /// The matching crate enum.
    pub fn to_crate(self) -> pingora_ketama::Version {
        match self {
            Version::V1 => pingora_ketama::Version::V1,
            Version::V2 { point_multiple } => pingora_ketama::Version::V2 { point_multiple },
        }
    }
}

/// Where point hashes come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointSource {
    /// The crate's nginx-compatible CRC32 chain.
    CrcChain,
    /// Truly independent uniform u32s from a seeded RNG (the textbook assumption).
    Random { seed: u64 },
}

/// Which of several points with the same hash survives dedup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TieBreak {
    /// V1: whatever std's `sort_unstable` leaves first.
    Unstable,
    /// V2: the server with the lowest `SocketAddr` wins.
    LowestAddr,
    /// Not in the crate: a pseudo-random but deterministic winner,
    /// ordering by crc32(hash ++ server address bytes).
    Hashed,
}

/// The bytes the crate hashes for a server: `"<ip>\0<port>"`.
pub fn base_bytes(addr: &SocketAddr) -> Vec<u8> {
    let mut b = Vec::with_capacity(45);
    write!(&mut b, "{}\0{}", addr.ip(), addr.port()).unwrap();
    b
}

/// The crate's point chain for one server:
/// `hash_i = crc32(base_bytes ++ hash_{i-1}.to_le_bytes())`, starting from 0.
pub fn crc_chain(addr: &SocketAddr) -> impl Iterator<Item = u32> {
    let mut base = Hasher::new();
    base.update(&base_bytes(addr));
    let mut prev = 0u32;
    std::iter::repeat_with(move || {
        let mut h = base.clone();
        h.update(&prev.to_le_bytes());
        prev = h.finalize();
        prev
    })
}

// Same layout and ordering as the crate's `PointV1`, so `sort_unstable`
// breaks ties among equal hashes exactly as the crate does.
// Deliberately not `Copy`: std's sort picks different code paths for `Copy`
// types, which would change which colliding point survives.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PointV1 {
    node: u32,
    hash: u32,
}
impl Ord for PointV1 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.hash.cmp(&other.hash)
    }
}
impl PartialOrd for PointV1 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A built ring: sorted, deduplicated point hashes and the owning server of each.
pub struct Ring {
    // Sorted, deduplicated points. `node` is the server index as the crate
    // stores it (cast through u16). Kept as one Vec so 400M-point rings fit in RAM.
    pts: Vec<PointV1>,
    pub addrs: Vec<SocketAddr>,
    pub weights: Vec<u32>,
    /// Points generated before dedup.
    pub generated: usize,
}

impl Ring {
    pub fn build(buckets: &[(SocketAddr, u32)], version: Version, source: PointSource) -> Self {
        let tie = match version {
            Version::V1 => TieBreak::Unstable,
            Version::V2 { .. } => TieBreak::LowestAddr,
        };
        Self::build_with(buckets, version.point_multiple(), source, tie)
    }

    /// Like `build`, with any points per weight unit and any tie-break rule.
    pub fn build_with(buckets: &[(SocketAddr, u32)], pm: u32, source: PointSource, tie: TieBreak) -> Self {
        let pm = pm as usize;
        let addrs: Vec<SocketAddr> = buckets.iter().map(|b| b.0).collect();
        let weights: Vec<u32> = buckets.iter().map(|b| b.1).collect();
        let total: usize = weights.iter().map(|&w| w as usize * pm).sum();

        let mut pts: Vec<PointV1> = Vec::with_capacity(total);
        for (i, &(addr, w)) in buckets.iter().enumerate() {
            let n = w as usize * pm;
            // The crate pushes `node as u16` in both versions.
            let node = (i as u16) as u32;
            match source {
                PointSource::CrcChain => {
                    pts.extend(crc_chain(&addr).take(n).map(|hash| PointV1 { node, hash }))
                }
                PointSource::Random { seed } => {
                    let mut r = crate::rng(seed, i as u64);
                    pts.extend((0..n).map(|_| PointV1 { node, hash: r.random() }))
                }
            }
        }
        let generated = pts.len();

        match tie {
            TieBreak::Unstable => pts.sort_unstable(),
            TieBreak::LowestAddr => pts.sort_unstable_by(|a, b| {
                a.hash
                    .cmp(&b.hash)
                    .then_with(|| addrs[a.node as usize].cmp(&addrs[b.node as usize]))
            }),
            TieBreak::Hashed => {
                let salt: Vec<Vec<u8>> = addrs.iter().map(base_bytes).collect();
                let rank = |p: &PointV1| {
                    let mut h = Hasher::new();
                    h.update(&p.hash.to_le_bytes());
                    h.update(&salt[p.node as usize]);
                    h.finalize()
                };
                pts.sort_unstable_by(|a, b| a.hash.cmp(&b.hash).then_with(|| rank(a).cmp(&rank(b))))
            }
        }
        pts.dedup_by(|a, b| a.hash == b.hash);

        Ring {
            pts,
            addrs,
            weights,
            generated,
        }
    }

    /// Convenience: equal-weight fleet.
    pub fn equal(addrs: &[SocketAddr], version: Version, source: PointSource) -> Self {
        let b: Vec<_> = addrs.iter().map(|&a| (a, 1)).collect();
        Self::build(&b, version, source)
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    pub fn hash(&self, i: usize) -> u32 {
        self.pts[i].hash
    }

    pub fn owner(&self, i: usize) -> usize {
        self.pts[i].node as usize
    }

    /// `(hash, owner)` for every point, in ring order.
    pub fn points(&self) -> impl Iterator<Item = (u32, usize)> + '_ {
        self.pts.iter().map(|p| (p.hash, p.node as usize))
    }

    /// Index of the first point with hash >= `h`, wrapping to 0.
    pub fn point_for_hash(&self, h: u32) -> usize {
        let i = self.pts.partition_point(|p| p.hash < h);
        if i == self.pts.len() {
            0
        } else {
            i
        }
    }

    pub fn server_for_hash(&self, h: u32) -> usize {
        self.owner(self.point_for_hash(h))
    }

    pub fn node(&self, key: &[u8]) -> SocketAddr {
        self.addrs[self.server_for_hash(crc32fast::hash(key))]
    }

    /// Exact fraction of the 2^32 key space owned by each server.
    /// Point i owns hashes in (hash[i-1], hash[i]]; point 0 also owns the wrap-around.
    pub fn exact_shares(&self) -> Vec<f64> {
        let mut owned = vec![0u64; self.addrs.len()];
        let n = self.pts.len();
        for i in 0..n {
            let arc = if i == 0 {
                (1u64 << 32) + self.hash(0) as u64 - self.hash(n - 1) as u64
            } else {
                (self.hash(i) - self.hash(i - 1)) as u64
            };
            owned[self.owner(i)] += arc;
        }
        owned.into_iter().map(|o| o as f64 / RING).collect()
    }

    /// Expected share of each server: weight / total weight.
    pub fn expected_shares(&self) -> Vec<f64> {
        let total: f64 = self.weights.iter().map(|&w| w as f64).sum();
        self.weights.iter().map(|&w| w as f64 / total).collect()
    }

    /// Points that survived dedup, per server.
    pub fn points_per_server(&self) -> Vec<usize> {
        let mut c = vec![0; self.addrs.len()];
        for p in &self.pts {
            c[p.node as usize] += 1;
        }
        c
    }
}

/// Build the real crate ring from `(addr, weight)` pairs.
pub fn crate_ring(buckets: &[(SocketAddr, u32)], version: Version) -> pingora_ketama::Continuum {
    let b: Vec<_> = buckets
        .iter()
        .map(|&(a, w)| pingora_ketama::Bucket::new(a, w))
        .collect();
    pingora_ketama::Continuum::new_with_version(&b, version.to_crate())
}

/// A 4-byte key whose CRC32 is exactly `target`.
///
/// CRC32 of a fixed-length input is affine over GF(2): crc(m) = L·m ⊕ crc(0).
/// We build L column by column and solve L·m = target ⊕ crc(0) by Gaussian
/// elimination. Used to aim keys at specific ring points.
pub fn forge_key(target: u32) -> [u8; 4] {
    let c0 = crc32fast::hash(&[0; 4]);
    // rows[r] = (bitmask of message bits feeding output bit r, rhs bit)
    let cols: Vec<u32> = (0..32)
        .map(|i| crc32fast::hash(&(1u32 << i).to_le_bytes()) ^ c0)
        .collect();
    let rhs = target ^ c0;
    let mut rows: Vec<(u32, u32)> = (0..32)
        .map(|r| {
            let mask = (0..32).fold(0u32, |m, i| m | (((cols[i] >> r) & 1) << i));
            (mask, (rhs >> r) & 1)
        })
        .collect();
    for col in 0..32 {
        let piv = (col..32).find(|&r| rows[r].0 >> col & 1 == 1).expect("CRC32 is invertible");
        rows.swap(col, piv);
        for r in 0..32 {
            if r != col && rows[r].0 >> col & 1 == 1 {
                rows[r].0 ^= rows[col].0;
                rows[r].1 ^= rows[col].1;
            }
        }
    }
    let m = (0..32).fold(0u32, |m, i| m | (rows[i].1 << i));
    m.to_le_bytes()
}
