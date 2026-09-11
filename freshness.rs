use chrono::{DateTime, Utc};

use crate::health::scorer::{FreshnessThresholds, VenueScorerInput, VenueType};

// ---------------------------------------------------------------------------
// FreshnessResult
// ---------------------------------------------------------------------------

/// Per-input freshness classification and measured staleness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshnessResult {
    pub is_fresh: bool,
    pub staleness_secs: u64,
}

// ---------------------------------------------------------------------------
// FreshnessOutcome
// ---------------------------------------------------------------------------

/// Aggregate result of evaluating a slice of `VenueScorerInput`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshnessOutcome {
    /// Indices (into the original slice) of fresh inputs.
    pub fresh: Vec<usize>,
    /// Indices (into the original slice) of stale inputs.
    pub stale: Vec<usize>,
    /// Maximum staleness in seconds observed across all evaluated inputs.
    pub max_staleness_secs: u64,
}

// ---------------------------------------------------------------------------
// FreshnessGuard
// ---------------------------------------------------------------------------

pub struct FreshnessGuard;

impl FreshnessGuard {
    /// Evaluate freshness for every input in `inputs`.
    ///
    /// Each input is classified against the per-venue-type threshold in
    /// `thresholds`.  A missing `last_updated_at` is treated as stale
    /// (Requirement 2.5).
    pub fn evaluate(
        inputs: &[VenueScorerInput],
        thresholds: &FreshnessThresholds,
        now: DateTime<Utc>,
    ) -> FreshnessOutcome {
        let mut fresh = Vec::new();
        let mut stale = Vec::new();
        let mut max_staleness_secs: u64 = 0;

        for (idx, input) in inputs.iter().enumerate() {
            let result = Self::classify(input, thresholds, now);
            if result.staleness_secs > max_staleness_secs {
                max_staleness_secs = result.staleness_secs;
            }
            if result.is_fresh {
                fresh.push(idx);
            } else {
                stale.push(idx);
            }
        }

        FreshnessOutcome {
            fresh,
            stale,
            max_staleness_secs,
        }
    }

