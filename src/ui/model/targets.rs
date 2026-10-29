//! Targets form: parsing and per-field validation without Windows controls.
//! Messages are the ones the classic settings form gave.
use crate::statistics::{self, Preferences};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Date,
    Weight,
    Steps,
    Minutes,
    Calories,
}
impl Field {
    pub const ALL: [Field; 5] = [
        Field::Date,
        Field::Weight,
        Field::Steps,
        Field::Minutes,
        Field::Calories,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|f| *f == self).unwrap()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    pub field: Field,
    pub message: String,
}
pub fn number(value: &str) -> Result<Option<f64>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    value
        .trim()
        .parse()
        .map(Some)
        .map_err(|_| "Enter a number using a decimal point, or leave blank.".into())
}
fn steps(value: &str) -> Result<Option<i64>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    value
        .trim()
        .parse()
        .map(Some)
        .map_err(|_| "Step target must be a whole number.".to_string())
}
/// Checks every field and reports each failing one next to its field.
pub fn check(
    effective: &str,
    weight: &str,
    steps_text: &str,
    minutes: &str,
    calories: &str,
) -> Result<Preferences, Vec<FieldError>> {
    let mut errors = Vec::new();
    let mut fail = |field, message: String| errors.push(FieldError { field, message });
    let effective = effective.trim().to_string();
    if let Err(e) = statistics::date(&effective) {
        fail(Field::Date, e);
    }
    let weight = number(weight).unwrap_or_else(|e| {
        fail(Field::Weight, e);
        None
    });
    let steps = steps(steps_text).unwrap_or_else(|e| {
        fail(Field::Steps, e);
        None
    });
    let minutes = number(minutes).unwrap_or_else(|e| {
        fail(Field::Minutes, e);
        None
    });
    let calories = number(calories).unwrap_or_else(|e| {
        fail(Field::Calories, e);
        None
    });
    // Range rules are Preferences::validate's; apply each to its own field.
    let alone = |p: Preferences| {
        Preferences {
            effective: "2000-01-01".into(),
            ..p
        }
        .validate()
    };
    let mut ranges = Vec::new();
    for (field, p) in [
        (
            Field::Weight,
            Preferences {
                weight,
                ..Default::default()
            },
        ),
        (
            Field::Steps,
            Preferences {
                steps,
                ..Default::default()
            },
        ),
        (
            Field::Minutes,
            Preferences {
                minutes,
                ..Default::default()
            },
        ),
        (
            Field::Calories,
            Preferences {
                calories,
                ..Default::default()
            },
        ),
    ] {
        if let Err(e) = alone(p) {
            ranges.push(FieldError { field, message: e });
        }
    }
    for e in ranges {
        if !errors.iter().any(|x| x.field == e.field) {
            errors.push(e);
        }
    }
    if !errors.is_empty() {
        errors.sort_by_key(|e| e.field.index());
        return Err(errors);
    }
    let value = Preferences {
        effective,
        weight,
        steps,
        minutes,
        calories,
    };
    value.validate().map_err(|message| {
        vec![FieldError {
            field: Field::Date,
            message,
        }]
    })?;
    Ok(value)
}
pub fn confirmation(effective: &str) -> String {
    format!(
        "Apply weight and targets from {effective}?\n\nThis updates estimates and percentages from that date until the next dated settings. Earlier dates are unchanged."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ok(effective: &str, w: &str, s: &str, m: &str, c: &str) -> Preferences {
        check(effective, w, s, m, c).unwrap()
    }
    fn err(effective: &str, w: &str, s: &str, m: &str, c: &str) -> Vec<FieldError> {
        check(effective, w, s, m, c).unwrap_err()
    }
    #[test]
    fn blank_fields_disable_optional_preferences() {
        let p = ok("2026-10-17", " ", "", "", " ");
        assert_eq!(
            (p.weight, p.steps, p.minutes, p.calories),
            (None, None, None, None)
        );
    }
    #[test]
    fn valid_fields_are_trimmed_and_typed() {
        let p = ok(" 2026-10-17 ", " 80.5 ", " 5000 ", "30", "100");
        assert_eq!(p.effective, "2026-10-17");
        assert_eq!(p.weight, Some(80.5));
        assert_eq!(p.steps, Some(5000));
        assert_eq!(p.minutes, Some(30.0));
        assert_eq!(p.calories, Some(100.0));
    }
    #[test]
    fn invalid_dates_numbers_and_fractional_steps_have_useful_errors() {
        let e = err("bad", "", "", "", "");
        assert_eq!(e[0].field, Field::Date);
        assert!(e[0].message.contains("date"));
        let e = err("2026-10-17", "bad", "", "", "");
        assert_eq!(e[0].field, Field::Weight);
        assert!(e[0].message.contains("number"));
        let e = err("2026-10-17", "", "1.5", "", "");
        assert_eq!(e[0].field, Field::Steps);
        assert!(e[0].message.contains("whole number"));
        let e = err("2026-10-7", "", "", "", "");
        assert!(e[0].message.contains("YYYY-MM-DD"));
    }
    #[test]
    fn non_finite_out_of_range_and_nonpositive_values_are_rejected() {
        for value in ["NaN", "inf", "-inf", "19", "150.001", "151", "400"] {
            let e = err("2026-10-17", value, "", "", "");
            assert_eq!(e[0].field, Field::Weight, "{value}");
        }
        for value in ["NaN", "inf", "0", "-1", "100001"] {
            assert_eq!(
                err("2026-10-17", "", "", value, "")[0].field,
                Field::Minutes
            );
            assert_eq!(
                err("2026-10-17", "", "", "", value)[0].field,
                Field::Calories
            );
        }
        for value in ["0", "-1", "1000001"] {
            assert_eq!(err("2026-10-17", "", value, "", "")[0].field, Field::Steps);
        }
    }
    #[test]
    fn weight_endpoints_are_inclusive_and_blank_stays_optional() {
        for (input, expected) in [("20", Some(20.0)), ("150", Some(150.0)), ("", None)] {
            assert_eq!(
                check("2026-10-25", input, "", "", "").unwrap().weight,
                expected
            );
        }
    }
    #[test]
    fn range_messages_match_the_classic_form_text() {
        assert_eq!(
            err("2026-10-17", "19", "", "", "")[0].message,
            "Weight must be between 20 and 150 kg, or blank."
        );
        assert_eq!(
            err("2026-10-17", "", "0", "", "")[0].message,
            "Step target must be positive, or blank to disable it."
        );
        assert_eq!(
            err("2026-10-17", "", "", "0", "")[0].message,
            "Minute and calorie targets must be positive, or blank to disable them."
        );
    }
    #[test]
    fn every_failing_field_is_reported_once_in_field_order() {
        let e = err("bad", "x", "1.5", "0", "-2");
        let fields: Vec<_> = e.iter().map(|e| e.field).collect();
        assert_eq!(fields, Field::ALL.to_vec());
    }
    #[test]
    fn confirmation_names_the_effective_date() {
        let text = confirmation("2026-10-17");
        assert!(text.contains("from 2026-10-17?") && text.contains("Earlier dates are unchanged."));
    }
}
