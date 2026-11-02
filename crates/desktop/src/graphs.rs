use std::collections::VecDeque;

pub const WINDOW_NS: u64 = 120_000_000_000;

/// Zero-based axes with modest headroom and stable, readable upper bounds.
/// Keep every visible peak; changing the scale never changes the samples.
pub fn utilization_ceiling(values: &VecDeque<(u64, f64)>, now: u64, auto: bool) -> f64 {
    if !auto {
        return 100.0;
    }
    let peak = values
        .iter()
        .filter(|(time, value)| {
            *time <= now && now - time <= WINDOW_NS && value.is_finite() && *value >= 0.0
        })
        .map(|(_, value)| *value)
        .fold(0.0, f64::max);
    let desired = (peak * 1.1).clamp(1.0, 100.0);
    [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0]
        .into_iter()
        .find(|limit| *limit >= desired)
        .unwrap_or(100.0)
}

pub fn tick_label(value: f64) -> String {
    format!("{value:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn low_usage_expands_without_hiding_visible_peaks() {
        let values = VecDeque::from([(1, 0.1), (2, 0.7)]);
        assert_eq!(utilization_ceiling(&values, 2, true), 1.0);
        assert_eq!(utilization_ceiling(&values, 2, false), 100.0);
        for peak in [0.0, 0.9, 1.0, 4.0, 19.0, 50.0, 99.0, 100.0] {
            let upper = utilization_ceiling(&VecDeque::from([(1, peak)]), 1, true);
            assert!(upper >= peak && (1.0..=100.0).contains(&upper));
        }
    }
    #[test]
    fn ignores_old_future_and_invalid_samples_but_retains_in_window_spikes() {
        let now = WINDOW_NS + 100;
        let mut values = VecDeque::from([
            (0, 95.0),
            (now + 1, 99.0),
            (now - 3, f64::NAN),
            (now - 2, f64::INFINITY),
            (now - 1, -5.0),
            (now, 0.5),
        ]);
        assert_eq!(utilization_ceiling(&values, now, true), 1.0);
        values.push_front((100, 80.0));
        assert_eq!(utilization_ceiling(&values, now, true), 100.0);
        assert_eq!(utilization_ceiling(&VecDeque::new(), now, true), 1.0);
    }
    #[test]
    fn fractional_axis_labels_remain_distinct() {
        let labels: Vec<_> = (0..=4).map(|i| tick_label(i as f64 / 4.0)).collect();
        assert_eq!(labels, ["0", "0.25", "0.5", "0.75", "1"]);
        assert_eq!(tick_label(100.0), "100");
    }
}
