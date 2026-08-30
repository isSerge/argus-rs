//! Adaptive polling cadence for the block ingestor.
//!
//! Three pieces of chain awareness sit here so `block_ingestor` stays
//! focused on fetching and handing blocks downstream:
//!
//! - `live_poll_interval` picks the steady-state poll cadence, tracking the
//!   chain's expected block time when configured;
//! - `BlockTimeCalibrator` samples chain head progression and warns when the
//!   observed block time diverges from the configured expectation, which
//!   catches a misconfigured chain.

use std::time::Duration;

use argus_core::config::AppConfig;

/// Floor for the live poll interval derived from `expected_block_time_ms`.
pub(crate) const MIN_LIVE_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Minimum head advancement before the block-time calibrator judges the
/// observed rate, smoothing out per-block timing jitter.
const CALIBRATION_MIN_BLOCKS: u64 = 4;

/// Warn threshold: the observed block time must differ from the configured
/// expectation by more than this factor to trigger a warning.
const CALIBRATION_DIVERGENCE_FACTOR: u32 = 2;

/// Interval between polls once caught up.
///
/// When `expected_block_time_ms` is configured the cadence tracks the chain
/// so live alert latency stays ~one block: the value is clamped to a floor
/// (so absurdly small values don't hammer the RPC) and to
/// `polling_interval_ms` (so the configured ceiling is respected). Falls back
/// to `polling_interval_ms` when unset.
pub(crate) fn live_poll_interval(config: &AppConfig) -> Duration {
    match config.expected_block_time_ms {
        Some(block_time) => block_time.max(MIN_LIVE_POLL_INTERVAL).min(config.polling_interval_ms),
        None => config.polling_interval_ms,
    }
}

/// Samples chain head progression to estimate the observed block time and
/// warns (once) when it diverges significantly from the configured
/// `expected_block_time_ms`, which catches a misconfigured chain.
pub(crate) struct BlockTimeCalibrator {
    expected: Option<Duration>,
    sample: Option<(std::time::Instant, u64)>,
    warned: bool,
}

impl BlockTimeCalibrator {
    pub(crate) fn new(expected: Option<Duration>) -> Self {
        Self { expected, sample: None, warned: false }
    }

    pub(crate) fn observe(&mut self, now: std::time::Instant, head: u64) {
        let Some(expected) = self.expected else { return };

        let Some((t0, h0)) = self.sample else {
            self.sample = Some((now, head));
            return;
        };

        let delta = head.saturating_sub(h0);
        if delta < CALIBRATION_MIN_BLOCKS {
            return;
        }

        let observed = now.saturating_duration_since(t0) / delta as u32;
        if !self.warned
            && (observed * CALIBRATION_DIVERGENCE_FACTOR < expected
                || observed > expected * CALIBRATION_DIVERGENCE_FACTOR)
        {
            tracing::warn!(
                expected_ms = expected.as_millis() as u64,
                observed_ms = observed.as_millis() as u64,
                "Observed block time diverges from expected_block_time_ms; check the chain \
                 configuration."
            );
            self.warned = true;
        }
        self.sample = Some((now, head));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use argus_core::{config::AppConfig, models::NetworkId};

    use super::*;

    #[test]
    fn test_live_poll_interval_tracks_expected_block_time() {
        let base =
            || AppConfig::builder().network_id(&NetworkId::default()).polling_interval(10_000);

        assert_eq!(live_poll_interval(&base().build()), Duration::from_millis(10_000));

        let fast = base().expected_block_time(2_000).build();
        assert_eq!(live_poll_interval(&fast), Duration::from_millis(2_000));

        let below_floor = base().expected_block_time(100).build();
        assert_eq!(live_poll_interval(&below_floor), MIN_LIVE_POLL_INTERVAL);

        let above_polling = base().expected_block_time(30_000).build();
        assert_eq!(live_poll_interval(&above_polling), Duration::from_millis(10_000));
    }

    #[test]
    fn test_block_time_calibrator_warns_on_divergence() {
        let mut calibrator = BlockTimeCalibrator::new(Some(Duration::from_secs(2)));
        let t0 = Instant::now();

        calibrator.observe(t0, 100);
        // Only 2 blocks advanced: below the calibration threshold, so the
        // sample window keeps accumulating.
        calibrator.observe(t0 + Duration::from_secs(100), 102);
        assert!(!calibrator.warned);

        // 6 blocks over 120s = 20s observed vs 2s expected.
        calibrator.observe(t0 + Duration::from_secs(120), 106);
        assert!(calibrator.warned);
    }

    #[test]
    fn test_block_time_calibrator_silent_when_matching() {
        let mut calibrator = BlockTimeCalibrator::new(Some(Duration::from_secs(2)));
        let t0 = Instant::now();

        calibrator.observe(t0, 100);
        // 4 blocks in 8s = 2s observed, matching the expectation.
        calibrator.observe(t0 + Duration::from_secs(8), 104);
        assert!(!calibrator.warned);
    }

    #[test]
    fn test_block_time_calibrator_inactive_without_expectation() {
        let mut calibrator = BlockTimeCalibrator::new(None);
        let t0 = Instant::now();

        calibrator.observe(t0, 100);
        calibrator.observe(t0 + Duration::from_secs(1000), 104);
        assert!(!calibrator.warned);
    }
}
