//! Section "Our Fleet and Keys": what the fake servers and keys look like.
//!
//! cargo run --release --example 00_fleet_and_keys
use clap::Parser;
use ring_lab::stats::RING;
use ring_lab::{fleet, random_keys, url_keys, CommonArgs};

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
}

fn main() {
    let args = Args::parse();
    let seed = args.common.seed;

    // 1. A fleet: N distinct random IPv4 addresses in 10.0.0.0/8, port 443.
    //    The third argument picks an independent stream, e.g. one per repeat.
    println!("fleet(5, seed={seed}, stream=0):");
    for a in fleet(5, seed, 0) {
        println!("  {a}");
    }

    // 2. Keys: random 16-byte strings, and URL-like strings. Both are hashed with CRC32,
    //    the same hash pingora-ketama uses for lookups.
    println!("\nrandom_keys(3) and url_keys(3), with their CRC32 position on the ring:");
    for k in random_keys(3, seed, 1) {
        let hex: String = k.iter().map(|b| format!("{b:02x}")).collect();
        println!("  {hex:<46} → {:>5.1}%", crc32fast::hash(&k) as f64 / RING * 100.0);
    }
    for k in url_keys(3, seed, 2) {
        println!("  {k:<46} → {:>5.1}%", crc32fast::hash(k.as_bytes()) as f64 / RING * 100.0);
    }

    // 3. CRC32 spreads both key types evenly: fraction of 1M keys in each tenth of the ring.
    let deciles = |hs: Vec<u32>| {
        let mut c = [0usize; 10];
        hs.iter().for_each(|&h| c[(h as u64 * 10 >> 32) as usize] += 1);
        c.iter().map(|&x| format!("{:.1}", x as f64 / hs.len() as f64 * 100.0)).collect::<Vec<_>>().join(" ")
    };
    println!("\n% of 1M keys per tenth of the ring:");
    println!("  random: {}", deciles(random_keys(1_000_000, seed, 3).iter().map(|k| crc32fast::hash(k)).collect()));
    println!("  url:    {}", deciles(url_keys(1_000_000, seed, 4).iter().map(|k| crc32fast::hash(k.as_bytes())).collect()));
}
