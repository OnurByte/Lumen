//! Bounded, time-limited duplicate suppression (reconnects often replay recent messages).
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

pub struct Deduplicator {
    seen: HashMap<String, Instant>,
    order: VecDeque<(String, Instant)>,
    capacity: usize,
    ttl: Duration,
}

impl Deduplicator {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self { seen: HashMap::with_capacity(capacity.min(65536)), order: VecDeque::new(), capacity: capacity.max(1), ttl }
    }
    /// Returns `true` the first time a key is seen within the TTL window.
    pub fn first_seen(&mut self, key: &str, now: Instant) -> bool {
        self.evict(now);
        if self.seen.contains_key(key) {
            return false;
        }
        while self.seen.len() >= self.capacity {
            let Some((old, _)) = self.order.pop_front() else { break };
            self.seen.remove(&old);
        }
        self.seen.insert(key.to_owned(), now);
        self.order.push_back((key.to_owned(), now));
        true
    }
    fn evict(&mut self, now: Instant) {
        while let Some((key, at)) = self.order.front() {
            if now.duration_since(*at) <= self.ttl {
                break;
            }
            if self.seen.get(key) == Some(at) {
                self.seen.remove(key);
            }
            self.order.pop_front();
        }
    }
    pub fn len(&self) -> usize {
        self.seen.len()
    }
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suppresses_within_ttl_and_respects_capacity() {
        let t0 = Instant::now();
        let mut d = Deduplicator::new(2, Duration::from_secs(10));
        assert!(d.first_seen("a", t0));
        assert!(!d.first_seen("a", t0));
        assert!(d.first_seen("b", t0));
        assert!(d.first_seen("c", t0)); // evicts "a"
        assert_eq!(d.len(), 2);
        assert!(d.first_seen("a", t0));
        let later = t0 + Duration::from_secs(30);
        assert!(d.first_seen("c", later));
    }
}
