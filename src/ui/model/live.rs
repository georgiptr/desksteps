//! Live card: connection pill, measured speed, command enable flags and progress.
use crate::{application::View, statistics::Day, ui::kit::theme::Tone};

#[derive(Clone, Debug, PartialEq)]
pub struct LiveCard {
    pub connection: &'static str,
    pub tone: Tone,
    /// Measured (or simulated) speed, or an em dash when unknown. Never the selected speed.
    pub speed: String,
    pub speed_caption: &'static str,
    /// Set only while the selected speed differs from the measured one.
    pub pending: Option<String>,
    pub decrease: String,
    pub increase: String,
    pub minus: bool,
    pub plus: bool,
    pub start: bool,
    pub pause: bool,
    pub steps: String,
    pub target_fraction: Option<f32>,
    pub footer_error: Option<String>,
}
pub const START_HINT: &str =
    "Start/Resume uses the speed selected by the pad. Pause retains workout counters.";

pub fn group_digits(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 { format!("-{out}") } else { out }
}
/// A completed step count, its target and uncapped whole-number percentage.
pub fn step_progress(steps: i64, target: i64, unit: &str) -> String {
    format!(
        "{}/{}{unit} · {:.0}%",
        group_digits(steps),
        group_digits(target),
        steps as f64 / target as f64 * 100.0
    )
}
fn connection(v: &View) -> (&'static str, Tone) {
    if v.development {
        ("Synthetic pad", Tone::Development)
    } else if v.no_bluetooth {
        ("Bluetooth off (preview)", Tone::Neutral)
    } else if v.connected && v.control_uncertain {
        ("Uncertain", Tone::Warning)
    } else if v.connected {
        ("Connected", Tone::Good)
    } else if v.status.starts_with("Connecting") {
        ("Connecting", Tone::Warning)
    } else {
        ("Disconnected", Tone::Bad)
    }
}
impl LiveCard {
    /// `today` is today's dashboard day when known: its steps and target are what the Today
    /// page shows. Without it the card falls back to the live recorded total, with no target.
    pub fn from_view(v: &View, today: Option<&Day>) -> Self {
        // These rules are the ones the classic control strip applies in its refresh.
        let available = v.connected && v.controls_enabled && (!v.no_bluetooth || v.development);
        let speed = v.fresh_speed;
        let ready = available && !v.control_busy && !v.control_uncertain;
        let speed_ready = available && v.speed_available && !v.control_uncertain;
        let (connection, tone) = connection(v);
        let step = v.increment.max(1) as f64 / 10.0;
        let today = today.filter(|d| d.date == v.totals.day);
        let steps = today.map_or(v.totals.recorded_steps, |d| d.steps);
        let target = today.and_then(|d| d.preferences.steps).filter(|t| *t > 0);
        Self {
            connection,
            tone,
            speed: speed
                .map(|n| format!("{n:.1}"))
                .unwrap_or_else(|| "—".into()),
            speed_caption: match (speed.is_some(), v.development) {
                (false, _) => "km/h",
                (true, true) => "km/h · simulated",
                (true, false) => "km/h",
            },
            pending: v
                .selected_speed
                .filter(|n| speed.is_none_or(|m| format!("{m:.1}") != format!("{n:.1}")))
                .map(|n| format!("→ {n:.1} selected")),
            decrease: format!("−{step:.1}"),
            increase: format!("+{step:.1}"),
            minus: speed_ready && v.adjustable_speed.is_some_and(|n| n > 1.0),
            plus: speed_ready && v.adjustable_speed.is_some_and(|n| (1.0..6.0).contains(&n)),
            start: ready && !speed.is_some_and(|n| n > 0.0),
            pause: available,
            steps: match target {
                Some(t) => step_progress(steps, t, " steps"),
                None => format!("{} steps today", group_digits(steps)),
            },
            target_fraction: target.map(|t| (steps as f64 / t as f64).clamp(0.0, 1.0) as f32),
            footer_error: v.error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statistics::{Day, Preferences};
    fn ready() -> View {
        View {
            connected: true,
            controls_enabled: true,
            speed_available: true,
            increment: 1,
            fresh_speed: Some(0.0),
            adjustable_speed: Some(3.0),
            ..Default::default()
        }
    }
    fn flags(v: &View) -> [bool; 4] {
        let c = LiveCard::from_view(v, None);
        [c.minus, c.plus, c.start, c.pause]
    }
    #[test]
    fn stopped_available_pad_enables_all_controls() {
        assert_eq!(flags(&ready()), [true, true, true, true]);
    }
    #[test]
    fn pause_follows_availability_only() {
        for (v, expected) in [
            (
                View {
                    connected: false,
                    ..ready()
                },
                false,
            ),
            (
                View {
                    controls_enabled: false,
                    ..ready()
                },
                false,
            ),
            (
                View {
                    no_bluetooth: true,
                    ..ready()
                },
                false,
            ),
            (
                View {
                    no_bluetooth: true,
                    development: true,
                    ..ready()
                },
                true,
            ),
            // Busy and uncertain exchanges never disable Pause.
            (
                View {
                    control_busy: true,
                    ..ready()
                },
                true,
            ),
            (
                View {
                    control_uncertain: true,
                    ..ready()
                },
                true,
            ),
            (
                View {
                    control_busy: true,
                    control_uncertain: true,
                    ..ready()
                },
                true,
            ),
            (
                View {
                    fresh_speed: Some(3.0),
                    ..ready()
                },
                true,
            ),
        ] {
            assert_eq!(LiveCard::from_view(&v, None).pause, expected);
        }
    }
    #[test]
    fn start_is_blocked_while_moving_busy_or_uncertain() {
        for (v, expected) in [
            (ready(), true),
            (
                View {
                    fresh_speed: None,
                    ..ready()
                },
                true,
            ),
            (
                View {
                    fresh_speed: Some(0.1),
                    ..ready()
                },
                false,
            ),
            (
                View {
                    control_busy: true,
                    ..ready()
                },
                false,
            ),
            (
                View {
                    control_uncertain: true,
                    ..ready()
                },
                false,
            ),
            (
                View {
                    connected: false,
                    ..ready()
                },
                false,
            ),
        ] {
            assert_eq!(LiveCard::from_view(&v, None).start, expected);
        }
    }
    #[test]
    fn speed_buttons_follow_adjustable_speed_and_limits() {
        for (adjustable, minus, plus) in [
            (None, false, false),
            (Some(0.9), false, false),
            (Some(1.0), false, true),
            (Some(1.1), true, true),
            (Some(5.9), true, true),
            (Some(6.0), true, false),
        ] {
            let v = View {
                adjustable_speed: adjustable,
                ..ready()
            };
            assert_eq!(flags(&v)[..2], [minus, plus], "{adjustable:?}");
        }
        // A busy speed write still allows more clicks; uncertainty or a missing speed does not.
        assert_eq!(
            flags(&View {
                control_busy: true,
                ..ready()
            })[..2],
            [true, true]
        );
        assert_eq!(
            flags(&View {
                control_uncertain: true,
                ..ready()
            })[..2],
            [false, false]
        );
        assert_eq!(
            flags(&View {
                speed_available: false,
                ..ready()
            })[..2],
            [false, false]
        );
        assert_eq!(
            flags(&View {
                connected: false,
                ..ready()
            })[..2],
            [false, false]
        );
    }
    #[test]
    fn selected_speed_is_never_the_measured_speed() {
        let c = LiveCard::from_view(
            &View {
                selected_speed: Some(3.5),
                fresh_speed: None,
                ..ready()
            },
            None,
        );
        assert_eq!(c.speed, "—");
        assert_eq!(c.speed_caption, "km/h");
        assert_eq!(c.pending.as_deref(), Some("→ 3.5 selected"));
        let c = LiveCard::from_view(
            &View {
                selected_speed: Some(3.5),
                fresh_speed: Some(3.0),
                ..ready()
            },
            None,
        );
        assert_eq!(c.speed, "3.0");
        assert_eq!(c.speed_caption, "km/h");
        assert_eq!(c.pending.as_deref(), Some("→ 3.5 selected"));
        let c = LiveCard::from_view(
            &View {
                selected_speed: Some(3.0),
                fresh_speed: Some(3.0),
                ..ready()
            },
            None,
        );
        assert_eq!(c.pending, None);
        let c = LiveCard::from_view(
            &View {
                development: true,
                fresh_speed: Some(2.0),
                ..ready()
            },
            None,
        );
        assert!(c.speed_caption.contains("simulated"));
    }
    #[test]
    fn step_labels_follow_the_increment() {
        let c = LiveCard::from_view(
            &View {
                increment: 5,
                ..ready()
            },
            None,
        );
        assert_eq!((c.decrease.as_str(), c.increase.as_str()), ("−0.5", "+0.5"));
        let c = LiveCard::from_view(
            &View {
                increment: 0,
                ..ready()
            },
            None,
        );
        assert_eq!(c.increase, "+0.1");
    }
    #[test]
    fn connection_pill_covers_every_state() {
        let base = View::default();
        for (v, label, tone) in [
            (
                View {
                    development: true,
                    ..base.clone()
                },
                "Synthetic pad",
                Tone::Development,
            ),
            (
                View {
                    no_bluetooth: true,
                    ..base.clone()
                },
                "Bluetooth off (preview)",
                Tone::Neutral,
            ),
            (
                View {
                    connected: true,
                    control_uncertain: true,
                    ..base.clone()
                },
                "Uncertain",
                Tone::Warning,
            ),
            (
                View {
                    connected: true,
                    ..base.clone()
                },
                "Connected",
                Tone::Good,
            ),
            (
                View {
                    status: "Connecting to WalkingPad".into(),
                    ..base.clone()
                },
                "Connecting",
                Tone::Warning,
            ),
            (base.clone(), "Disconnected", Tone::Bad),
        ] {
            let c = LiveCard::from_view(&v, None);
            assert_eq!((c.connection, c.tone), (label, tone));
        }
    }
    #[test]
    fn steps_progress_uses_the_target_for_today_only() {
        let mut v = ready();
        v.totals.day = "2026-10-23".into();
        v.totals.recorded_steps = 4231;
        let c = LiveCard::from_view(&v, None);
        assert_eq!(
            (c.steps.as_str(), c.target_fraction),
            ("4,231 steps today", None)
        );
        let day = |date: &str, steps| Day {
            date: date.into(),
            steps,
            preferences: Preferences {
                steps: Some(8000),
                ..Default::default()
            },
            ..Default::default()
        };
        // A day other than today never supplies the target.
        let c = LiveCard::from_view(&v, Some(&day("2026-10-22", 10)));
        assert_eq!(
            (c.steps.as_str(), c.target_fraction),
            ("4,231 steps today", None)
        );
        // Today's dashboard day is the same source the Today page uses.
        let c = LiveCard::from_view(&v, Some(&day("2026-10-23", 4000)));
        assert_eq!(c.steps, "4,000/8,000 steps · 50%");
        assert!((c.target_fraction.unwrap() - 0.5).abs() < 1e-6);
        let c = LiveCard::from_view(&v, Some(&day("2026-10-23", 9000)));
        assert_eq!(c.steps, "9,000/8,000 steps · 112%");
        assert_eq!(c.target_fraction, Some(1.0));
    }
    #[test]
    fn footer_reports_recording_errors() {
        let mut v = ready();
        assert_eq!(LiveCard::from_view(&ready(), None).footer_error, None);
        v.last_sample = Some("09:38:12".into());
        v.error = Some("Recording stopped".into());
        let c = LiveCard::from_view(&v, None);
        assert_eq!(c.footer_error.as_deref(), Some("Recording stopped"));
    }
    #[test]
    fn digits_are_grouped() {
        for (n, text) in [
            (0, "0"),
            (999, "999"),
            (1000, "1,000"),
            (1234567, "1,234,567"),
            (-12345, "-12,345"),
        ] {
            assert_eq!(group_digits(n), text);
        }
    }
}
