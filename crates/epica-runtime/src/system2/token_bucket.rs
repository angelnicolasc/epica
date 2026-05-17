//! Rate-limiter for System 2 LLM calls.
//!
//! Uses a continuous-refill token bucket: tokens accumulate at
//! `refill_rate_per_sec` per second up to `capacity`, so a short burst can
//! consume many tokens while long idle periods restore full capacity.

use std::time::Instant;

/// Token bucket that rate-limits System 2 activations.
///
/// System 2 (LLM reflection call) has real cost. Without a budget, a
/// low-confidence belief cascade could trigger thousands of reflection calls
/// per session. The bucket starts full, drains on consumption, and refills
/// continuously based on wall-clock elapsed time.
pub struct TokenBucket {
    capacity: u32,
    /// Fractional available tokens — `f32` so sub-second refill accumulates
    /// correctly across rapid successive calls.
    available: f32,
    refill_rate_per_sec: f32,
    last_refill: Instant,
}

impl TokenBucket {
    /// Create a bucket with `capacity` tokens, refilled at `refill_rate_per_sec`
    /// tokens per second. Starts full.
    pub fn new(capacity: u32, refill_rate_per_sec: f32) -> Self {
        Self {
            capacity,
            available: capacity as f32,
            refill_rate_per_sec,
            last_refill: Instant::now(),
        }
    }

    /// Try to consume `tokens`. Returns `true` and deducts them if available;
    /// returns `false` without mutation if the bucket does not have enough.
    pub fn try_consume(&mut self, tokens: u32) -> bool {
        self.refill();
        if self.available >= tokens as f32 {
            self.available -= tokens as f32;
            true
        } else {
            false
        }
    }

    /// Return `tokens` to the bucket (refund), capped at capacity.
    ///
    /// Called when a System 2 LLM call fails after budget was already consumed,
    /// so that a transient network error does not permanently drain the session budget.
    pub fn release(&mut self, tokens: u32) {
        self.available = (self.available + tokens as f32).min(self.capacity as f32);
    }

    /// Available token count (floor, since fractional tokens cannot be consumed).
    pub fn available(&self) -> u32 {
        self.available as u32
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    // ── Private ──────────────────────────────────────────────────────────────

    fn refill(&mut self) {
        if self.refill_rate_per_sec <= 0.0 {
            return;
        }
        let elapsed = self.last_refill.elapsed().as_secs_f32();
        self.available = (self.available + elapsed * self.refill_rate_per_sec)
            .min(self.capacity as f32);
        self.last_refill = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_full() {
        let b = TokenBucket::new(10, 1.0);
        assert_eq!(b.available(), 10);
    }

    #[test]
    fn consume_reduces_available() {
        let mut b = TokenBucket::new(10, 0.0);
        assert!(b.try_consume(3));
        assert_eq!(b.available(), 7);
    }

    #[test]
    fn consume_fails_when_empty() {
        let mut b = TokenBucket::new(2, 0.0);
        assert!(b.try_consume(2));
        assert!(!b.try_consume(1));
    }

    #[test]
    fn zero_refill_rate_never_refills() {
        let mut b = TokenBucket::new(1, 0.0);
        assert!(b.try_consume(1));
        // Even after refill() is called with elapsed > 0, no tokens come back
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!b.try_consume(1));
    }
}
