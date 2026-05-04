//! Circuit breaker — per-route-granularity failure tracking with half-open recovery.
//!
//! Each route target `(model, provider, target_model)` is an independent circuit.
//! After N consecutive failures, the circuit opens (blocks requests) for a cooldown period.
//! After cooldown, the circuit enters half-open: one request is allowed through.
//! If that request succeeds, the circuit resets. If it fails, the circuit opens again.
//!
//! # State machine
//!
//! ```text
//! Closed ──(N failures)──→ Open ──(cooldown expires)──→ HalfOpen
//!   ↑                                                    │
//!   └──────────────(success)─────────────────────────────┘
//!                                                     │
//!                          (failure) ──→ Open ←───────┘
//! ```

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The state of a single circuit.
#[derive(Debug, Clone, PartialEq)]
enum CircuitState {
    /// Normal operation — requests pass through.
    Closed,
    /// Blocked — too many consecutive failures. `since` tracks when the circuit opened.
    Open { since: Instant },
    /// One request allowed through to test recovery.
    HalfOpen,
}

/// Tracks failure count and state for a single circuit key.
#[derive(Debug)]
struct Circuit {
    state: CircuitState,
    consecutive_failures: u32,
}

/// Configuration for circuit breaker behavior.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening the circuit.
    pub failure_threshold: u32,
    /// How long to wait in Open state before transitioning to HalfOpen.
    pub cooldown: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            cooldown: Duration::from_secs(30),
        }
    }
}

/// Thread-safe circuit breaker with per-key granularity.
pub struct CircuitBreaker {
    circuits: Mutex<HashMap<String, Circuit>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            circuits: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// Check if the circuit for `key` is open (blocked).
    ///
    /// Returns `true` if requests should be rejected. If the circuit is Open
    /// and the cooldown has elapsed, transitions to HalfOpen and returns `false`.
    pub fn is_open(&self, key: &str) -> bool {
        let mut circuits = self.circuits.lock().unwrap();
        let circuit = circuits.entry(key.to_string()).or_insert(Circuit {
            state: CircuitState::Closed,
            consecutive_failures: 0,
        });

        match &circuit.state {
            CircuitState::Closed => false,
            CircuitState::Open { since } => {
                if since.elapsed() >= self.config.cooldown {
                    circuit.state = CircuitState::HalfOpen;
                    false
                } else {
                    true
                }
            }
            CircuitState::HalfOpen => false,
        }
    }

    /// Record a successful request. Resets the circuit to Closed.
    pub fn record_success(&self, key: &str) {
        let mut circuits = self.circuits.lock().unwrap();
        if let Some(circuit) = circuits.get_mut(key) {
            circuit.state = CircuitState::Closed;
            circuit.consecutive_failures = 0;
        }
    }

    /// Record a failed request. May transition to Open if threshold is reached.
    pub fn record_failure(&self, key: &str) {
        let mut circuits = self.circuits.lock().unwrap();
        let circuit = circuits.entry(key.to_string()).or_insert(Circuit {
            state: CircuitState::Closed,
            consecutive_failures: 0,
        });

        circuit.consecutive_failures += 1;
        if circuit.consecutive_failures >= self.config.failure_threshold {
            circuit.state = CircuitState::Open { since: Instant::now() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t31_consecutive_failures_opens_circuit() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            cooldown: Duration::from_secs(30),
        });

        assert!(!cb.is_open("route-a"));

        cb.record_failure("route-a");
        cb.record_failure("route-a");
        assert!(!cb.is_open("route-a")); // not yet

        cb.record_failure("route-a");
        assert!(cb.is_open("route-a")); // now open
    }

    #[test]
    fn t32_cooldown_enters_half_open() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            cooldown: Duration::from_millis(50),
        });

        cb.record_failure("route-b");
        assert!(cb.is_open("route-b"));

        // Wait for cooldown
        std::thread::sleep(Duration::from_millis(60));
        // Should transition to half-open and allow through
        assert!(!cb.is_open("route-b"));
    }

    #[test]
    fn t33_half_open_success_resets() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            cooldown: Duration::from_millis(50),
        });

        cb.record_failure("route-c");
        cb.record_failure("route-c");
        assert!(cb.is_open("route-c"));

        std::thread::sleep(Duration::from_millis(60));
        assert!(!cb.is_open("route-c")); // half-open, allows through

        cb.record_success("route-c");
        // Fully reset — need threshold (2) failures to open again
        cb.record_failure("route-c");
        assert!(!cb.is_open("route-c")); // only 1 failure, threshold is 2
        cb.record_failure("route-c");
        assert!(cb.is_open("route-c")); // 2 failures = threshold, now open
    }

    #[test]
    fn t34_half_open_failure_reopens() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            cooldown: Duration::from_millis(50),
        });

        cb.record_failure("route-d");
        assert!(cb.is_open("route-d"));

        std::thread::sleep(Duration::from_millis(60));
        assert!(!cb.is_open("route-d")); // half-open

        cb.record_failure("route-d"); // fails during half-open
        assert!(cb.is_open("route-d")); // back to open
    }

    #[test]
    fn t35_different_routes_independent() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            cooldown: Duration::from_secs(30),
        });

        cb.record_failure("route-e");
        assert!(cb.is_open("route-e"));
        assert!(!cb.is_open("route-f")); // independent
    }
}
