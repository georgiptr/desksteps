//! Application messages and published state.
use super::*;
#[derive(Clone, Debug)]
pub struct Candidate {
    pub address: u64,
    pub name: String,
}
#[derive(Clone, Debug, Default)]
pub struct View {
    pub window_behavior: crate::window_behavior::WindowBehavior,
    pub windows_startup: bool,
    pub windows_startup_available: bool,
    pub startup_error: Option<String>,
    pub theme_preference: crate::appearance::ThemePreference,
    pub development: bool,
    pub development_shortcuts: bool,
    pub simulator_status: String,
    pub status: String,
    pub selected: Option<u64>,
    pub selected_name: Option<String>,
    pub candidates: Vec<Candidate>,
    pub totals: Totals,
    pub last_sample: Option<String>,
    pub speed: Option<f64>,
    pub device_steps: Option<u32>,
    pub error: Option<String>,
    pub connected: bool,
    pub generation: u64,
    pub automatic_connection: bool,
    pub recover_gaps: bool,
    pub no_bluetooth: bool,
    pub controls_enabled: bool,
    pub control_busy: bool,
    pub speed_available: bool,
    pub selected_speed: Option<f64>,
    pub increment: u8,
    pub fresh_speed: Option<f64>,
    pub adjustable_speed: Option<f64>,
    pub control_uncertain: bool,
    pub shortcuts: crate::shortcuts::Shortcuts,
    pub shortcut_error: Option<String>,
    pub dashboard: Option<Arc<crate::statistics::Dashboard>>,
    pub ui_message: Option<String>,
    pub dashboard_visible: bool,
}
pub enum UserCommand {
    WindowsStartup(bool),
    WindowPreference(crate::window_behavior::Preference),
    Theme(crate::appearance::ThemePreference),
    Simulator(super::SimAction),
    Dashboard(Option<crate::statistics::Request>),
    SavePreferences(crate::statistics::Preferences),
    Control {
        command: control::Command,
        generation: u64,
    },
    Increment(u8),
    Shortcuts(crate::shortcuts::Shortcuts),
    AutomaticConnection(bool),
    RecoverGaps(bool),
    Disconnect,
    Select(u64),
    ActiveScan,
    Exit,
    DashboardVisible(bool),
    ShortcutStatus(Option<String>),
    ClearUiMessage,
}
pub(super) enum DeviceEvent {
    Found(Candidate, Instant),
    Lost(u64),
    ScanStopped {
        error: String,
        interrupted: bool,
    },
    Raw {
        generation: u64,
        device: u64,
        utc: DateTime<Utc>,
        mono: i64,
        kind: &'static str,
        uuid: String,
        bytes: Vec<u8>,
        detail: String,
    },
}

pub(super) enum Input {
    User(UserCommand),
    Device(DeviceEvent),
}
impl From<UserCommand> for Input {
    fn from(value: UserCommand) -> Self {
        Self::User(value)
    }
}
impl From<DeviceEvent> for Input {
    fn from(value: DeviceEvent) -> Self {
        Self::Device(value)
    }
}
