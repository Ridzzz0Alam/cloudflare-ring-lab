//! Small statistics helpers and the theory formulas we test against.

pub const RING: f64 = 4_294_967_296.0; // 2^32

pub fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Population standard deviation (divide by n).
pub fn sd(xs: &[f64]) -> f64 {
    let m = mean(xs);
    (xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / xs.len() as f64).sqrt()
}

pub fn cv(xs: &[f64]) -> f64 {
    sd(xs) / mean(xs)
}

/// CV of loads relative to their *expected* values: `sd(actual/expected)`.
/// For equal weights this equals `cv`, because the mean share is exactly 1/N.
pub fn relative_cv(actual: &[f64], expected: &[f64]) -> f64 {
    let r: Vec<f64> = actual.iter().zip(expected).map(|(a, e)| a / e).collect();
    let m2 = r.iter().map(|x| (x - 1.0).powi(2)).sum::<f64>() / r.len() as f64;
    m2.sqrt()
}

pub fn max(xs: &[f64]) -> f64 {
    xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
}

pub fn min(xs: &[f64]) -> f64 {
    xs.iter().cloned().fold(f64::INFINITY, f64::min)
}

/// Root-mean-square, used to average CVs across repeats (averaging CV²
/// is what the variance formula predicts).
pub fn rms(xs: &[f64]) -> f64 {
    (xs.iter().map(|x| x * x).sum::<f64>() / xs.len() as f64).sqrt()
}

/// The article's formula: CV_k = sqrt((N-1)/(N·k+1)). k=1 gives sqrt((N-1)/(N+1)).
pub fn theory_cv(n: f64, k: f64) -> f64 {
    ((n - 1.0) / (n * k + 1.0)).sqrt()
}

/// Expected number of colliding (dropped) points when M uniform points are
/// thrown into 2^32 slots: M − 2^32·(1 − e^(−M/2^32)).
pub fn birthday_dropped(m: f64) -> f64 {
    m - RING * (1.0 - (-m / RING).exp())
}

/// The quick approximation M²/(2·2^32).
pub fn birthday_dropped_approx(m: f64) -> f64 {
    m * m / (2.0 * RING)
}

/// Expected multinomial sampling CV when counting `keys` keys over N equal servers.
pub fn sampling_cv(n: f64, keys: f64) -> f64 {
    ((n - 1.0) / keys).sqrt()
}

/// How evenly a ring spreads load, relative to each server's expected share.
#[derive(Clone, Copy, Debug)]
pub struct Balance {
    /// CV of actual/expected (= plain CV for equal weights).
    pub cv: f64,
    /// Busiest server's load / its expected load.
    pub max_ratio: f64,
    /// Idlest server's load / its expected load.
    pub min_ratio: f64,
}

pub fn balance(actual: &[f64], expected: &[f64]) -> Balance {
    let r: Vec<f64> = actual.iter().zip(expected).map(|(a, e)| a / e).collect();
    Balance {
        cv: relative_cv(actual, expected),
        max_ratio: max(&r),
        min_ratio: min(&r),
    }
}

/// CV of a server with weight `w` out of total weight `total`, with `k` points
/// per weight unit: its share is Beta(w·k, (total−w)·k), so
/// CV² = (total − w) / (w · (total·k + 1)). With w=1, total=N this is the
/// article's formula.
pub fn theory_cv_weighted(w: f64, total: f64, k: f64) -> f64 {
    ((total - w) / (w * (total * k + 1.0))).sqrt()
}
