//! Round-robin and fill-first account/provider load balancing.

use std::sync::atomic::{AtomicU64, Ordering};

/// Load-balancing strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Distribute requests evenly across candidates.
    RoundRobin,
    /// Exhaust first candidate before moving to next.
    FillFirst,
}

impl Strategy {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "fill-first" | "fill_first" | "fillfirst" => Self::FillFirst,
            _ => Self::RoundRobin,
        }
    }
}

/// Per-candidate usage counter.
#[derive(Debug)]
struct CandidateCounter {
    requests: AtomicU64,
}

/// Thread-safe load balancer that picks the next candidate index.
#[derive(Debug)]
pub struct Balancer {
    strategy: Strategy,
    /// Global round-robin counter (wraps around)
    rr_counter: AtomicU64,
    /// Per-candidate request counters for tracking
    counters: Vec<CandidateCounter>,
}

impl Balancer {
    /// Create a balancer for `n` candidates.
    pub fn new(strategy: Strategy, n: usize) -> Self {
        let counters = (0..n)
            .map(|_| CandidateCounter {
                requests: AtomicU64::new(0),
            })
            .collect();

        Self {
            strategy,
            rr_counter: AtomicU64::new(0),
            counters,
        }
    }

    /// Pick the next candidate index from the given candidate list.
    ///
    /// `candidates` is a slice of indices into the provider list.
    /// Returns the chosen index (from `candidates`).
    pub fn pick(&self, candidates: &[usize]) -> usize {
        if candidates.is_empty() {
            return 0;
        }
        if candidates.len() == 1 {
            self.increment(candidates[0]);
            return candidates[0];
        }

        let chosen = match self.strategy {
            Strategy::RoundRobin => {
                let tick = self.rr_counter.fetch_add(1, Ordering::Relaxed);
                let pos = (tick as usize) % candidates.len();
                candidates[pos]
            }
            Strategy::FillFirst => {
                // Always pick the first candidate
                candidates[0]
            }
        };

        self.increment(chosen);
        chosen
    }

    fn increment(&self, idx: usize) {
        if idx < self.counters.len() {
            self.counters[idx].requests.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get the request count for a candidate.
    pub fn request_count(&self, idx: usize) -> u64 {
        self.counters
            .get(idx)
            .map(|c| c.requests.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Total requests across all candidates.
    pub fn total_requests(&self) -> u64 {
        self.counters
            .iter()
            .map(|c| c.requests.load(Ordering::Relaxed))
            .sum()
    }
}
