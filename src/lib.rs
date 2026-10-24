//! ring-lab: shared helpers for the experiments.
//!
//! - fake fleets (seeded random IPv4 addresses)
//! - seeded keys (random 16-byte keys and URL-like strings)
//! - a CSV + `.meta.json` writer so every number can be traced to a run
//! - a tiny scoped-thread parallel map

pub mod model;
pub mod stats;

use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::Serialize;

/// The upstream commit pinned in Cargo.toml.
pub const PINGORA_COMMIT: &str = "623e163dfab9685c4a49bab08ed78838dd366ba6";

/// Default seed used by every program.
pub const DEFAULT_SEED: u64 = 42;

/// Arguments shared by every example.
#[derive(clap::Args, Debug, Clone)]
pub struct CommonArgs {
    /// RNG seed (fleets, keys, random-points variant)
    #[arg(long, default_value_t = DEFAULT_SEED)]
    pub seed: u64,
    /// Directory for CSV + meta output
    #[arg(long, default_value = "results")]
    pub out_dir: PathBuf,
}

/// A seeded RNG. `stream` lets one seed drive many independent streams
/// (e.g. one per repeat) without them overlapping.
pub fn rng(seed: u64, stream: u64) -> ChaCha8Rng {
    let mut r = ChaCha8Rng::seed_from_u64(seed);
    r.set_stream(stream);
    r
}

/// `n` distinct random addresses in 10.0.0.0/8, all on port 443.
pub fn fleet(n: usize, seed: u64, stream: u64) -> Vec<SocketAddr> {
    let mut r = rng(seed, stream);
    let mut seen = HashSet::with_capacity(n);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let ip = 0x0A00_0000 | (r.random::<u32>() & 0x00FF_FFFF);
        if seen.insert(ip) {
            out.push(SocketAddr::new(Ipv4Addr::from(ip).into(), 443));
        }
    }
    out
}

/// `n` random 16-byte keys.
pub fn random_keys(n: usize, seed: u64, stream: u64) -> Vec<[u8; 16]> {
    let mut r = rng(seed, stream);
    (0..n)
        .map(|_| {
            let mut k = [0u8; 16];
            r.fill(&mut k);
            k
        })
        .collect()
}

/// `n` URL-like keys such as `https://cdn7.example.com/img/a8f3c1/41522.jpg`.
pub fn url_keys(n: usize, seed: u64, stream: u64) -> Vec<String> {
    const DIRS: [&str; 6] = ["img", "js", "css", "video", "api/v2", "static/fonts"];
    const EXTS: [&str; 6] = ["jpg", "png", "js", "css", "mp4", "woff2"];
    let mut r = rng(seed, stream);
    (0..n)
        .map(|_| {
            format!(
                "https://cdn{}.example.com/{}/{:06x}/{}.{}",
                r.random_range(0..32),
                DIRS[r.random_range(0..DIRS.len())],
                r.random::<u32>() & 0xFF_FFFF,
                r.random_range(0..100_000),
                EXTS[r.random_range(0..EXTS.len())],
            )
        })
        .collect()
}

/// Run `f` over `items` on all cores, keeping input order.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = items.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|c| s.spawn(|| c.iter().map(&f).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

// ---------------------------------------------------------------------------
// Run output: results/<name>_<timestamp>.csv + .meta.json
// ---------------------------------------------------------------------------

/// One CSV output of one run, with its metadata written up front.
pub struct Run {
    csv: csv::Writer<File>,
    pub csv_path: PathBuf,
}

impl Run {
    /// Create `<out_dir>/<name>_<timestamp>.csv` and the sibling `.meta.json`.
    /// `params` records any experiment-specific settings.
    pub fn start(
        args: &CommonArgs,
        name: &str,
        params: serde_json::Value,
    ) -> io::Result<Self> {
        fs::create_dir_all(&args.out_dir)?;
        let stamp = utc_stamp();
        let csv_path = args.out_dir.join(format!("{name}_{stamp}.csv"));
        let meta_path = csv_path.with_extension("meta.json");
        let meta = serde_json::json!({
            "name": name,
            "timestamp_utc": stamp,
            "command": std::env::args().collect::<Vec<_>>().join(" "),
            "seed": args.seed,
            "params": params,
            "pingora_commit": PINGORA_COMMIT,
            "ring_lab_commit": git_commit(),
            "rustc": cmd_out("rustc", &["--version"]),
            "os": os_name(),
            "kernel": cmd_out("uname", &["-sr"]),
            "cpu": cpu_model(),
            "cpus": std::thread::available_parallelism().map_or(1, |n| n.get()),
            "ram_gb": ram_gb(),
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        });
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)?;
        let csv = csv::Writer::from_path(&csv_path)?;
        Ok(Run { csv, csv_path })
    }

    pub fn row<S: Serialize>(&mut self, row: &S) {
        self.csv.serialize(row).expect("csv write");
    }

    pub fn finish(mut self) {
        self.csv.flush().expect("csv flush");
        println!("→ wrote {}", self.csv_path.display());
    }
}

fn cmd_out(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn git_commit() -> String {
    let head = cmd_out("git", &["rev-parse", "--short=12", "HEAD"]);
    let dirty = !cmd_out("git", &["status", "--porcelain", "--untracked-files=no"]).is_empty();
    if head.is_empty() {
        "unknown".into()
    } else if dirty {
        format!("{head}-dirty")
    } else {
        head
    }
}

fn os_name() -> String {
    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string()))
        })
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

pub fn cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("model name").map(|v| v.trim_start_matches([' ', '\t', ':']).to_string()))
        })
        .unwrap_or_else(|| "unknown".into())
}

fn ram_gb() -> f64 {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("MemTotal:")
                    .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok())
            })
        })
        .map_or(0.0, |kb| (kb / 1024.0 / 1024.0 * 10.0).round() / 10.0)
}

/// `YYYYMMDDTHHMMSSZ` in UTC, without pulling in a date crate.
fn utc_stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
