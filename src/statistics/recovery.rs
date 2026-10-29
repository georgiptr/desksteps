//! Counter-based catch-up estimates. Raw sample quality remains unchanged.
use crate::protocol::Sample;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Recovery {
    pub steps: i64,
    pub active_ms: i64,
    /// Active kcal per kg, or unknown when distance/time cannot support it.
    pub factor: Option<f64>,
}
impl Recovery {
    pub fn any(self) -> bool {
        self.steps > 0 || self.active_ms > 0
    }
}

pub(crate) fn estimate(old: &Sample, new: &Sample, wall_ms: i64) -> Recovery {
    let decreases = old.steps.zip(new.steps).is_some_and(|(a, b)| b < a)
        || old.elapsed_s.zip(new.elapsed_s).is_some_and(|(a, b)| b < a)
        || old
            .distance_m
            .zip(new.distance_m)
            .is_some_and(|(a, b)| b < a);
    if wall_ms <= 0 || decreases {
        return Recovery::default();
    }
    let steps = old.steps.zip(new.steps).map(|(a, b)| i64::from(b - a));
    let distance = old.distance_m.zip(new.distance_m).map(|(a, b)| b - a);
    let active_ms = old
        .elapsed_s
        .zip(new.elapsed_s)
        .map(|(a, b)| i64::from(b - a) * 1000)
        .filter(|t| *t > 0 && *t <= wall_ms.saturating_add(2000))
        // Known empty-belt progress must not become walking time. Either
        // positive counter can establish activity when the other is absent.
        .filter(|_| steps.is_some_and(|s| s > 0) || distance.is_some_and(|d| d > 0))
        .unwrap_or(0);
    let factor = distance
        .filter(|d| *d > 0)
        .filter(|_| active_ms > 0)
        .and_then(|d| super::met(f64::from(d) * 3600.0 / active_ms as f64))
        .map(|met| (met - 1.0) * 3.5 / 200.0 * active_ms as f64 / 60_000.0);
    Recovery {
        steps: steps.unwrap_or(0),
        active_ms,
        factor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counter_estimates_reject_nonpositive_time_and_unsupported_speed() {
        let old = Sample {
            steps: Some(10),
            elapsed_s: Some(10),
            distance_m: Some(10),
            ..Default::default()
        };
        let new = Sample {
            steps: Some(20),
            elapsed_s: Some(20),
            distance_m: Some(110),
            ..Default::default()
        };
        for wall in [-1, 0] {
            assert_eq!(estimate(&old, &new, wall), Recovery::default());
        }
        let r = estimate(&old, &new, 10000);
        assert_eq!((r.steps, r.active_ms, r.factor), (10, 10000, None));
        assert_eq!(
            estimate(&Sample::default(), &Sample::default(), 10000),
            Recovery::default()
        );
    }
}
