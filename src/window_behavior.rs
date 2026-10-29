//! Window preferences shared by the main window and compact remote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowBehavior {
    pub start_minimized: bool,
    pub minimize_to_tray: bool,
    pub keep_running_on_close: bool,
}
impl Default for WindowBehavior {
    fn default() -> Self {
        Self {
            start_minimized: true,
            minimize_to_tray: false,
            keep_running_on_close: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Preference {
    StartMinimized(bool),
    MinimizeToTray(bool),
    KeepRunningOnClose(bool),
}
impl Preference {
    pub fn setting(self) -> (&'static str, bool) {
        match self {
            Self::StartMinimized(value) => ("start_minimized", value),
            Self::MinimizeToTray(value) => ("minimize_to_tray", value),
            Self::KeepRunningOnClose(value) => ("keep_running_on_close", value),
        }
    }
    pub fn apply(self, behavior: &mut WindowBehavior) {
        match self {
            Self::StartMinimized(value) => behavior.start_minimized = value,
            Self::MinimizeToTray(value) => behavior.minimize_to_tray = value,
            Self::KeepRunningOnClose(value) => behavior.keep_running_on_close = value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Startup {
    Open,
    Minimized,
    Tray,
}
impl WindowBehavior {
    pub fn startup(self, first_run: bool, development: bool) -> Startup {
        if first_run || development || !self.start_minimized {
            Startup::Open
        } else if self.minimize_to_tray {
            Startup::Tray
        } else {
            Startup::Minimized
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_run_and_development_open_while_saved_launch_choices_use_the_right_destination() {
        for start_minimized in [false, true] {
            for minimize_to_tray in [false, true] {
                let behavior = WindowBehavior {
                    start_minimized,
                    minimize_to_tray,
                    ..Default::default()
                };
                assert_eq!(behavior.startup(true, false), Startup::Open);
                assert_eq!(behavior.startup(false, true), Startup::Open);
                assert_eq!(
                    behavior.startup(false, false),
                    if !start_minimized {
                        Startup::Open
                    } else if minimize_to_tray {
                        Startup::Tray
                    } else {
                        Startup::Minimized
                    }
                );
            }
        }
    }
}
