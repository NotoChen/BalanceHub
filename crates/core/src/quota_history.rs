//! Short quota-reading series shared by OAuth accounts and relay subscriptions.
//!
//! Providers serve only the current value, so a trend can only come from what
//! BalanceHub itself observed. Both account types keep the same bounded series
//! and the same deduplication rule; only the window identity differs.

use crate::{limits, models::AccountQuotaSample};

/// Append one reading unless it repeats the previous value.
///
/// A reading without a finite percentage is skipped; it must not break the
/// series or be written as zero. The oldest readings are dropped once the
/// window reaches [`limits::QUOTA_HISTORY_SAMPLES`].
pub fn push_sample(history: &mut Vec<AccountQuotaSample>, percent: Option<f64>, at: i64) {
    let Some(percent) = percent.filter(|value| value.is_finite()) else {
        return;
    };
    let unchanged = history.last().is_some_and(|sample| {
        (sample.used_percent - percent).abs() < limits::QUOTA_HISTORY_EPSILON
    });
    if unchanged {
        return;
    }
    history.push(AccountQuotaSample {
        at,
        used_percent: percent,
    });
    if history.len() > limits::QUOTA_HISTORY_SAMPLES {
        let excess = history.len() - limits::QUOTA_HISTORY_SAMPLES;
        history.drain(..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::push_sample;
    use crate::limits;

    #[test]
    fn repeated_and_missing_readings_do_not_grow_the_series() {
        let mut history = Vec::new();
        push_sample(&mut history, Some(10.0), 1);
        push_sample(&mut history, Some(10.04), 2);
        push_sample(&mut history, None, 3);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].used_percent, 10.0);
    }

    #[test]
    fn series_keeps_only_the_newest_readings() {
        let mut history = Vec::new();
        for index in 0..(limits::QUOTA_HISTORY_SAMPLES + 5) {
            push_sample(&mut history, Some(index as f64), index as i64);
        }
        assert_eq!(history.len(), limits::QUOTA_HISTORY_SAMPLES);
        assert_eq!(history[0].used_percent, 5.0);
        assert_eq!(
            history.last().unwrap().used_percent,
            (limits::QUOTA_HISTORY_SAMPLES + 4) as f64
        );
    }
}
