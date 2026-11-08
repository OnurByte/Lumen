//! Token bucket used for per-account ingress and per-user moderation limits.
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(capacity: u32, refill_per_sec: f64, now: Instant) -> Self {
        Self { capacity: f64::from(capacity.max(1)), tokens: f64::from(capacity.max(1)), refill_per_sec: refill_per_sec.max(0.0), last: now }
    }
    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        self.last = now;
    }
    pub fn try_take(&mut self, now: Instant) -> bool {
        self.refill(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
    /// Seconds until one token is available (0 if available now).
    pub fn wait_secs(&mut self, now: Instant) -> f64 {
        self.refill(now);
        if self.tokens >= 1.0 || self.refill_per_sec == 0.0 { 0.0 } else { (1.0 - self.tokens) / self.refill_per_sec }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn bucket_limits_and_refills() {
        let t = Instant::now();
        let mut b = TokenBucket::new(2, 1.0, t);
        assert!(b.try_take(t));
        assert!(b.try_take(t));
        assert!(!b.try_take(t));
        assert!(b.try_take(t + Duration::from_millis(1001)));
    }
}
