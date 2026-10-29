//! Persisted appearance choice, independent of the Windows UI palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ThemePreference {
    #[default]
    Automatic,
    Light,
    Dark,
}
impl ThemePreference {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::Light, Self::Dark];

    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::Automatic,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_choices_round_trip_and_missing_or_unknown_settings_follow_windows() {
        for preference in ThemePreference::ALL {
            assert_eq!(
                ThemePreference::from_setting(Some(preference.as_str())),
                preference
            );
        }
        for value in [None, Some(""), Some("unknown")] {
            assert_eq!(
                ThemePreference::from_setting(value),
                ThemePreference::Automatic
            );
        }
    }
}
