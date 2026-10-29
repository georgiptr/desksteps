//! Day and history view models projected from a statistics `Dashboard`.
//! Every number a page shows for a day comes from `Dashboard.day`, never from live totals.
use crate::statistics::{self, Dashboard, Day, Session};
use chrono::{DateTime, NaiveDate, Timelike};
use chrono_tz::Tz;

pub fn amount(value: Option<f64>) -> String {
    value
        .map(|v| format!("{:.0}", v.round()))
        .unwrap_or_else(|| "N/A".into())
}
pub fn recovery_text(steps: i64, ms: i64, kcal: Option<f64>, missing: bool) -> Option<String> {
    if steps == 0 && ms == 0 {
        return None;
    }
    Some(format!(
        "Recovered: {steps} steps · {} min · {} active kcal{}. Time assumes walking during the device workout.",
        amount(Some(ms as f64 / 60000.0)),
        amount(kcal),
        if missing { " (incomplete)" } else { "" }
    ))
}
fn selected_recovery(
    steps: i64,
    ms: i64,
    kcal: Option<f64>,
    missing: bool,
    pad: Option<&statistics::PadCalories>,
) -> Option<String> {
    if let Some(pad) = pad {
        recovery_text(steps, ms, pad.recovered, pad.recovery_missing)
            .map(|text| text.replace("active kcal", "pad kcal"))
    } else {
        recovery_text(steps, ms, kcal, missing)
    }
}
pub fn local_time(stamp: &str, zone: Tz) -> String {
    DateTime::parse_from_rfc3339(stamp)
        .map(|d| d.with_timezone(&zone).format("%H:%M").to_string())
        .unwrap_or_else(|_| "?".into())
}
fn zone(data: &Dashboard) -> Tz {
    data.timezone.parse().unwrap_or(chrono_tz::UTC)
}
/// "Today", or the weekday and date for any other day.
pub fn day_title(date: &str, today: &str) -> String {
    if date == today {
        return "Today".into();
    }
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| d.format("%a %-d %b %Y").to_string())
        .unwrap_or_else(|_| date.into())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub title: &'static str,
    pub value: String,
    /// Completed/target and percentage, or a hint when no target is usable.
    pub target: String,
    /// "302 remaining" or "12 above target"; empty without a target.
    pub remaining: String,
    pub progress: Option<f32>,
}
fn tile(title: &'static str, value: Option<f64>, goal: Option<f64>, hint: &str) -> Tile {
    let goal = goal.filter(|g| g.is_finite() && *g > 0.0);
    let text = |v: f64| crate::ui::model::live::group_digits(v.round() as i64);
    let (Some(v), Some(g)) = (value, goal) else {
        return Tile {
            title,
            value: value.map(text).unwrap_or_else(|| "—".into()),
            target: if value.is_none() && !hint.is_empty() {
                hint.into()
            } else if goal.is_none() {
                "No target set".into()
            } else {
                hint.into()
            },
            remaining: String::new(),
            progress: None,
        };
    };
    let left = g - v;
    Tile {
        title,
        value: text(v),
        target: format!("{}/{} · {:.0}%", text(v), text(g), v / g * 100.0),
        remaining: if left >= 0.0 {
            format!("{} remaining", text(left))
        } else {
            format!("{} above target", text(-left))
        },
        progress: Some((v / g).clamp(0.0, 1.0) as f32),
    }
}
/// Steps, walking minutes and the selected calorie source, rounded only for display.
pub fn tiles(d: &Day) -> [Tile; 3] {
    let p = &d.preferences;
    let steps = tile("Steps", Some(d.steps as f64), p.steps.map(|s| s as f64), "");
    let mut calories = tile(
        if p.weight.is_some() {
            "Active kcal"
        } else {
            "Pad kcal"
        },
        d.displayed_kcal(),
        p.calories,
        if p.weight.is_none() {
            "No usable pad calorie readings"
        } else {
            "Needs usable speed or recovered distance/time"
        },
    );
    if p.weight.is_none() && d.pad_calories.incomplete {
        calories.target.push_str(" · partial");
    }
    [
        steps,
        tile(
            "Walking minutes",
            Some(d.active_ms as f64 / 60000.0),
            p.minutes,
            "",
        ),
        calories,
    ]
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    pub start: String,
    /// "now" while live.
    pub end: String,
    pub state: String,
    pub live: bool,
    pub continued: bool,
    /// Wall-clock length: "30 min", "continued · 30 min" or "Live · 34 min".
    pub duration: String,
    pub steps: String,
    pub minutes: String,
    pub kcal: String,
    /// "24 min · 61 kcal".
    pub detail: String,
    pub gap: bool,
    /// "Recording gap · 12 steps recovered"; only for gaps.
    pub gap_line: Option<String>,
    pub recovery: Option<String>,
}
/// "45 min" or "3 h 44 min", rounded to the minute.
pub fn duration_text(seconds: i64) -> String {
    let minutes = (seconds.max(0) + 30) / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}
