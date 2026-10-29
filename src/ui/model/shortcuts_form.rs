//! Shortcut form validation with the messages of the old Shortcuts window.
//! Start/Resume has no shortcut; only the four fields below exist.
use crate::shortcuts::{self, Shortcuts};

pub const LABELS: [&str; 4] = [
    "Pause",
    "Increase speed",
    "Decrease speed",
    "Show/hide compact controls",
];
pub const HELP: &str = "Leave blank to disable. Example: Ctrl+Alt+P. Use at least two modifiers. Start/Resume has no shortcut.";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Errors {
    pub fields: [Option<String>; 4],
    /// Problems between fields, such as two identical combinations.
    pub general: Option<String>,
}
pub fn check(bindings: [String; 4]) -> Result<Shortcuts, Errors> {
    let config = Shortcuts { bindings };
    let mut errors = Errors::default();
    for (i, binding) in config.bindings.iter().enumerate() {
        errors.fields[i] = shortcuts::parse(binding).err();
    }
    if errors.fields.iter().all(Option::is_none) {
        errors.general = config.validate().err();
    }
    if errors == Errors::default() {
        Ok(config)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b(a: &str, b: &str, c: &str, d: &str) -> [String; 4] {
        [a.into(), b.into(), c.into(), d.into()]
    }
    #[test]
    fn blank_and_distinct_shortcuts_are_valid() {
        assert!(check(b("", "", "", "")).is_ok());
        let ok = check(b(
            "Ctrl+Alt+P",
            "Ctrl+Alt+Up",
            "Ctrl+Alt+Down",
            "Ctrl+Alt+C",
        ))
        .unwrap();
        assert_eq!(ok.bindings[0], "Ctrl+Alt+P");
    }
    #[test]
    fn a_bad_field_is_reported_on_that_field_with_the_classic_message() {
        let e = check(b("", "P", "", "Ctrl+Alt+F12")).unwrap_err();
        assert_eq!(e.fields[0], None);
        assert_eq!(
            e.fields[1].as_deref(),
            Some("Shortcuts require at least two of Ctrl, Alt and Shift")
        );
        assert_eq!(e.fields[3].as_deref(), Some("F12 is reserved by Windows"));
        assert_eq!(e.general, None);
    }
    #[test]
    fn duplicate_combinations_are_a_general_error() {
        let e = check(b("Ctrl+Alt+P", "ctrl+alt+p", "", "")).unwrap_err();
        assert_eq!(e.fields, [None, None, None, None]);
        assert_eq!(
            e.general.as_deref(),
            Some("Each shortcut must have a different combination")
        );
    }
    #[test]
    fn there_is_no_start_resume_field_and_help_says_so() {
        assert_eq!(LABELS.len(), 4);
        assert!(LABELS.iter().all(|l| !l.contains("Start")));
        assert!(HELP.contains("Start/Resume has no shortcut"));
    }
}
