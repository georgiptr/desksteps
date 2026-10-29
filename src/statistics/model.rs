//! Statistics values and preference validation; no database access.
use super::*;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    pub effective: String,
    pub weight: Option<f64>,
    pub steps: Option<i64>,
    pub minutes: Option<f64>,
    pub calories: Option<f64>,
}
impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        date(&self.effective)?;
        if self
            .weight
            .is_some_and(|x| !x.is_finite() || !(20.0..=150.0).contains(&x))
        {
            return Err("Weight must be between 20 and 150 kg, or blank.".into());
        }
        if self.steps.is_some_and(|x| !(1..=1_000_000).contains(&x)) {
            return Err("Step target must be positive, or blank to disable it.".into());
        }
        if [self.minutes, self.calories]
            .iter()
            .flatten()
            .any(|x| !x.is_finite() || *x <= 0.0 || *x > 100_000.0)
        {
            return Err(
                "Minute and calorie targets must be positive, or blank to disable them.".into(),
            );
        }
        Ok(())
    }
}
pub fn date(value: &str) -> Result<NaiveDate, String> {
    let d = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "Use a date such as 2026-10-13.".to_string())?;
    if d.to_string() != value {
        return Err("Use YYYY-MM-DD with two-digit month and day.".into());
    }
    Ok(d)
}
#[derive(Clone, Debug, Default)]
pub struct PadCalories {
    pub total: Option<f64>,
    pub incomplete: bool,
    pub recovered: Option<f64>,
    pub recovery_missing: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Day {
    pub date: String,
    pub steps: i64,
    pub active_ms: i64,
    pub factor: f64,
    pub kcal: Option<f64>,
    pub pad_calories: PadCalories,
    pub sessions: i64,
    pub samples: i64,
    pub uncertain: i64,
    pub unassigned: i64,
    pub partial_sessions: i64,
    pub recovered_steps: i64,
    pub recovered_ms: i64,
    pub recovered_kcal: Option<f64>,
    pub recovery_kcal_missing: bool,
    pub preferences: Preferences,
}
impl Day {
    pub fn displayed_kcal(&self) -> Option<f64> {
        if self.preferences.weight.is_some() {
            self.kcal
        } else {
            self.pad_calories.total
        }
    }
    pub fn calorie_label(&self) -> &'static str {
        if self.preferences.weight.is_some() {
            "active kcal"
        } else {
            "pad kcal"
        }
    }
    pub fn quality(&self) -> &'static str {
        if self.samples == 0 {
            "No recorded data"
        } else if self.uncertain > 0 || self.unassigned > 0 || self.partial_sessions > 0 {
            "Partial / gaps or legacy data"
        } else {
            "Observed intervals only"
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub id: i64,
    pub start: String,
    pub end: String,
    pub state: String,
    pub steps: i64,
    pub active_ms: i64,
    pub kcal: Option<f64>,
    pub pad_calories: PadCalories,
    pub gap: bool,
    pub recovered_steps: i64,
    pub recovered_ms: i64,
    pub recovered_kcal: Option<f64>,
    pub recovery_kcal_missing: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Dashboard {
    pub day: Day,
    pub days: Vec<Day>,
    pub sessions: Vec<Session>,
    pub current: Option<Session>,
    pub today: String,
    pub timezone: String,
}
/// Longest history window one dashboard request may read.
pub const MAX_RANGE: u32 = 90;
#[derive(Clone, Debug)]
pub struct Request {
    pub day: String,
    pub range: u32,
}
