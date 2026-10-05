use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

pub const LOGIN_MAX_ATTEMPTS: u32 = 10;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);
pub const REGISTER_MAX_ATTEMPTS: u32 = 5;
pub const REGISTER_WINDOW: Duration = Duration::from_secs(60 * 60);
/// How often expired entries are evicted so the map cannot grow without bound.
pub const PRUNE_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Fixed-window, in-memory attempt counter keyed by client IP.
pub struct RateLimiter {
    attempts: DashMap<IpAddr, (u32, Instant)>,
    max_attempts: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_attempts: u32, window: Duration) -> Self {
        Self {
            attempts: DashMap::new(),
            max_attempts,
            window,
        }
    }

    /// Records an attempt. Returns `false` if the IP has exhausted its window.
    pub fn check(&self, ip: IpAddr) -> bool {
        self.check_at(ip, Instant::now())
    }

    fn check_at(&self, ip: IpAddr, now: Instant) -> bool {
        let mut entry = self.attempts.entry(ip).or_insert((0, now));
        let (count, window_start) = entry.value_mut();
        if now.duration_since(*window_start) >= self.window {
            *count = 0;
            *window_start = now;
        }
        if *count >= self.max_attempts {
            return false;
        }
        *count += 1;
        true
    }

    pub fn prune(&self) {
        self.prune_at(Instant::now());
    }

    fn prune_at(&self, now: Instant) {
        self.attempts
            .retain(|_, (_, window_start)| now.duration_since(*window_start) < self.window);
    }
}

/// Periodically evicts expired entries from every limiter.
pub fn spawn_pruner(limiters: Vec<Arc<RateLimiter>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(PRUNE_INTERVAL);
        loop {
            interval.tick().await;
            for limiter in &limiters {
                limiter.prune();
            }
            tracing::debug!("rate limiter entries pruned");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: u32 = 3;
    const WINDOW: Duration = Duration::from_secs(60);

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn blocks_after_max_attempts_per_ip() {
        let limiter = RateLimiter::new(MAX, WINDOW);
        let now = Instant::now();
        for _ in 0..MAX {
            assert!(limiter.check_at(ip(1), now));
        }
        assert!(!limiter.check_at(ip(1), now));
        assert!(limiter.check_at(ip(2), now), "other IPs are unaffected");
    }

    #[test]
    fn resets_after_window_elapses() {
        let limiter = RateLimiter::new(MAX, WINDOW);
        let start = Instant::now();
        for _ in 0..MAX {
            limiter.check_at(ip(1), start);
        }
        assert!(!limiter.check_at(ip(1), start + WINDOW - Duration::from_secs(1)));
        assert!(limiter.check_at(ip(1), start + WINDOW));
    }

    #[test]
    fn prune_removes_only_expired_entries() {
        let limiter = RateLimiter::new(MAX, WINDOW);
        let start = Instant::now();
        limiter.check_at(ip(1), start);
        limiter.check_at(ip(2), start + WINDOW);
        limiter.prune_at(start + WINDOW);
        assert!(!limiter.attempts.contains_key(&ip(1)));
        assert!(limiter.attempts.contains_key(&ip(2)));
    }
}