    /// Classify a single input.
    fn classify(
        input: &VenueScorerInput,
        thresholds: &FreshnessThresholds,
        now: DateTime<Utc>,
    ) -> FreshnessResult {
        let threshold = match input.venue_type {
            VenueType::Sdex => thresholds.sdex,
            VenueType::Amm => thresholds.amm,
        };

        match input.last_updated_at {
            None => FreshnessResult {
                is_fresh: false,
                // Use a sentinel value that clearly signals "unknown age"
                staleness_secs: u64::MAX,
            },
            Some(ts) => {
                // Use ceil-to-seconds to avoid classifying 60.1s as 60s due truncation.
                let staleness_ms = (now - ts).num_milliseconds();
                let staleness_secs = if staleness_ms <= 0 {
                    0
                } else {
                    ((staleness_ms + 999) / 1000) as u64
                };
                FreshnessResult {
                    is_fresh: staleness_secs <= threshold,
                    staleness_secs,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn thresholds() -> FreshnessThresholds {
        FreshnessThresholds { sdex: 30, amm: 60 }
    }

    fn make_sdex(last_updated_at: Option<DateTime<Utc>>) -> VenueScorerInput {
        VenueScorerInput {
            venue_ref: "sdex:XLM/USDC".to_string(),
            venue_type: VenueType::Sdex,
            best_bid_e7: Some(9_990_000),
            best_ask_e7: Some(10_010_000),
            depth_top_n_e7: Some(5_000_000_000),
            reserve_a_e7: None,
            reserve_b_e7: None,
            tvl_e7: None,
            last_updated_at,
        }
    }

    fn make_amm(last_updated_at: Option<DateTime<Utc>>) -> VenueScorerInput {
        VenueScorerInput {
            venue_ref: "amm:XLM/USDC".to_string(),
            venue_type: VenueType::Amm,
            best_bid_e7: None,
            best_ask_e7: None,
            depth_top_n_e7: None,
            reserve_a_e7: Some(1_000_000_000),
            reserve_b_e7: Some(1_000_000_000),
            tvl_e7: Some(2_000_000_000),
            last_updated_at,
        }
    }

    // --- Unit tests for FreshnessGuard::evaluate (Task 2.2) ---

    #[test]
    fn all_fresh_inputs() {
        let now = Utc::now();
        let inputs = vec![
            make_sdex(Some(now - Duration::seconds(10))),
            make_amm(Some(now - Duration::seconds(30))),
        ];
        let outcome = FreshnessGuard::evaluate(&inputs, &thresholds(), now);
        assert_eq!(outcome.fresh, vec![0, 1]);
        assert!(outcome.stale.is_empty());
        assert_eq!(outcome.max_staleness_secs, 30);
    }

    #[test]
    fn all_stale_inputs() {
        let now = Utc::now();
        let inputs = vec![
            make_sdex(Some(now - Duration::seconds(60))), // > 30s threshold
            make_amm(Some(now - Duration::seconds(120))), // > 60s threshold
        ];
        let outcome = FreshnessGuard::evaluate(&inputs, &thresholds(), now);
        assert!(outcome.fresh.is_empty());
        assert_eq!(outcome.stale, vec![0, 1]);
        assert_eq!(outcome.max_staleness_secs, 120);
    }

    #[test]
    fn mixed_fresh_and_stale() {
        let now = Utc::now();
        let inputs = vec![
            make_sdex(Some(now - Duration::seconds(10))), // fresh (≤ 30s)
            make_amm(Some(now - Duration::seconds(90))),  // stale (> 60s)
        ];
        let outcome = FreshnessGuard::evaluate(&inputs, &thresholds(), now);
        assert_eq!(outcome.fresh, vec![0]);
        assert_eq!(outcome.stale, vec![1]);
        assert_eq!(outcome.max_staleness_secs, 90);
    }

    #[test]
    fn missing_timestamp_treated_as_stale() {
        let now = Utc::now();
        let inputs = vec![make_sdex(None)];
        let outcome = FreshnessGuard::evaluate(&inputs, &thresholds(), now);
        assert!(outcome.fresh.is_empty());
        assert_eq!(outcome.stale, vec![0]);
        assert_eq!(outcome.max_staleness_secs, u64::MAX);
    }

    #[test]
    fn sdex_and_amm_thresholds_applied_independently() {
        let now = Utc::now();
        // 45s: stale for SDEX (threshold 30), fresh for AMM (threshold 60)
        let inputs = vec![
            make_sdex(Some(now - Duration::seconds(45))),
            make_amm(Some(now - Duration::seconds(45))),
        ];
        let outcome = FreshnessGuard::evaluate(&inputs, &thresholds(), now);
        assert_eq!(outcome.fresh, vec![1], "AMM at 45s should be fresh");
        assert_eq!(outcome.stale, vec![0], "SDEX at 45s should be stale");
    }

    #[test]
    fn exactly_at_threshold_is_fresh() {
        let now = Utc::now();
        // Exactly at threshold → fresh (≤ threshold)
        let inputs = vec![
            make_sdex(Some(now - Duration::seconds(30))),
            make_amm(Some(now - Duration::seconds(60))),
        ];
        let outcome = FreshnessGuard::evaluate(&inputs, &thresholds(), now);
        assert_eq!(outcome.fresh, vec![0, 1]);
        assert!(outcome.stale.is_empty());
    }

    #[test]
    fn one_over_threshold_is_stale() {
        let now = Utc::now();
        let inputs = vec![
            make_sdex(Some(now - Duration::seconds(31))),
            make_amm(Some(now - Duration::seconds(61))),
        ];
        let outcome = FreshnessGuard::evaluate(&inputs, &thresholds(), now);
        assert!(outcome.fresh.is_empty());
        assert_eq!(outcome.stale, vec![0, 1]);
    }

    #[test]
    fn empty_inputs_returns_empty_outcome() {
        let now = Utc::now();
        let outcome = FreshnessGuard::evaluate(&[], &thresholds(), now);
        assert!(outcome.fresh.is_empty());
        assert!(outcome.stale.is_empty());
        assert_eq!(outcome.max_staleness_secs, 0);
    }
}
