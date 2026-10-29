//! Optional native hotkeys. No Start/Resume action or keyboard hook.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shortcuts {
    pub bindings: [String; 4],
}
pub fn parse(value: &str) -> Result<Option<(u32, u32)>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let parts = value
        .split('+')
        .map(|p| p.trim().to_ascii_uppercase())
        .collect::<Vec<_>>();
    let mut modifiers: u32 = 0;
    for part in &parts[..parts.len() - 1] {
        let flag = match part.as_str() {
            "CTRL" => 2,
            "ALT" => 1,
            "SHIFT" => 4,
            _ => return Err("Use Ctrl, Alt and Shift modifiers".into()),
        };
        if modifiers & flag != 0 {
            return Err("Duplicate shortcut modifier".into());
        }
        modifiers |= flag;
    }
    if modifiers.count_ones() < 2 {
        return Err("Shortcuts require at least two of Ctrl, Alt and Shift".into());
    }
    let key = parts.last().unwrap();
    let vk = match key.as_str() {
        "UP" => 0x26,
        "DOWN" => 0x28,
        "LEFT" => 0x25,
        "RIGHT" => 0x27,
        "SPACE" => 0x20,
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => u32::from(k.as_bytes()[0]),
        k if k.starts_with('F') => k[1..]
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=24).contains(n))
            .map(|n| 0x6f + n)
            .ok_or("Use F1–F24")?,
        _ => return Err("Use a letter, digit, arrow, Space or F1–F24".into()),
    };
    // F12 is reserved by Windows for debugger use.
    if vk == 0x7b {
        return Err("F12 is reserved by Windows".into());
    }
    Ok(Some((modifiers, vk)))
}
impl Shortcuts {
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        for binding in &self.bindings {
            if let Some(value) = parse(binding)?
                && !seen.insert(value)
            {
                return Err("Each shortcut must have a different combination".into());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_valid_distinct_and_reserved_combinations() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("Ctrl+Alt+P").unwrap(), Some((3, 80)));
        assert_eq!(parse("shift + ctrl + Up").unwrap(), Some((6, 0x26)));
        assert_eq!(parse("Alt+Shift+Space").unwrap(), Some((5, 32)));
        assert_eq!(parse("Ctrl+Alt+F24").unwrap(), Some((3, 0x87)));
        assert_eq!(parse("Ctrl+Alt+9").unwrap(), Some((3, 57)));
        assert_eq!(parse("Alt+Shift+Left").unwrap(), Some((5, 37)));
        assert_eq!(parse("Alt+Shift+Right").unwrap(), Some((5, 39)));
        assert_eq!(parse("Alt+Shift+Down").unwrap(), Some((5, 40)));
        for bad in [
            "P",
            "Ctrl+P",
            "Win+Alt+P",
            "Ctrl+Ctrl+P",
            "Ctrl+Alt+F12",
            "Ctrl+Alt+F25",
            "Ctrl+Alt+Enter",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(Shortcuts::default().validate().is_ok());
        assert!(
            Shortcuts {
                bindings: [
                    "Ctrl+Alt+P".into(),
                    "ctrl+alt+p".into(),
                    String::new(),
                    String::new()
                ]
            }
            .validate()
            .is_err()
        );
    }
}