fn wall_seconds(c: &Session) -> i64 {
    match (
        DateTime::parse_from_rfc3339(&c.start),
        DateTime::parse_from_rfc3339(&c.end),
    ) {
        (Ok(a), Ok(b)) => (b - a).num_seconds(),
        _ => 0,
    }
}
/// Minutes since local midnight of `day`, clamped to the day's 0 to 1440.
fn day_minutes(stamp: &str, zone: Tz, day: &str) -> Option<f32> {
    let t = DateTime::parse_from_rfc3339(stamp)
        .ok()?
        .with_timezone(&zone);
    let date = t.format("%Y-%m-%d").to_string();
    Some(match date.as_str().cmp(day) {
        std::cmp::Ordering::Less => 0.0,
        std::cmp::Ordering::Greater => 1440.0,
        std::cmp::Ordering::Equal => (t.hour() * 60 + t.minute()) as f32 + t.second() as f32 / 60.0,
    })
}
fn session_row(c: &Session, zone: Tz, day: &str, live: bool, use_pad: bool) -> SessionRow {
    let continued = DateTime::parse_from_rfc3339(&c.start)
        .is_ok_and(|t| t.with_timezone(&zone).format("%Y-%m-%d").to_string() != day);
    let length = duration_text(wall_seconds(c));
    let minutes = amount(Some(c.active_ms as f64 / 60000.0));
    let kcal = amount(if use_pad {
        c.pad_calories.total
    } else {
        c.kcal
    });
    let partial_calories = use_pad && c.pad_calories.incomplete;
    SessionRow {
        start: local_time(&c.start, zone),
        end: if live {
            "now".into()
        } else {
            local_time(&c.end, zone)
        },
        state: c.state.clone(),
        live,
        continued,
        duration: match (live, continued) {
            (true, _) => format!("Live · {length}"),
            (false, true) => format!("continued · {length}"),
            _ => length,
        },
        steps: crate::ui::model::live::group_digits(c.steps),
        detail: format!("{minutes} min · {kcal} kcal"),
        minutes,
        kcal,
        gap: c.gap || partial_calories,
        gap_line: if !c.gap && partial_calories {
            Some("Partial pad calories".into())
        } else {
            c.gap.then(|| match c.recovered_steps {
                0 => "Recording gap · no steps recovered".to_string(),
                1 => "Recording gap · 1 step recovered".to_string(),
                n => format!(
                    "Recording gap · {} steps recovered",
                    crate::ui::model::live::group_digits(n)
                ),
            })
        },
        recovery: selected_recovery(
            c.recovered_steps,
            c.recovered_ms,
            c.recovered_kcal,
            c.recovery_kcal_missing,
            use_pad.then_some(&c.pad_calories),
        ),
    }
}
pub fn session_rows(data: &Dashboard) -> Vec<SessionRow> {
    let zone = zone(data);
    data.sessions
        .iter()
        .map(|c| {
            let live = data.current.as_ref().is_some_and(|l| l.id == c.id);
            session_row(
                c,
                zone,
                &data.day.date,
                live,
                data.day.preferences.weight.is_none(),
            )
        })
        .collect()
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineBar {
    /// Fractions of the track, 0 to 1.
    pub start: f32,
    pub end: f32,
    pub gap: bool,
    pub live: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    /// Track bounds in minutes since local midnight, whole hours.
    pub from: u32,
    pub to: u32,
    pub bars: Vec<TimelineBar>,
    /// Earliest start to latest end inside the day, in minutes.
    pub span: f32,
}
impl Timeline {
    pub fn new(data: &Dashboard) -> Option<Self> {
        let zone = zone(data);
        let day = &data.day.date;
        let spans: Vec<(f32, f32, bool, bool)> = data
            .sessions
            .iter()
            .filter_map(|s| {
                let a = day_minutes(&s.start, zone, day)?;
                let b = day_minutes(&s.end, zone, day)?.max(a);
                let live = data.current.as_ref().is_some_and(|l| l.id == s.id);
                Some((a, b, s.gap, live))
            })
            .collect();
        let first = spans.iter().map(|s| s.0).reduce(f32::min)?;
        let last = spans.iter().map(|s| s.1).reduce(f32::max)?;
        let from = ((first / 60.0).floor() as u32 * 60).min(1380);
        let to = ((last / 60.0).ceil() as u32 * 60).clamp(from + 60, 1440);
        let len = (to - from) as f32;
        Some(Self {
            from,
            to,
            span: last - first,
            bars: spans
                .into_iter()
                .map(|(a, b, gap, live)| TimelineBar {
                    start: ((a - from as f32) / len).clamp(0.0, 1.0),
                    end: ((b - from as f32) / len).clamp(0.0, 1.0),
                    gap,
                    live,
                })
                .collect(),
        })
    }
}
/// Hour labels for a track `width` px wide: (fraction, "HH:00"), at least 48 px apart.
pub fn timeline_ticks(from: u32, to: u32, width: f32) -> Vec<(f32, String)> {
    let hours = (to.saturating_sub(from) / 60).max(1);
    let step = [1u32, 2, 3, 4, 6, 12, 24]
        .into_iter()
        .find(|s| width * *s as f32 / hours as f32 >= 48.0)
        .unwrap_or(24);
    (0..=hours)
        .step_by(step as usize)
        .map(|h| {
            (
                h as f32 / hours as f32,
                format!("{:02}:00", (from / 60 + h) % 24),
            )
        })
        .collect()
}
/// "3 sessions · 24 min walking · 3 h 44 min".
fn summary(data: &Dashboard, span: Option<f32>) -> String {
    let n = data.sessions.len();
    let walking: i64 = data.sessions.iter().map(|s| s.active_ms).sum();
    let minutes = walking as f64 / 60000.0;
    let mut text = format!(
        "{n} session{} · {} min",
        if n == 1 { "" } else { "s" },
        amount(Some(minutes))
    );
    if let Some(span) = span.filter(|span| (f64::from(*span) - minutes).abs() > 1.0) {
        text.push_str(&format!(
            " walking · {}",
            duration_text((span * 60.0).round() as i64)
        ));
    }
    text
}
#[derive(Clone, Debug, PartialEq)]
pub struct DayView {
    pub date: String,
    pub title: String,
    pub is_today: bool,
    pub tiles: [Tile; 3],
    pub sessions: Vec<SessionRow>,
    pub timeline: Option<Timeline>,
    pub summary: String,
    pub has_gap: bool,
    pub legacy: Option<String>,
    pub recovery: Option<String>,
}
impl DayView {
    pub fn new(data: &Dashboard) -> Self {
        let d = &data.day;
        let timeline = Timeline::new(data);
        Self {
            date: d.date.clone(),
            title: day_title(&d.date, &data.today),
            is_today: d.date == data.today,
            tiles: tiles(d),
            sessions: session_rows(data),
            summary: summary(data, timeline.as_ref().map(|t| t.span)),
            timeline,
            has_gap: data.sessions.iter().any(|s| s.gap),
            recovery: selected_recovery(
                d.recovered_steps,
                d.recovered_ms,
                d.recovered_kcal,
                d.recovery_kcal_missing,
                d.preferences.weight.is_none().then_some(&d.pad_calories),
            ),
            legacy: (d.unassigned > 0)
                .then(|| format!("{} legacy steps without sessions", d.unassigned)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarKind {
    Observed,
    Partial,
    Empty,
}
fn kind(d: &Day) -> BarKind {
    if d.samples == 0 {
        BarKind::Empty
    } else if d.uncertain > 0
        || d.unassigned > 0
        || d.partial_sessions > 0
        || d.preferences.weight.is_none() && d.pad_calories.incomplete
    {
        BarKind::Partial
    } else {
        BarKind::Observed
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub date: String,
    /// "10-23".
    pub label: String,
    pub steps: i64,
    pub goal: Option<i64>,
    pub kind: BarKind,
    pub tooltip: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryRow {
    pub date: String,
    pub steps: String,
    pub minutes: String,
    pub kcal: String,
    pub sessions: String,
    pub target: String,
    pub kind: BarKind,
    pub coverage: &'static str,
}
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryView {
    /// Oldest first, as drawn.
    pub bars: Vec<Bar>,
    /// Newest first.
    pub rows: Vec<HistoryRow>,
}
impl HistoryView {
    pub fn new(data: &Dashboard) -> Self {
        let bars = data
            .days
            .iter()
            .map(|d| Bar {
                date: d.date.clone(),
                label: d.date.get(5..).unwrap_or(&d.date).into(),
                steps: d.steps,
                goal: d.preferences.steps,
                kind: kind(d),
                tooltip: format!(
                    "{} · {} steps · {} min · {} {} · {} sessions · {}{}{}",
                    d.date,
                    d.steps,
                    amount(Some(d.active_ms as f64 / 60000.0)),
                    amount(d.displayed_kcal()),
                    d.calorie_label(),
                    d.sessions,
                    d.quality(),
                    if d.preferences.weight.is_none() && d.pad_calories.incomplete {
                        " · Partial pad calories"
                    } else {
                        ""
                    },
                    selected_recovery(
                        d.recovered_steps,
                        d.recovered_ms,
                        d.recovered_kcal,
                        d.recovery_kcal_missing,
                        d.preferences.weight.is_none().then_some(&d.pad_calories),
                    )
                    .map(|s| format!(" · {s}"))
                    .unwrap_or_default()
                ),
            })
            .collect();
        let rows = data
            .days
            .iter()
            .rev()
            .map(|d| HistoryRow {
                date: d.date.clone(),
                steps: d.steps.to_string(),
                minutes: amount(Some(d.active_ms as f64 / 60000.0)),
                kcal: amount(d.displayed_kcal()),
                sessions: d.sessions.to_string(),
                target: statistics::progress(d.steps as f64, d.preferences.steps.map(|v| v as f64))
                    .map(|p| format!("{p:.0}%"))
                    .unwrap_or_else(|| "Off".into()),
                kind: kind(d),
                coverage: if d.preferences.weight.is_none() && d.pad_calories.incomplete {
                    "Partial / gaps or pad calories"
                } else {
                    d.quality()
                },
            })
            .collect();
        Self { bars, rows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weight_selects_calories_for_tiles_sessions_recovery_and_history_targets() {
        let mut d = data();
        d.day.kcal = Some(25.0);
        d.day.preferences.calories = Some(100.0);
        d.day.pad_calories = statistics::PadCalories {
            total: Some(60.0),
            recovered: Some(10.0),
            ..Default::default()
        };
        d.day.recovered_steps = 10;
        d.sessions[0].kcal = Some(25.0);
        d.sessions[0].recovered_steps = 10;
        d.sessions[0].pad_calories = d.day.pad_calories.clone();
        let v = DayView::new(&d);
        assert_eq!(
            (v.tiles[2].title, v.tiles[2].value.as_str()),
            ("Pad kcal", "60")
        );
        assert_eq!(v.tiles[2].target, "60/100 · 60%");
        assert_eq!(v.sessions[0].kcal, "60");
        assert!(v.recovery.unwrap().contains("10 pad kcal"));
        assert!(
            v.sessions[0]
                .recovery
                .as_ref()
                .unwrap()
                .contains("10 pad kcal")
        );
        d.days.push(d.day.clone());
        d.day.preferences.weight = Some(80.0);
        d.day.recovered_kcal = Some(5.0);
        d.sessions[0].recovered_kcal = Some(5.0);
        let v = DayView::new(&d);
        assert_eq!(
            (v.tiles[2].title, v.tiles[2].value.as_str()),
            ("Active kcal", "25")
        );
        assert_eq!(v.tiles[2].target, "25/100 · 25%");
        assert_eq!(v.sessions[0].kcal, "25");
        assert!(v.recovery.unwrap().contains("5 active kcal"));
        d.days.push(d.day.clone());
        let h = HistoryView::new(&d);
        assert_eq!(
            (h.rows[0].kcal.as_str(), h.rows[1].kcal.as_str()),
            ("25", "60")
        );
        assert!(h.bars[0].tooltip.contains("60 pad kcal"));
        assert!(h.bars[1].tooltip.contains("25 active kcal"));
        // Incomplete pad coverage stays explicit and does not alter the weighted estimate.
        d.day.preferences.weight = None;
        d.day.pad_calories.incomplete = true;
        d.sessions[0].gap = false;
        d.sessions[0].pad_calories.incomplete = true;
        let v = DayView::new(&d);
        assert!(v.tiles[2].target.ends_with("partial"));
        assert_eq!(
            v.sessions[0].gap_line.as_deref(),
            Some("Partial pad calories")
        );
        d.days = vec![d.day.clone()];
        let h = HistoryView::new(&d);
        assert_eq!(h.rows[0].coverage, "Partial / gaps or pad calories");
        assert!(h.bars[0].tooltip.contains("Partial pad calories"));
        assert_eq!(h.bars[0].kind, BarKind::Partial);
        d.day.preferences.weight = Some(80.0);
        assert!(!DayView::new(&d).tiles[2].target.contains("partial"));
        d.day.kcal = None;
        assert_eq!(DayView::new(&d).tiles[2].value, "—");
    }
    #[test]
    fn recovered_contributions_are_visible_in_days_sessions_and_history() {
        let mut d = data();
        d.day.preferences.weight = Some(80.0);
        d.day.recovered_steps = 300;
        d.day.recovered_ms = 180000;
        d.day.recovered_kcal = Some(7.56);
        d.sessions[0].recovered_steps = 300;
        d.sessions[0].recovered_ms = 180000;
        d.sessions[0].recovery_kcal_missing = true;
        d.days.push(d.day.clone());
        let v = DayView::new(&d);
        assert!(
            v.recovery
                .as_ref()
                .unwrap()
                .contains("300 steps · 3 min · 8")
        );
        assert!(
            v.sessions[0]
                .recovery
                .as_ref()
                .unwrap()
                .contains("N/A active kcal (incomplete)")
        );
        assert!(HistoryView::new(&d).bars[0].tooltip.contains("Recovered:"));
        assert_eq!(recovery_text(0, 0, None, false), None);
    }
    use crate::statistics::Preferences;
    #[test]
    fn invalid_dashboard_timezone_uses_utc_without_a_machine_specific_default() {
        let mut data = Dashboard {
            timezone: "invalid/timezone".into(),
            ..Default::default()
        };
        assert_eq!(zone(&data), chrono_tz::UTC);
        data.timezone = "America/New_York".into();
        assert_eq!(zone(&data), chrono_tz::America::New_York);
    }
    fn day(steps: i64, minutes: f64, kcal: Option<f64>, p: Preferences) -> Day {
        Day {
            date: "2026-10-23".into(),
            steps,
            active_ms: (minutes * 60000.0) as i64,
            kcal,
            samples: 5,
            preferences: p,
            ..Default::default()
        }
    }
    #[test]
    fn tiles_show_targets_percentages_and_whole_minutes_and_kcal() {
        let p = Preferences {
            steps: Some(1000),
            minutes: Some(30.0),
            calories: Some(100.0),
            weight: Some(80.0),
            ..Default::default()
        };
        let t = tiles(&day(698, 6.3, Some(14.7), p));
        assert_eq!(t[0].value, "698");
        assert_eq!(t[0].target, "698/1,000 · 70%");
        assert_eq!(t[0].remaining, "302 remaining");
        assert!((t[0].progress.unwrap() - 0.698).abs() < 1e-6);
        assert_eq!(t[1].value, "6");
        assert_eq!(t[1].target, "6/30 · 21%");
        assert_eq!(t[1].remaining, "24 remaining");
        assert_eq!(t[2].value, "15");
        assert_eq!(t[2].target, "15/100 · 15%");
        assert_eq!(t[2].remaining, "85 remaining");
        assert!(t[2].progress.unwrap() < 0.2);
    }
    #[test]
    fn all_target_tiles_show_completed_goal_percent_and_remaining_on_separate_lines() {
        let p = Preferences {
            weight: Some(80.0),
            steps: Some(2000),
            minutes: Some(200.0),
            calories: Some(400.0),
            ..Default::default()
        };
        let t = tiles(&day(2, 120.0, Some(120.0), p.clone()));
        assert_eq!(
            (t[0].target.as_str(), t[0].remaining.as_str()),
            ("2/2,000 · 0%", "1,998 remaining")
        );
        assert_eq!(
            (t[1].target.as_str(), t[1].remaining.as_str()),
            ("120/200 · 60%", "80 remaining")
        );
        assert_eq!(
            (t[2].target.as_str(), t[2].remaining.as_str()),
            ("120/400 · 30%", "280 remaining")
        );
        let met = tiles(&day(2000, 200.0, Some(400.0), p));
        for tile in met {
            assert!(tile.target.ends_with("100%"));
            assert_eq!(tile.remaining, "0 remaining");
        }
    }
    #[test]
    fn tiles_report_above_target_and_cap_progress() {
        let p = Preferences {
            steps: Some(500),
            ..Default::default()
        };
        let t = tiles(&day(1234, 1.0, None, p));
        assert_eq!(t[0].remaining, "734 above target");
        assert_eq!(t[0].progress, Some(1.0));
        assert_eq!(t[0].value, "1,234");
        assert_eq!(t[0].target, "1,234/500 · 247%");
        assert!(!t[1].target.contains('%'));
        assert!(!t[2].target.contains('%'));
    }
    #[test]
    fn step_count_stays_prominent_and_is_repeated_with_target_and_percentage() {
        let p = Preferences {
            steps: Some(8000),
            ..Default::default()
        };
        let t = tiles(&day(1500, 0.0, None, p));
        assert_eq!(t[0].title, "Steps");
        assert_eq!(t[0].value, "1,500");
        assert_eq!(t[0].target, "1,500/8,000 · 19%");
        assert_eq!(t[0].remaining, "6,500 remaining");
    }
    #[test]
    fn percentages_require_each_metrics_own_target_and_available_value() {
        for selected in 0..3 {
            let p = Preferences {
                steps: (selected == 0).then_some(1000),
                minutes: (selected == 1).then_some(30.0),
                calories: (selected == 2).then_some(100.0),
                weight: Some(80.0),
                ..Default::default()
            };
            for (index, t) in tiles(&day(500, 15.0, Some(50.0), p)).iter().enumerate() {
                assert_eq!(t.target.contains('%'), index == selected);
                if index == selected {
                    assert!(t.target.contains("50%"));
                }
            }
        }
        let p = Preferences {
            steps: Some(1000),
            minutes: Some(30.0),
            calories: Some(100.0),
            ..Default::default()
        };
        let t = tiles(&day(0, 0.0, None, p));
        assert_eq!(t[0].target, "0/1,000 · 0%");
        assert_eq!(t[1].target, "0/30 · 0%");
        assert!(!t[2].target.contains('%'));
    }
    #[test]
    fn missing_targets_weights_and_estimates_have_explanations() {
        let t = tiles(&day(10, 0.0, None, Preferences::default()));
        assert_eq!(
            (t[0].target.as_str(), t[0].progress),
            ("No target set", None)
        );
        assert!(t[0].remaining.is_empty());
        assert_eq!(
            (t[2].value.as_str(), t[2].target.as_str()),
            ("—", "No usable pad calorie readings")
        );
        let with_weight = Preferences {
            weight: Some(70.0),
            calories: Some(50.0),
            ..Default::default()
        };
        let t = tiles(&day(10, 0.0, None, with_weight));
        assert_eq!(t[2].target, "Needs usable speed or recovered distance/time");
        assert_eq!(t[2].progress, None);
        // A value without a target is shown plainly.
        let t = tiles(&day(
            10,
            2.0,
            Some(3.0),
            Preferences {
                weight: Some(80.0),
                ..Default::default()
            },
        ));
        assert_eq!(
            (t[2].value.as_str(), t[2].target.as_str()),
            ("3", "No target set")
        );
    }
    #[test]
    fn titles_use_today_or_weekday_and_date() {
        assert_eq!(day_title("2026-10-23", "2026-10-23"), "Today");
        assert_eq!(day_title("2026-10-22", "2026-10-23"), "Thu 22 Oct 2026");
        assert_eq!(day_title("bad", "2026-10-23"), "bad");
    }
    fn data() -> Dashboard {
        let mut d = Dashboard {
            day: Day {
                date: "2026-10-17".into(),
                unassigned: 4,
                samples: 3,
                ..Default::default()
            },
            today: "2026-10-17".into(),
            timezone: "UTC".into(),
            ..Default::default()
        };
        d.sessions.push(Session {
            id: 1,
            start: "2026-10-16T23:59:00Z".into(),
            end: "2026-10-17T00:01:00Z".into(),
            state: "Walking".into(),
            steps: 20,
            active_ms: 60_000,
            kcal: None,
            gap: true,
            ..Default::default()
        });
        d
    }
    #[test]
    fn day_view_collects_sessions_quality_and_legacy_steps() {
        let mut d = data();
        let v = DayView::new(&d);
        assert!(v.is_today && v.title == "Today");
        let row = &v.sessions[0];
        assert_eq!((row.start.as_str(), row.end.as_str()), ("23:59", "00:01"));
        assert_eq!(row.duration, "continued · 2 min");
        assert!(row.continued && row.gap && !row.live);
        assert_eq!(
            row.gap_line.as_deref(),
            Some("Recording gap · no steps recovered")
        );
        assert_eq!(row.detail, "1 min · N/A kcal");
        assert_eq!((row.minutes.as_str(), row.kcal.as_str()), ("1", "N/A"));
        assert_eq!(v.legacy.as_deref(), Some("4 legacy steps without sessions"));
        assert!(v.has_gap);
        d.day.unassigned = 0;
        d.today = "2026-10-18".into();
        d.sessions[0].start = "2026-10-17T00:00:00Z".into();
        d.sessions[0].gap = false;
        d.sessions[0].steps = 1825;
        let v = DayView::new(&d);
        let row = &v.sessions[0];
        assert!(!v.is_today && v.legacy.is_none() && !v.has_gap);
        assert_eq!((row.continued, row.gap), (false, false));
        assert_eq!(
            (row.gap_line.as_deref(), row.steps.as_str()),
            (None, "1,825")
        );
        assert_eq!(row.duration, "1 min");
    }
    #[test]
    fn the_session_matching_current_is_live_and_keeps_its_state() {
        let mut d = data();
        d.sessions[0].gap = false;
        d.sessions[0].state = "Continuity uncertain".into();
        d.sessions.push(Session {
            id: 2,
            start: "2026-10-17T08:00:00Z".into(),
            end: "2026-10-17T08:10:00Z".into(),
            ..Default::default()
        });
        assert!(session_rows(&d).iter().all(|r| !r.live));
        d.current = Some(d.sessions[0].clone());
        let rows = session_rows(&d);
        assert_eq!(
            rows.iter().map(|r| r.live).collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(
            (rows[0].start.as_str(), rows[0].end.as_str()),
            ("23:59", "now")
        );
        assert_eq!(rows[0].duration, "Live · 2 min");
        assert_eq!(rows[0].gap_line, None);
        assert_eq!(rows[0].state, "Continuity uncertain");
        d.sessions[0].gap = true;
        d.sessions[0].recovered_steps = 1234;
        let rows = session_rows(&d);
        assert_eq!(
            rows[0].gap_line.as_deref(),
            Some("Recording gap · 1,234 steps recovered")
        );
        d.sessions[0].recovered_steps = 1;
        assert_eq!(
            session_rows(&d)[0].gap_line.as_deref(),
            Some("Recording gap · 1 step recovered")
        );
    }
    #[test]
    fn durations_are_rounded_to_minutes_and_hours() {
        assert_eq!(duration_text(-5), "0 min");
        assert_eq!(duration_text(29), "0 min");
        assert_eq!(duration_text(30), "1 min");
        assert_eq!(duration_text(45 * 60), "45 min");
        assert_eq!(duration_text(3600), "1 h");
        assert_eq!(duration_text((3 * 60 + 44) * 60), "3 h 44 min");
        assert_eq!(wall_seconds(&Session::default()), 0);
    }
    fn day_of(sessions: &[(&str, &str, bool)]) -> Dashboard {
        let mut d = Dashboard {
            day: Day {
                date: "2026-10-23".into(),
                ..Default::default()
            },
            today: "2026-10-23".into(),
            timezone: "UTC".into(),
            ..Default::default()
        };
        for (i, (a, b, gap)) in sessions.iter().enumerate() {
            d.sessions.push(Session {
                id: i as i64 + 1,
                start: a.to_string(),
                end: b.to_string(),
                steps: 100 * (i as i64 + 1),
                active_ms: 600_000,
                gap: *gap,
                ..Default::default()
            });
        }
        d
    }
    #[test]
    fn summary_counts_sessions_walking_minutes_and_span() {
        let d = day_of(&[
            ("2026-10-23T09:10:00Z", "2026-10-23T09:40:00Z", false),
            ("2026-10-23T11:12:00Z", "2026-10-23T11:42:00Z", true),
        ]);
        assert_eq!(
            DayView::new(&d).summary,
            "2 sessions · 20 min walking · 2 h 32 min"
        );
        let one = day_of(&[("2026-10-23T09:00:00Z", "2026-10-23T09:45:00Z", false)]);
        assert_eq!(
            DayView::new(&one).summary,
            "1 session · 10 min walking · 45 min"
        );
        let none = DayView::new(&day_of(&[]));
        assert!(none.timeline.is_none());
        assert_eq!(none.summary, "0 sessions · 0 min");
    }
    #[test]
    fn summary_splits_durations_only_beyond_one_minute_difference() {
        for end in ["09:10:00", "09:10:59", "09:11:00", "09:09:00"] {
            let end = format!("2026-10-23T{end}Z");
            let d = day_of(&[("2026-10-23T09:00:00Z", &end, false)]);
            assert_eq!(DayView::new(&d).summary, "1 session · 10 min", "{end}");
        }
        let d = day_of(&[("2026-10-23T09:00:00Z", "2026-10-23T09:11:01Z", false)]);
        assert_eq!(
            DayView::new(&d).summary,
            "1 session · 10 min walking · 11 min"
        );
    }
    #[test]
    fn timeline_maps_sessions_to_track_fractions_between_whole_hours() {
        let d = day_of(&[
            ("2026-10-23T11:12:00Z", "2026-10-23T11:42:00Z", true),
            ("2026-10-23T09:10:00Z", "2026-10-23T09:40:00Z", false),
        ]);
        let t = Timeline::new(&d).unwrap();
        assert_eq!((t.from, t.to), (540, 720));
        assert!((t.span - 152.0).abs() < 1e-3);
        assert!((t.bars[0].start - 132.0 / 180.0).abs() < 1e-5);
        assert!((t.bars[0].end - 162.0 / 180.0).abs() < 1e-5);
        assert!(t.bars[0].gap && !t.bars[1].gap && !t.bars[1].live);
        assert!((t.bars[1].start - 10.0 / 180.0).abs() < 1e-5);
    }
    #[test]
    fn timeline_clamps_to_the_day_and_covers_at_least_an_hour() {
        let d = day_of(&[
            ("2026-10-22T23:50:00Z", "2026-10-23T00:20:00Z", false),
            ("2026-10-23T23:40:00Z", "2026-10-24T00:10:00Z", false),
        ]);
        let t = Timeline::new(&d).unwrap();
        assert_eq!((t.from, t.to), (0, 1440));
        assert_eq!((t.bars[0].start, t.bars[1].end), (0.0, 1.0));
        assert!((t.bars[0].end - 20.0 / 1440.0).abs() < 1e-5);
        let short = day_of(&[("2026-10-23T09:05:00Z", "2026-10-23T09:15:00Z", false)]);
        let t = Timeline::new(&short).unwrap();
        assert_eq!((t.from, t.to), (540, 600));
        let late = day_of(&[("2026-10-23T23:30:00Z", "2026-10-23T23:50:00Z", false)]);
        let t = Timeline::new(&late).unwrap();
        assert_eq!((t.from, t.to), (1380, 1440));
        let after = day_of(&[("2026-10-24T01:00:00Z", "2026-10-24T02:00:00Z", false)]);
        let t = Timeline::new(&after).unwrap();
        assert_eq!((t.from, t.to), (1380, 1440));
        assert_eq!((t.bars[0].start, t.bars[0].end), (1.0, 1.0));
        let bad = day_of(&[("bad", "bad", false)]);
        assert!(Timeline::new(&bad).is_none());
    }
    #[test]
    fn hour_ticks_thin_out_to_keep_labels_apart() {
        let t = timeline_ticks(540, 720, 300.0);
        assert_eq!(
            t.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(),
            ["09:00", "10:00", "11:00", "12:00"]
        );
        assert_eq!((t[0].0, t[3].0), (0.0, 1.0));
        let day = timeline_ticks(0, 1440, 300.0);
        assert_eq!(day.len(), 7);
        assert_eq!(day[1].1, "04:00");
        assert_eq!(day[6].1, "00:00");
        for (a, b) in day.iter().zip(day.iter().skip(1)) {
            assert!((b.0 - a.0) * 300.0 >= 48.0);
        }
        assert_eq!(timeline_ticks(540, 600, 100.0).len(), 2);
        assert_eq!(timeline_ticks(0, 1440, 10.0).len(), 2);
        assert_eq!(timeline_ticks(600, 600, 100.0).len(), 2);
    }
    #[test]
    fn history_bars_and_rows_are_ordered_and_classified() {
        let d = Dashboard {
            days: vec![
                Day {
                    date: "2026-10-15".into(),
                    ..Default::default()
                },
                Day {
                    date: "2026-10-16".into(),
                    steps: 12,
                    samples: 2,
                    uncertain: 1,
                    preferences: Preferences {
                        steps: Some(10),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                Day {
                    date: "2026-10-17".into(),
                    steps: 5,
                    samples: 2,
                    sessions: 1,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let h = HistoryView::new(&d);
        assert_eq!(
            h.bars.iter().map(|b| b.kind).collect::<Vec<_>>(),
            [BarKind::Empty, BarKind::Partial, BarKind::Observed]
        );
        assert_eq!(h.bars[1].label, "10-16");
        assert_eq!(h.bars[1].goal, Some(10));
        assert!(h.bars[1].tooltip.contains("12 steps") && h.bars[1].tooltip.contains("Partial"));
        assert_eq!(h.rows[0].date, "2026-10-17");
        assert_eq!(h.rows[1].target, "120%");
        assert_eq!(h.rows[0].target, "Off");
        assert_eq!(h.rows[2].coverage, "No recorded data");
        assert_eq!(h.rows[0].coverage, "Observed intervals only");
        assert_eq!(h.rows[0].sessions, "1");
        let empty = HistoryView::new(&Dashboard::default());
        assert!(empty.bars.is_empty() && empty.rows.is_empty());
    }
    #[test]
    fn classic_helpers_keep_their_text() {
        assert_eq!(amount(None), "N/A");
        assert_eq!(local_time("bad", chrono_tz::UTC), "?");
    }
}
