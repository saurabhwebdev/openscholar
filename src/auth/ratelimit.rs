//! Login brute-force protection: an in-memory sliding-window per-IP limiter
//! (complements the DB-backed per-account lockout in `password`/`routes`).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Max failed login attempts per IP within the window before throttling.
pub const IP_MAX_FAILURES: usize = 20;
/// Sliding window length for the per-IP counter.
pub const IP_WINDOW: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct IpLimiter {
    inner: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

impl IpLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when this IP is currently over the failure limit (should be blocked).
    pub fn is_blocked(&self, ip: IpAddr) -> bool {
        let mut map = match self.inner.lock() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        if let Some(hits) = map.get_mut(&ip) {
            let cutoff = Instant::now() - IP_WINDOW;
            hits.retain(|t| *t >= cutoff);
            hits.len() >= IP_MAX_FAILURES
        } else {
            false
        }
    }

    /// Record one failed attempt from this IP.
    pub fn record_failure(&self, ip: IpAddr) {
        let mut map = match self.inner.lock() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        let now = Instant::now();
        let cutoff = now - IP_WINDOW;
        let hits = map.entry(ip).or_default();
        hits.retain(|t| *t >= cutoff);
        hits.push(now);
        // Opportunistic cleanup so the map doesn't grow unbounded.
        if map.len() > 10_000 {
            map.retain(|_, v| v.iter().any(|t| *t >= cutoff));
        }
    }

    /// Clear this IP's record (on a successful login).
    pub fn clear(&self, ip: IpAddr) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(&ip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_threshold() {
        let lim = IpLimiter::new();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(!lim.is_blocked(ip));
        for _ in 0..IP_MAX_FAILURES {
            lim.record_failure(ip);
        }
        assert!(lim.is_blocked(ip));
        lim.clear(ip);
        assert!(!lim.is_blocked(ip));
    }

    #[test]
    fn distinct_ips_are_independent() {
        let lim = IpLimiter::new();
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        for _ in 0..IP_MAX_FAILURES {
            lim.record_failure(a);
        }
        assert!(lim.is_blocked(a));
        assert!(!lim.is_blocked(b));
    }
}
