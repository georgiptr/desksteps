//! Conservative step accounting. No I/O; uncertain deltas are evidence, not credit.
use crate::protocol::Sample;
use chrono::{DateTime, Utc};

pub(crate) struct Anchor {
    pub connection: String,
    pub utc: DateTime<Utc>,
    pub monotonic_ns: Option<i64>,
    pub steps: i64,
    pub elapsed_s: Option<i64>,
    pub day: String,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Decision {
    pub delta: i64,
    pub quality: &'static str,
    pub update_anchor: bool,
}

pub(crate) fn decide(
    sample: &Sample,
    connection: &str,
    utc: DateTime<Utc>,
    monotonic_ns: Option<i64>,
    day: &str,
    prior: Option<&Anchor>,
) -> Decision {
    let Some(steps) = sample.steps else {
        return Decision {
            delta: 0,
            quality: "missing_steps",
            update_anchor: false,
        };
    };
    let Some(old) = prior else {
        return Decision {
            delta: steps.into(),
            quality: "initial_baseline",
            update_anchor: true,
        };
    };
    let wall_gap = (utc - old.utc).num_milliseconds();
    let mono_gap = monotonic_ns
        .zip(old.monotonic_ns)
        .map(|(new, old)| (new - old) / 1_000_000);
    if wall_gap < 0 || (connection == old.connection && mono_gap.is_some_and(|g| g < 0)) {
        return Decision {
            delta: 0,
            quality: "out_of_order",
            update_anchor: false,
        };
    }
    let reset = (steps as i64) < old.steps
        || sample
            .elapsed_s
            .zip(old.elapsed_s)
            .is_some_and(|(new, old)| (new as i64) < old);
    let delta = if reset {
        steps.into()
    } else {
        steps as i64 - old.steps
    };
    let close =
        connection == old.connection && mono_gap.unwrap_or(wall_gap) <= 5_000 && wall_gap <= 5_000;
    let quality = if reset {
        "counter_reset"
    } else if delta == 0 {
        "unchanged"
    } else if old.day != day {
        "day_boundary"
    } else if !close {
        "collection_gap"
    } else {
        "observed"
    };
    Decision {
        delta,
        quality,
        update_anchor: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prior() -> Anchor {
        Anchor {
            connection: "a".into(),
            utc: DateTime::from_timestamp(0, 0).unwrap(),
            monotonic_ns: Some(0),
            steps: 100,
            elapsed_s: Some(20),
            day: "1970-01-01".into(),
        }
    }
    #[test]
    fn boundaries_are_observed_only_with_close_ordered_continuity() {
        for gap in [0, 1, 4_999, 5_000, 5_001, 60_000] {
            for connection in ["a", "b"] {
                for mono in [None, Some(gap * 1_000_000)] {
                    let d = decide(
                        &Sample {
                            steps: Some(103),
                            elapsed_s: Some(23),
                            ..Default::default()
                        },
                        connection,
                        prior().utc + chrono::Duration::milliseconds(gap),
                        mono,
                        "1970-01-01",
                        Some(&prior()),
                    );
                    assert_eq!(d.delta, 3);
                    assert_eq!(
                        d.quality,
                        if connection == "a" && gap <= 5_000 {
                            "observed"
                        } else {
                            "collection_gap"
                        }
                    );
                    assert!(d.update_anchor);
                }
            }
        }
    }
    #[test]
    fn resets_missing_steps_and_initial_baselines_are_never_observed() {
        let s = Sample {
            steps: Some(3),
            elapsed_s: Some(21),
            ..Default::default()
        };
        assert_eq!(
            decide(&s, "a", prior().utc, Some(0), "1970-01-01", None).quality,
            "initial_baseline"
        );
        assert_eq!(
            decide(&s, "a", prior().utc, Some(0), "1970-01-01", Some(&prior())).quality,
            "counter_reset"
        );
        let missing = decide(
            &Sample::default(),
            "a",
            prior().utc,
            None,
            "1970-01-01",
            Some(&prior()),
        );
        assert_eq!(
            missing,
            Decision {
                delta: 0,
                quality: "missing_steps",
                update_anchor: false
            }
        );
        let elapsed_reset = Sample {
            steps: Some(105),
            elapsed_s: Some(1),
            ..Default::default()
        };
        assert_eq!(
            decide(
                &elapsed_reset,
                "a",
                prior().utc,
                Some(0),
                "1970-01-01",
                Some(&prior())
            )
            .delta,
            105
        );
    }
    #[test]
    fn ordering_and_day_boundaries_preserve_the_original_precedence() {
        let sample = Sample {
            steps: Some(101),
            ..Default::default()
        };
        for (wall, mono) in [(-1, Some(1)), (1, Some(-1_000_000))] {
            let d = decide(
                &sample,
                "a",
                prior().utc + chrono::Duration::milliseconds(wall),
                mono,
                "1970-01-01",
                Some(&prior()),
            );
            assert_eq!(
                d,
                Decision {
                    delta: 0,
                    quality: "out_of_order",
                    update_anchor: false
                }
            );
        }
        assert_eq!(
            decide(
                &sample,
                "b",
                prior().utc,
                Some(-1_000_000),
                "1970-01-01",
                Some(&prior())
            )
            .quality,
            "collection_gap"
        );
        assert_eq!(
            decide(
                &sample,
                "a",
                prior().utc,
                Some(0),
                "1970-01-02",
                Some(&prior())
            )
            .quality,
            "day_boundary"
        );
        let unchanged = Sample {
            steps: Some(100),
            ..Default::default()
        };
        assert_eq!(
            decide(
                &unchanged,
                "b",
                prior().utc,
                None,
                "1970-01-02",
                Some(&prior())
            )
            .quality,
            "unchanged"
        );
    }
}
