//! Page content as positioned blocks in content space (origin at the body's top-left).
//! The same block list drives painting and hit targets; this module is pure.
use crate::{
    application::View,
    ui::{
        kit::{
            chart,
            input::Id,
            layout::{Breakpoint, Rect, breakpoint, columns},
            widgets::ButtonKind,
        },
        model::{
            day::{DayView, HistoryView},
            device,
            live::START_HINT,
            page::Page,
            shortcuts_form,
        },
    },
};

pub const TILE_HEIGHT: f32 = 124.0;
pub const COMPACT_TILE_HEIGHT: f32 = 56.0;
pub const ROW_HEIGHT: f32 = 32.0;
pub const HEAD_HEIGHT: f32 = 24.0;
pub const SESSION_ROW: f32 = 54.0;
pub const SESSION_ROW_GAP: f32 = 70.0;
pub const TIMELINE_HEIGHT: f32 = 44.0;
pub const SETTINGS_ROW: f32 = 36.0;
const GAP: f32 = 12.0;

pub const PREV: Id = 40;
pub const NEXT: Id = 41;
pub const DATE: Id = 42;
pub const TODAY_BUTTON: Id = 43;
pub const BACK: Id = 44;
pub const RANGE_BASE: Id = 50;
pub const RANGES: [u32; 3] = [7, 30, 90];
pub const EFFECTIVE: Id = 45;
pub const TARGETS_SAVE: Id = 46;
pub const SHORTCUTS_SAVE: Id = 47;
pub const AUTO: Id = 70;
pub const SCAN: Id = 71;
pub const DISCONNECT: Id = 72;
pub const FOLDER: Id = 73;
pub const RECOVER_GAPS: Id = 74;
pub const THEME_BASE: Id = 75;
pub const WINDOWS_STARTUP: Id = 82;
pub const MINIMIZE_TO_TRAY: Id = 83;
pub const KEEP_RUNNING_ON_CLOSE: Id = 84;
pub const START_MINIMIZED: Id = 85;
pub const STEP_DOWN: Id = 80;
pub const STEP_UP: Id = 81;
pub const ROW_BASE: Id = 1000;
pub const BAR_BASE: Id = 2000;
/// Native EDIT fields: 3000-3003 on Targets, 3010-3013 shortcuts on Settings.
pub const FIELD_BASE: Id = 3000;
pub const CANDIDATE_BASE: Id = 3100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Today,
    History,
    HistoryDay,
    Targets,
    Settings,
}
impl Screen {
    pub fn of(page: Page, detail: bool) -> Self {
        match page {
            Page::Today => Self::Today,
            Page::History if detail => Self::HistoryDay,
            Page::History => Self::History,
            Page::Targets => Self::Targets,
            Page::Settings => Self::Settings,
        }
    }
    /// Each screen keeps its own scroll position.
    pub fn slot(self) -> usize {
        match self {
            Self::Today => 0,
            Self::History => 1,
            Self::Targets => 2,
            Self::Settings => 3,
            Self::HistoryDay => 4,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Steps,
    Minutes,
    Kcal,
    Coverage,
    Date,
    Sessions,
    Target,
}
impl Field {
    pub fn title(self) -> &'static str {
        match self {
            Self::Steps => "Steps",
            Self::Minutes => "Min",
            Self::Kcal => "kcal",
            Self::Coverage => "Coverage",
            Self::Date => "Date",
            Self::Sessions => "Sessions",
            Self::Target => "Target",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Col {
    pub field: Field,
    pub x: f32,
    pub w: f32,
    pub right: bool,
}
/// Fixed columns take their widths; the flexible first/last column takes the rest.
fn columns_for(spec: &[(Field, f32, bool)], width: f32, flex: Field) -> Vec<Col> {
    let fixed: f32 = spec.iter().filter(|c| c.0 != flex).map(|c| c.1).sum();
    let mut x = 12.0;
    let inner = width - 24.0;
    spec.iter()
        .map(|(field, w, right)| {
            let w = if *field == flex {
                (inner - fixed).max(60.0)
            } else {
                *w
            };
            let col = Col {
                field: *field,
                x,
                w,
                right: *right,
            };
            x += w;
            col
        })
        .collect()
}
/// Row height: rows with a recording gap carry a third line.
pub fn session_row_height(gap: bool) -> f32 {
    if gap { SESSION_ROW_GAP } else { SESSION_ROW }
}
/// Horizontal zones of a session row as (x, width) relative to the row, at every width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionZones {
    pub left: (f32, f32),
    pub right: (f32, f32),
}
pub fn session_zones(width: f32) -> SessionZones {
    let right = (width - 12.0 - 140.0, 140.0);
    SessionZones {
        left: (12.0, right.0 - 8.0 - 12.0),
        right,
    }
}
/// Track inside a timeline block; bars and hour labels are laid out against it.
pub fn timeline_track(block: Rect) -> Rect {
    Rect::new(block.x + 12.0, block.y + 6.0, block.w - 24.0, 18.0)
}
pub fn history_cols(width: f32) -> Vec<Col> {
    if width >= 540.0 {
        columns_for(
            &[
                (Field::Date, 92.0, false),
                (Field::Steps, 64.0, true),
                (Field::Minutes, 56.0, true),
                (Field::Kcal, 56.0, true),
                (Field::Sessions, 72.0, true),
                (Field::Target, 64.0, true),
                (Field::Coverage, 0.0, false),
            ],
            width,
            Field::Coverage,
        )
    } else {
        columns_for(
            &[
                (Field::Date, 92.0, false),
                (Field::Steps, 64.0, true),
                (Field::Target, 64.0, true),
                (Field::Coverage, 0.0, false),
            ],
            width,
            Field::Coverage,
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Tile {
        index: usize,
        compact: bool,
    },
    Section(&'static str),
    /// Secondary-colour paragraph.
    Note(String),
    Row {
        label: &'static str,
        value: String,
        divider: bool,
    },
    /// Card behind a list or group of rows.
    Panel,
    Button {
        id: Id,
        label: String,
        kind: ButtonKind,
    },
    IconButton {
        id: Id,
        glyph: char,
    },
    /// Right-aligned secondary text on a section's line.
    Summary(String),
    Timeline,
    Session {
        index: usize,
        divider: bool,
    },
    Chart,
    /// Switch row for a yes/no setting.
    Toggle {
        id: Id,
        label: String,
        on: bool,
    },
    /// Row of a device list.
    Candidate {
        id: Id,
        index: usize,
        divider: bool,
    },
    /// A value with a decrease and an increase button.
    Stepper {
        down: Id,
        up: Id,
        value: String,
    },
    /// Plain body-size label.
    Label(String),
    /// Red inline message.
    Error(String),
    /// Label, framed native EDIT, optional caption and error.
    Field {
        id: Id,
        label: String,
        caption: Option<String>,
        error: Option<String>,
    },
    DayHead,
    DayRow {
        id: Id,
        index: usize,
        divider: bool,
    },
}
/// Text and numbers the pages show, derived from published state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageData {
    pub window_behavior: crate::window_behavior::WindowBehavior,
    pub windows_startup: bool,
    pub windows_startup_available: bool,
    pub startup_error: Option<String>,
    pub theme_preference: crate::appearance::ThemePreference,
    pub connection: String,
    pub device: String,
    pub step: String,
    pub version: String,
    pub day: Option<DayView>,
    pub history: Option<HistoryView>,
    pub range: u32,
    pub increment: u8,
    pub recover_gaps: bool,
    pub devices: Devices,
    pub form: Form,
    /// Targets that are shown but cannot be used right now.
    pub disabled: Vec<Id>,
    pub development_note: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Devices {
    pub available: bool,
    pub automatic: bool,
    pub connected: bool,
    /// Address, label, saved default.
    pub candidates: Vec<(u64, String, bool)>,
}
/// Form state kept by the window: inline errors and the last result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    pub effective: String,
    pub errors: [Option<String>; 5],
    pub message: Option<String>,
    pub shortcut_errors: [Option<String>; 4],
    pub shortcut_general: Option<String>,
    pub shortcuts_saved: bool,
}
impl PageData {
    pub fn from_view(v: &View) -> Self {
        Self {
            window_behavior: v.window_behavior,
            windows_startup: v.windows_startup,
            windows_startup_available: v.windows_startup_available,
            startup_error: v.startup_error.clone(),
            theme_preference: v.theme_preference,
            connection: v.status.clone(),
            device: v
                .selected
                .map(crate::address)
                .unwrap_or_else(|| "none selected".into()),
            step: format!("{:.1} km/h", v.increment.max(1) as f64 / 10.0),
            version: env!("CARGO_PKG_VERSION").into(),
            range: 30,
            increment: v.increment.max(1),
            recover_gaps: v.recover_gaps,
            devices: Devices {
                available: device::available(v),
                automatic: v.automatic_connection,
                connected: v.connected,
                candidates: device::choices(v)
                    .into_iter()
                    .map(|(a, label)| (a, label, v.selected == Some(a)))
                    .collect(),
            },
            development_note: v.development && !v.development_shortcuts,
            disabled: {
                let mut off = Vec::new();
                if !v.windows_startup_available {
                    off.push(WINDOWS_STARTUP);
                }
                if v.increment.max(1) <= 1 {
                    off.push(STEP_DOWN);
                }
                if v.increment >= 10 {
                    off.push(STEP_UP);
                }
                if !device::available(v) {
                    off.extend([AUTO, SCAN, DISCONNECT]);
                    off.extend((0..v.candidates.len() as Id + 1).map(|i| CANDIDATE_BASE + i));
                }
                if v.connected {
                    off.push(SCAN);
                }
                off
            },
            ..Default::default()
        }
    }
}
pub fn note_height(text: &str, width: f32) -> f32 {
    // Rough wrap estimate: 12 px captions average 6.2 px per character.
    let lines = ((text.chars().count() as f32 * 6.2) / width.max(1.0))
        .ceil()
        .max(1.0);
    lines * 16.0 + 4.0
}
struct Builder {
    out: Vec<(Rect, Block)>,
}
impl Builder {
    fn put(&mut self, rect: Rect, block: Block) {
        self.out.push((rect, block));
    }
    fn note(&mut self, x: f32, y: f32, width: f32, text: &str) -> f32 {
        let h = note_height(text, width);
        self.put(Rect::new(x, y, width, h), Block::Note(text.into()));
        y + h + GAP
    }
    fn error(&mut self, x: f32, y: f32, width: f32, text: &str) -> f32 {
        let h = note_height(text, width);
        self.put(Rect::new(x, y, width, h), Block::Error(text.into()));
        y + h + 4.0
    }
    fn section(&mut self, x: f32, y: f32, width: f32, title: &'static str) -> f32 {
        self.put(Rect::new(x, y, width, 24.0), Block::Section(title));
        y + 28.0
    }
}
/// Blocks and total content height for `screen` in a body `width` wide.
pub fn layout(screen: Screen, width: f32, data: &PageData) -> (Vec<(Rect, Block)>, f32) {
    let mut b = Builder { out: Vec::new() };
    let y = match screen {
        Screen::Today | Screen::HistoryDay => day_screen(&mut b, screen, width, data),
        Screen::History => history_screen(&mut b, width, data),
        Screen::Targets => targets_screen(&mut b, width, data),
        Screen::Settings => settings_screen(&mut b, width, data),
    };
    (b.out, (y - GAP).max(0.0))
}
fn loading(b: &mut Builder, y: f32, width: f32) -> f32 {
    b.note(0.0, y, width, "Loading statistics…")
}
fn day_screen(b: &mut Builder, screen: Screen, width: f32, data: &PageData) -> f32 {
    let mut y = 0.0;
    let day = data.day.as_ref();
    if screen == Screen::HistoryDay {
        b.put(
            Rect::new(0.0, y, 150.0, 32.0),
            Block::Button {
                id: BACK,
                label: "‹ Back to History".into(),
                kind: ButtonKind::Standard,
            },
        );
    } else {
        b.put(
            Rect::new(0.0, y, 32.0, 32.0),
            Block::IconButton {
                id: PREV,
                glyph: '\u{E76B}',
            },
        );
        b.put(
            Rect::new(36.0, y, 32.0, 32.0),
            Block::IconButton {
                id: NEXT,
                glyph: '\u{E76C}',
            },
        );
        b.put(
            Rect::new(72.0, y, 148.0, 32.0),
            Block::Button {
                id: DATE,
                label: day
                    .map(|d| d.date.clone())
                    .unwrap_or_else(|| "Select date".into()),
                kind: ButtonKind::Standard,
            },
        );
        if day.is_some_and(|d| !d.is_today) {
            b.put(
                Rect::new(228.0, y, 132.0, 32.0),
                Block::Button {
                    id: TODAY_BUTTON,
                    label: "Back to today".into(),
                    kind: ButtonKind::Standard,
                },
            );
        }
    }
    y += 32.0 + GAP;
    let Some(day) = day else {
        return loading(b, y, width);
    };
    let narrow = breakpoint(width + 48.0) == Breakpoint::Narrow;
    let wide = breakpoint(width + 48.0) == Breakpoint::Wide;
    let (left_w, right_x) = if wide { (280.0, 296.0) } else { (width, 0.0) };
    let right_w = if wide { width - right_x } else { width };
    // Tiles.
    let mut ty = y;
    if narrow {
        for i in 0..3 {
            b.put(
                Rect::new(0.0, ty, width, COMPACT_TILE_HEIGHT),
                Block::Tile {
                    index: i,
                    compact: true,
                },
            );
            ty += COMPACT_TILE_HEIGHT + 8.0;
        }
        ty += GAP - 8.0;
    } else if wide {
        for i in 0..3 {
            b.put(
                Rect::new(0.0, ty, left_w, TILE_HEIGHT),
                Block::Tile {
                    index: i,
                    compact: false,
                },
            );
            ty += TILE_HEIGHT + GAP;
        }
    } else {
        for (i, rect) in columns(Rect::new(0.0, ty, width, TILE_HEIGHT), 3, GAP)
            .into_iter()
            .enumerate()
        {
            b.put(
                rect,
                Block::Tile {
                    index: i,
                    compact: false,
                },
            );
        }
        ty += TILE_HEIGHT + GAP;
    }
    let (rx, rw) = if wide {
        (right_x, right_w)
    } else {
        (0.0, width)
    };
    let mut ry = if wide { y } else { ty };
    if let Some(recovery) = &day.recovery {
        ry = b.note(rx, ry, rw, recovery);
    }
    let head_y = ry;
    ry = b.section(rx, ry, rw, "Sessions");
    if day.sessions.is_empty() {
        ry = b.note(rx, ry, rw, "No sessions recorded for this day.");
    } else {
        b.put(
            Rect::new(rx + 88.0, head_y, rw - 88.0, 24.0),
            Block::Summary(day.summary.clone()),
        );
        let n = day.sessions.len();
        let top = if day.timeline.is_some() {
            TIMELINE_HEIGHT
        } else {
            0.0
        };
        let heights: Vec<f32> = day
            .sessions
            .iter()
            .map(|r| session_row_height(r.gap))
            .collect();
        let total = top + heights.iter().sum::<f32>();
        b.put(Rect::new(rx, ry, rw, total), Block::Panel);
        if top > 0.0 {
            b.put(Rect::new(rx, ry, rw, top), Block::Timeline);
        }
        let mut row_y = ry + top;
        for (i, h) in heights.iter().enumerate() {
            b.put(
                Rect::new(rx, row_y, rw, *h),
                Block::Session {
                    index: i,
                    divider: i + 1 < n,
                },
            );
            row_y += h;
        }
        ry += total + GAP;
        if day.has_gap {
            ry = b.note(
                rx,
                ry,
                rw,
                "Recovered steps are estimates and are already included in totals.",
            );
        }
    }
    if let Some(legacy) = &day.legacy {
        ry = b.note(rx, ry, rw, legacy);
    }
    if wide { ry.max(ty) } else { ry }
}
fn history_screen(b: &mut Builder, width: f32, data: &PageData) -> f32 {
    let mut y = 0.0;
    for (i, days) in RANGES.iter().enumerate() {
        b.put(
            Rect::new(i as f32 * 96.0, y, 90.0, 32.0),
            Block::Button {
                id: RANGE_BASE + i as Id,
                label: format!("{days} days"),
                kind: if *days == data.range {
                    ButtonKind::Selected
                } else {
                    ButtonKind::Standard
                },
            },
        );
    }
    y += 32.0 + GAP;
    let Some(history) = &data.history else {
        return loading(b, y, width);
    };
    let wide = breakpoint(width + 48.0) == Breakpoint::Wide;
    let n = history.rows.len();
    let list_h = HEAD_HEIGHT + ROW_HEIGHT * n as f32;
    let list = |b: &mut Builder, x: f32, y: f32, w: f32| -> f32 {
        let mut y = b.section(x, y, w, "Days");
        if n == 0 {
            return b.note(x, y, w, "No days in this range.");
        }
        b.put(Rect::new(x, y, w, list_h), Block::Panel);
        b.put(Rect::new(x, y, w, HEAD_HEIGHT), Block::DayHead);
        for i in 0..n {
            b.put(
                Rect::new(x, y + HEAD_HEIGHT + ROW_HEIGHT * i as f32, w, ROW_HEIGHT),
                Block::DayRow {
                    id: ROW_BASE + i as Id,
                    index: i,
                    divider: i + 1 < n,
                },
            );
        }
        y += list_h + GAP;
        y
    };
    let legend = "Grey bars are partial or uncertain days. The line marks the step target. Click a bar or double-click a day to open it.";
    if wide {
        let cw = ((width - GAP) * 0.45).floor();
        let mut cy = b.section(0.0, y, cw, "Steps per day");
        b.put(Rect::new(0.0, cy, cw, 300.0), Block::Chart);
        cy += 300.0 + 8.0;
        cy = b.note(0.0, cy, cw, legend);
        let ly = list(b, cw + GAP, y, width - cw - GAP);
        ly.max(cy)
    } else {
        y = b.section(0.0, y, width, "Steps per day");
        let h = if width < 512.0 { 190.0 } else { 230.0 };
        b.put(Rect::new(0.0, y, width, h), Block::Chart);
        y += h + 8.0;
        y = b.note(0.0, y, width, legend);
        list(b, 0.0, y, width)
    }
}
/// Label, input frame and wrapped caption/error, with space before the next field.
pub fn field_height(caption: Option<&str>, error: Option<&str>, width: f32) -> f32 {
    20.0 + 32.0
        + caption.map_or(0.0, |text| note_height(text, width))
        + error.map_or(0.0, |text| note_height(text, width))
        + 8.0
}
/// The framed input area inside a `Block::Field` rectangle.
pub fn field_frame(block: Rect) -> Rect {
    Rect::new(block.x, block.y + 20.0, block.w, 32.0)
}
/// Where the native EDIT control sits inside a field.
pub fn field_edit(block: Rect) -> Rect {
    field_frame(block).inset_xy(8.0, 6.0)
}
fn field(
    b: &mut Builder,
    y: f32,
    width: f32,
    id: Id,
    label: &str,
    caption: Option<&str>,
    error: Option<&String>,
) -> f32 {
    let w = width.min(360.0);
    let h = field_height(caption, error.map(String::as_str), w);
    b.put(
        Rect::new(0.0, y, w, h),
        Block::Field {
            id,
            label: label.into(),
            caption: caption.map(String::from),
            error: error.cloned(),
        },
    );
    y + h
}
/// Buttons side by side, wrapping to new rows; returns the y below the last row.
fn flow(b: &mut Builder, mut y: f32, width: f32, items: Vec<(Id, String, ButtonKind, f32)>) -> f32 {
    let mut x = 0.0;
    for (id, label, kind, w) in items {
        let w = w.min(width);
        if x > 0.0 && x + w > width {
            x = 0.0;
            y += 32.0 + 8.0;
        }
        b.put(Rect::new(x, y, w, 32.0), Block::Button { id, label, kind });
        x += w + 8.0;
    }
    y + 32.0 + GAP
}
fn targets_screen(b: &mut Builder, width: f32, data: &PageData) -> f32 {
    let f = &data.form;
    let mut y = 0.0;
    b.put(
        Rect::new(0.0, y, width, 18.0),
        Block::Label("Effective from".into()),
    );
    y += 20.0;
    b.put(
        Rect::new(0.0, y, 148.0, 32.0),
        Block::Button {
            id: EFFECTIVE,
            label: f.effective.clone(),
            kind: ButtonKind::Standard,
        },
    );
    y += 32.0 + 4.0;
    if let Some(e) = &f.errors[0] {
        y = b.error(0.0, y, width, e);
    }
    y = b.note(
        0.0,
        y,
        width,
        "Applies from this date until the next dated settings.",
    );
    let captions = [
        (
            "Weight (kg)",
            "Optional, 20 to 150. Blank uses pad calories.",
        ),
        (
            "Step target",
            "Whole steps per day, or blank to turn the target off.",
        ),
        (
            "Walking minutes target",
            "Minutes per day, or blank to turn the target off.",
        ),
        (
            "Calorie target",
            "Uses the displayed calorie source. Blank turns the target off.",
        ),
    ];
    for (i, (label, caption)) in captions.iter().enumerate() {
        y = field(
            b,
            y,
            width,
            FIELD_BASE + i as Id,
            label,
            Some(caption),
            f.errors[i + 1].as_ref(),
        );
    }
    y += 4.0;
    b.put(
        Rect::new(0.0, y, 120.0, 32.0),
        Block::Button {
            id: TARGETS_SAVE,
            label: "Save".into(),
            kind: ButtonKind::Accent,
        },
    );
    y += 32.0 + GAP;
    if let Some(message) = &f.message {
        y = b.note(0.0, y, width, message);
    }
    y
}
fn settings_screen(b: &mut Builder, width: f32, data: &PageData) -> f32 {
    let mut y = 0.0;
    y = b.section(0.0, y, width, "Startup and tray");
    for (id, label, on) in [
        (WINDOWS_STARTUP, "Start with Windows", data.windows_startup),
        (
            START_MINIMIZED,
            "Start minimized",
            data.window_behavior.start_minimized,
        ),
        (
            MINIMIZE_TO_TRAY,
            "Minimize to tray",
            data.window_behavior.minimize_to_tray,
        ),
        (
            KEEP_RUNNING_ON_CLOSE,
            "Keep running when closed",
            data.window_behavior.keep_running_on_close,
        ),
    ] {
        b.put(
            Rect::new(0.0, y, width, 32.0),
            Block::Toggle {
                id,
                label: label.into(),
                on,
            },
        );
        y += 32.0 + 8.0;
    }
    if let Some(error) = &data.startup_error {
        y = b.error(0.0, y, width, error);
    }
    y = b.section(0.0, y, width, "Calories");
    y = b.note(0.0, y, width, "Without weight, calories come from your pad. With weight, DeskSteps estimates active calories; values may differ. Pad readings are always saved.");
    y = b.section(0.0, y, width, "Appearance");
    y = flow(
        b,
        y,
        width,
        crate::appearance::ThemePreference::ALL
            .into_iter()
            .enumerate()
            .map(|(i, preference)| {
                (
                    THEME_BASE + i as Id,
                    preference.label().into(),
                    if data.theme_preference == preference {
                        ButtonKind::Selected
                    } else {
                        ButtonKind::Standard
                    },
                    if i == 0 { 120.0 } else { 88.0 },
                )
            })
            .collect(),
    );
    y = b.note(0.0, y, width, "Automatic follows the Windows app theme. Changes apply immediately. Windows high contrast takes priority.");
    // Device.
    y = b.section(0.0, y, width, "Device");
    let rows = [
        ("Status", data.connection.clone()),
        ("Selected device", data.device.clone()),
    ];
    y = panel_rows(b, y, width, &rows);
    let d = &data.devices;
    b.put(
        Rect::new(0.0, y, width.min(360.0), 32.0),
        Block::Toggle {
            id: AUTO,
            label: "Connect automatically".into(),
            on: d.automatic,
        },
    );
    y += 32.0 + 8.0;
    let mut buttons = vec![(SCAN, "Scan".into(), ButtonKind::Standard, 80.0)];
    if d.connected {
        buttons.push((DISCONNECT, "Disconnect".into(), ButtonKind::Standard, 110.0));
    }
    y = flow(b, y, width, buttons);
    if !d.available {
        y = b.note(
            0.0,
            y,
            width,
            "Bluetooth is off in this mode, so connection commands are unavailable.",
        );
    } else if d.candidates.is_empty() {
        y = b.note(
            0.0,
            y,
            width,
            "No devices found yet. Scan looks for a nearby WalkingPad for 10 seconds.",
        );
    } else {
        y = b.note(0.0, y, width, "Choose a device to save as the default.");
        let n = d.candidates.len();
        b.put(Rect::new(0.0, y, width, 36.0 * n as f32), Block::Panel);
        for i in 0..n {
            b.put(
                Rect::new(0.0, y + 36.0 * i as f32, width, 36.0),
                Block::Candidate {
                    id: CANDIDATE_BASE + i as Id,
                    index: i,
                    divider: i + 1 < n,
                },
            );
        }
        y += 36.0 * n as f32 + GAP;
    }
    y = b.section(0.0, y, width, "Activity recovery");
    b.put(
        Rect::new(0.0, y, width, 32.0),
        Block::Toggle {
            id: RECOVER_GAPS,
            label: "Recover activity after gaps".into(),
            on: data.recover_gaps,
        },
    );
    y += 40.0;
    y = b.note(0.0, y, width, "On by default. Add steps and estimated walking time from retained pad counters after closing the app or losing data. Estimate active kcal when distance, time and weight are available. Empty-belt time may be included. Applies to future gaps; earlier totals stay unchanged. Gaps crossing midnight go to the reconnect date.");
    // Controls.
    y = b.section(0.0, y, width, "Controls");
    b.put(
        Rect::new(0.0, y, width, 18.0),
        Block::Label("Speed step for the − and + buttons".into()),
    );
    y += 22.0;
    b.put(
        Rect::new(0.0, y, 220.0_f32.min(width), 32.0),
        Block::Stepper {
            down: STEP_DOWN,
            up: STEP_UP,
            value: format!("{:.1} km/h", data.increment as f32 / 10.0),
        },
    );
    y += 32.0 + GAP;
    y = b.note(0.0, y, width, START_HINT);
    // Shortcuts.
    y = b.section(0.0, y, width, "Shortcuts");
    y = b.note(0.0, y, width, shortcuts_form::HELP);
    if data.development_note {
        y = b.note(
            0.0,
            y,
            width,
            "In development mode shortcuts stay off unless enabled in the simulator panel.",
        );
    }
    for (i, label) in shortcuts_form::LABELS.iter().enumerate() {
        y = field(
            b,
            y,
            width,
            FIELD_BASE + 10 + i as Id,
            label,
            None,
            data.form.shortcut_errors[i].as_ref(),
        );
    }
    y += 4.0;
    b.put(
        Rect::new(0.0, y, 160.0, 32.0),
        Block::Button {
            id: SHORTCUTS_SAVE,
            label: "Save shortcuts".into(),
            kind: ButtonKind::Accent,
        },
    );
    y += 32.0 + GAP;
    if let Some(e) = &data.form.shortcut_general {
        y = b.error(0.0, y, width, e);
    } else if data.form.shortcuts_saved {
        y = b.note(0.0, y, width, "Shortcuts sent to the recorder.");
    }
    // Data and diagnostics.
    y = b.section(0.0, y, width, "Data and diagnostics");
    y = b.note(
        0.0,
        y,
        width,
        "Raw device records are kept as received, with no automatic deletion. They live in the local database in the data folder.",
    );
    y = b.note(
        0.0,
        y,
        width,
        "Portable (no installer): data folder beside desksteps.exe. Normal install: %LOCALAPPDATA%\\DeskSteps. The button opens the folder in use.",
    );
    y = flow(
        b,
        y,
        width,
        vec![(
            FOLDER,
            "Open data folder".into(),
            ButtonKind::Standard,
            180.0,
        )],
    );
    // About.
    y = b.section(0.0, y, width, "About");
    panel_rows(b, y, width, &[("Version", data.version.clone())])
}
fn panel_rows(b: &mut Builder, y: f32, width: f32, items: &[(&'static str, String)]) -> f32 {
    let panel_h = SETTINGS_ROW * items.len() as f32;
    b.put(Rect::new(0.0, y, width, panel_h), Block::Panel);
    let n = items.len();
    for (i, (label, value)) in items.iter().enumerate() {
        b.put(
            Rect::new(0.0, y + SETTINGS_ROW * i as f32, width, SETTINGS_ROW),
            Block::Row {
                label,
                value: value.clone(),
                divider: i + 1 < n,
            },
        );
    }
    y + panel_h + GAP
}
/// The decrease and increase buttons of a stepper block.
pub fn stepper_buttons(block: Rect) -> (Rect, Rect) {
    (
        Rect::new(block.x, block.y, 32.0, block.h),
        Rect::new(block.right() - 32.0, block.y, 32.0, block.h),
    )
}
/// Inner chart area for a chart block.
pub fn chart_area(block: Rect) -> Rect {
    block.inset(8.0)
}
pub fn bar_slots(block: Rect, n: usize) -> Vec<Rect> {
    chart::slots(chart_area(block), n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statistics::{Dashboard, Day, Preferences, Session};
    fn dash(sessions: usize, days: usize) -> Dashboard {
        let mut d = Dashboard {
            day: Day {
                date: "2026-10-23".into(),
                steps: 700,
                samples: 3,
                preferences: Preferences {
                    steps: Some(1000),
                    ..Default::default()
                },
                ..Default::default()
            },
            today: "2026-10-23".into(),
            timezone: "UTC".into(),
            ..Default::default()
        };
        for i in 0..sessions {
            d.sessions.push(Session {
                id: i as i64 + 1,
                start: "2026-10-23T09:00:00Z".into(),
                end: "2026-10-23T09:10:00Z".into(),
                state: "Finished".into(),
                steps: 100,
                active_ms: 600_000,
                kcal: Some(3.0),
                gap: i % 2 == 1,
                ..Default::default()
            });
        }
        d.days = (0..days)
            .map(|i| Day {
                date: format!("2026-09-{:02}", i % 28 + 1),
                steps: i as i64 * 10,
                samples: 1,
                ..Default::default()
            })
            .collect();
        if sessions > 0 {
            d.current = d.sessions.first().cloned();
        }
        d
    }
    fn data(sessions: usize, days: usize) -> PageData {
        let d = dash(sessions, days);
        PageData {
            day: Some(DayView::new(&d)),
            history: Some(HistoryView::new(&d)),
            range: 30,
            ..PageData::from_view(&View::default())
        }
    }
    fn inside(width: f32, screen: Screen, d: &PageData) -> (Vec<(Rect, Block)>, f32) {
        let (blocks, height) = layout(screen, width, d);
        assert!(!blocks.is_empty());
        for (rect, block) in &blocks {
            assert!(
                rect.x >= 0.0 && rect.right() <= width + 0.01,
                "{screen:?} {width}: {block:?} {rect:?}"
            );
            assert!(
                rect.bottom() <= height + 0.01,
                "{screen:?} {width}: {block:?} {rect:?} > {height}"
            );
        }
        (blocks, height)
    }
    #[test]
    fn every_screen_fits_its_width_at_narrow_medium_and_wide() {
        for screen in [
            Screen::Today,
            Screen::History,
            Screen::HistoryDay,
            Screen::Targets,
            Screen::Settings,
        ] {
            for width in [408.0, 511.0, 512.0, 552.0, 700.0, 852.0, 853.0, 992.0] {
                inside(width, screen, &data(0, 0));
                inside(width, screen, &data(3, 7));
                inside(width, screen, &data(12, 90));
                inside(width, screen, &PageData::default());
            }
        }
    }
    fn tiles(width: f32) -> Vec<Rect> {
        layout(Screen::Today, width, &data(2, 7))
            .0
            .into_iter()
            .filter(|(_, b)| matches!(b, Block::Tile { .. }))
            .map(|(r, _)| r)
            .collect()
    }
    #[test]
    fn narrow_tiles_are_compact_rows_that_fit_a_small_window() {
        let t = tiles(408.0);
        assert_eq!(t.len(), 3);
        assert!(t.iter().all(|r| r.h == COMPACT_TILE_HEIGHT && r.w == 408.0));
        assert!(t[1].y > t[0].bottom() - 0.01);
        // At 520x420 the three tiles end well inside the 328 px body.
        assert!(t[2].bottom() + 12.0 + 32.0 + 12.0 < 328.0);
        assert!(t[2].bottom() < 32.0 + 12.0 + 3.0 * 64.0);
    }
    #[test]
    fn medium_tiles_share_a_row_and_wide_tiles_stack_left_of_the_sessions() {
        let m = tiles(552.0);
        assert!(m.iter().all(|r| r.y == m[0].y && r.h == TILE_HEIGHT));
        assert!(m[1].x > m[0].right());
        let w = tiles(900.0);
        assert!(w.iter().all(|r| r.x == 0.0 && r.w == 280.0));
        assert!(w[1].y > w[0].y);
        let blocks = layout(Screen::Today, 900.0, &data(2, 7)).0;
        let sessions = blocks
            .iter()
            .find(|(_, b)| *b == Block::Section("Sessions"))
            .unwrap()
            .0;
        assert!(sessions.x >= 296.0);
    }
    #[test]
    fn today_shows_date_navigation_and_back_to_today_only_for_other_days() {
        let mut d = data(0, 7);
        let ids = |d: &PageData| {
            layout(Screen::Today, 552.0, d)
                .0
                .into_iter()
                .filter_map(|(_, b)| match b {
                    Block::Button { id, .. } | Block::IconButton { id, .. } => Some(id),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&d), vec![PREV, NEXT, DATE]);
        d.day.as_mut().unwrap().is_today = false;
        assert_eq!(ids(&d), vec![PREV, NEXT, DATE, TODAY_BUTTON]);
        let detail = layout(Screen::HistoryDay, 552.0, &d).0;
        assert!(matches!(&detail[0].1, Block::Button { id, .. } if *id == BACK));
        assert!(
            detail
                .iter()
                .all(|(_, b)| !matches!(b, Block::IconButton { .. }))
        );
    }
    fn notes(blocks: &[(Rect, Block)]) -> Vec<&str> {
        blocks
            .iter()
            .filter_map(|(_, b)| match b {
                Block::Note(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn session_rows_sit_under_a_timeline_and_gap_rows_are_taller() {
        let none = layout(Screen::Today, 552.0, &data(0, 0)).0;
        assert!(none.iter().all(|(_, b)| !matches!(
            b,
            Block::Session { .. } | Block::Timeline | Block::Summary(_)
        )));
        assert!(notes(&none).iter().any(|t| t.contains("No sessions")));
        for width in [408.0, 552.0, 900.0] {
            let some = layout(Screen::Today, width, &data(3, 0)).0;
            let rows: Vec<Rect> = some
                .iter()
                .filter(|(_, b)| matches!(b, Block::Session { .. }))
                .map(|(r, _)| *r)
                .collect();
            // Sessions 1 and 3 are plain, session 2 has a gap.
            assert_eq!(
                rows.iter().map(|r| r.h).collect::<Vec<_>>(),
                [SESSION_ROW, SESSION_ROW_GAP, SESSION_ROW]
            );
            for pair in rows.windows(2) {
                assert_eq!(pair[0].bottom(), pair[1].y);
            }
            let find = |want: fn(&Block) -> bool| some.iter().find(|(_, b)| want(b)).unwrap().0;
            let panel = find(|b| *b == Block::Panel);
            let line = find(|b| *b == Block::Section("Sessions"));
            let timeline = find(|b| *b == Block::Timeline);
            let summary = find(|b| matches!(b, Block::Summary(t) if t.starts_with("3 sessions")));
            assert_eq!((summary.y, summary.h), (line.y, line.h));
            assert!(summary.x > line.x && summary.right() <= line.right() + 0.01);
            assert_eq!((timeline.y, timeline.h), (panel.y, TIMELINE_HEIGHT));
            assert_eq!(rows[0].y, timeline.bottom());
            assert_eq!(panel.bottom(), rows[2].bottom());
        }
    }
    #[test]
    fn the_legend_appears_only_with_gaps_and_the_legacy_note_only_with_legacy_steps() {
        let legend = |blocks: &[(Rect, Block)]| {
            notes(blocks)
                .contains(&"Recovered steps are estimates and are already included in totals.")
        };
        let mut d = data(1, 0);
        assert!(!legend(&layout(Screen::Today, 552.0, &d).0));
        d.day.as_mut().unwrap().legacy = Some("4 legacy steps without sessions".into());
        let blocks = layout(Screen::Today, 552.0, &d).0;
        assert!(notes(&blocks).contains(&"4 legacy steps without sessions"));
        let gappy = data(2, 0);
        let blocks = layout(Screen::Today, 552.0, &gappy).0;
        assert!(legend(&blocks));
        assert!(!notes(&blocks).iter().any(|t| t.contains("legacy")));
        assert!(
            blocks
                .iter()
                .all(|(_, b)| !matches!(b, Block::Note(t) if t.contains("Fully observed")))
        );
    }
    #[test]
    fn session_zones_use_one_two_column_layout() {
        for width in [360.0, 408.0, 539.0, 540.0, 700.0, 900.0] {
            let z = session_zones(width);
            assert!(z.left.0 + z.left.1 <= z.right.0);
            assert!((z.right.0 + z.right.1 - (width - 12.0)).abs() < 0.01);
            assert!(z.left.1 >= 180.0);
        }
        assert_eq!(session_zones(400.0).left.0, session_zones(900.0).left.0);
        let track = timeline_track(Rect::new(10.0, 20.0, 300.0, TIMELINE_HEIGHT));
        assert_eq!(track, Rect::new(22.0, 26.0, 276.0, 18.0));
        assert!(track.bottom() + 14.0 <= 20.0 + TIMELINE_HEIGHT);
    }
    #[test]
    fn loading_pages_keep_their_controls() {
        let blocks = layout(Screen::Today, 552.0, &PageData::default()).0;
        assert!(
            blocks
                .iter()
                .any(|(_, b)| matches!(b, Block::IconButton { id, .. } if *id == PREV))
        );
        assert!(
            blocks
                .iter()
                .any(|(_, b)| matches!(b, Block::Note(t) if t.contains("Loading")))
        );
        let history = layout(
            Screen::History,
            552.0,
            &PageData {
                range: 7,
                ..Default::default()
            },
        )
        .0;
        assert!(history.iter().any(|(_, b)| matches!(b, Block::Button { id, kind: ButtonKind::Selected, .. } if *id == RANGE_BASE)));
    }
    #[test]
    fn history_stacks_chart_over_list_until_wide_then_sits_side_by_side() {
        let find = |blocks: &[(Rect, Block)], want: Block| {
            blocks.iter().find(|(_, b)| *b == want).unwrap().0
        };
        let d = data(0, 30);
        let medium = layout(Screen::History, 552.0, &d).0;
        let chart = find(&medium, Block::Chart);
        let head = find(&medium, Block::DayHead);
        assert!(head.y > chart.bottom() && head.x == 0.0);
        let wide = layout(Screen::History, 900.0, &d).0;
        let chart = find(&wide, Block::Chart);
        let head = find(&wide, Block::DayHead);
        assert!(head.x >= chart.right() && head.y < chart.bottom());
        let rows = wide
            .iter()
            .filter(|(_, b)| matches!(b, Block::DayRow { .. }))
            .count();
        assert_eq!(rows, 30);
        let first = wide.iter().find_map(|(_, b)| match b {
            Block::DayRow { id, index: 0, .. } => Some(*id),
            _ => None,
        });
        assert_eq!(first, Some(ROW_BASE));
        let empty = layout(Screen::History, 552.0, &data(0, 0)).0;
        assert!(
            empty
                .iter()
                .any(|(_, b)| matches!(b, Block::Note(t) if t.contains("No days")))
        );
    }
    #[test]
    fn range_selector_marks_the_active_range() {
        for (range, selected) in [(7, 0), (30, 1), (90, 2)] {
            let mut d = data(0, 7);
            d.range = range;
            let blocks = layout(Screen::History, 552.0, &d).0;
            let accents: Vec<_> = blocks
                .iter()
                .filter_map(|(_, b)| match b {
                    Block::Button {
                        id,
                        kind: ButtonKind::Selected,
                        ..
                    } => Some(*id - RANGE_BASE),
                    _ => None,
                })
                .collect();
            assert_eq!(accents, vec![selected]);
        }
    }
    #[test]
    fn columns_drop_low_value_fields_when_narrow_and_fill_the_width() {
        for width in [400.0, 539.0, 540.0, 700.0] {
            {
                let cols = history_cols(width);
                let last = cols.last().unwrap();
                assert!(
                    (last.x + last.w - (width - 12.0)).abs() < 1.0
                        || last.w == 116.0
                        || last.w == 60.0,
                    "{width}"
                );
                for pair in cols.windows(2) {
                    assert!(pair[0].x + pair[0].w <= pair[1].x + 0.01);
                }
            }
        }
        assert!(
            history_cols(400.0)
                .iter()
                .all(|c| c.field != Field::Sessions)
        );
        assert!(
            history_cols(600.0)
                .iter()
                .any(|c| c.field == Field::Sessions)
        );
        assert_eq!(Field::Coverage.title(), "Coverage");
    }
    #[test]
    fn screens_keep_separate_scroll_slots() {
        let all = [
            Screen::Today,
            Screen::History,
            Screen::Targets,
            Screen::Settings,
            Screen::HistoryDay,
        ];
        let mut slots: Vec<_> = all.iter().map(|s| s.slot()).collect();
        slots.sort();
        assert_eq!(slots, vec![0, 1, 2, 3, 4]);
        assert_eq!(Screen::of(Page::History, true), Screen::HistoryDay);
        assert_eq!(Screen::of(Page::Today, true), Screen::Today);
    }
    fn buttons(blocks: &[(Rect, Block)]) -> Vec<Id> {
        blocks
            .iter()
            .filter_map(|(_, b)| match b {
                Block::Button { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }
    fn fields(blocks: &[(Rect, Block)]) -> Vec<Id> {
        blocks
            .iter()
            .filter_map(|(_, b)| match b {
                Block::Field { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn settings_has_every_section_with_its_controls_and_no_classic_link() {
        let mut d = PageData::from_view(&View {
            connected: true,
            increment: 3,
            candidates: vec![crate::application::Candidate {
                address: 0x1122334455,
                name: "Pad".into(),
            }],
            ..Default::default()
        });
        d.form.shortcut_errors[1] = Some("bad".into());
        let (blocks, _) = layout(Screen::Settings, 600.0, &d);
        let startup: Vec<_> = blocks
            .iter()
            .take_while(|(_, b)| !matches!(b, Block::Section("Calories")))
            .collect();
        assert!(
            startup
                .iter()
                .all(|(_, b)| matches!(b, Block::Section(_) | Block::Toggle { .. }))
        );
        let toggles: Vec<_> = startup
            .iter()
            .filter_map(|(_, b)| match b {
                Block::Toggle { label, on, .. } => Some((label.as_str(), *on)),
                _ => None,
            })
            .collect();
        assert_eq!(
            toggles,
            vec![
                ("Start with Windows", false),
                ("Start minimized", true),
                ("Minimize to tray", false),
                ("Keep running when closed", true)
            ]
        );
        assert!(blocks.iter().any(|(_,block)| matches!(block, Block::Note(text) if text.contains("Without weight, calories come from your pad.") && text.contains("Pad readings are always saved."))));
        let sections: Vec<_> = blocks
            .iter()
            .filter_map(|(_, b)| match b {
                Block::Section(t) => Some(*t),
                _ => None,
            })
            .collect();
        assert_eq!(
            sections,
            [
                "Startup and tray",
                "Calories",
                "Appearance",
                "Device",
                "Activity recovery",
                "Controls",
                "Shortcuts",
                "Data and diagnostics",
                "About"
            ]
        );
        let ids = buttons(&blocks);
        for id in [SCAN, DISCONNECT, SHORTCUTS_SAVE, FOLDER] {
            assert!(ids.contains(&id), "{id}");
        }
        // The automatic-connection switch, the candidate row and the stepper are not buttons.
        assert!(
            blocks
                .iter()
                .any(|(_, b)| matches!(b, Block::Toggle { id, on: false, .. } if *id == AUTO))
        );
        assert!(
            blocks
                .iter()
                .any(|(_, b)| matches!(b, Block::Candidate { id, .. } if *id == CANDIDATE_BASE))
        );
        assert!(
            blocks
                .iter()
                .any(|(_, b)| matches!(b, Block::Stepper { value, .. } if value == "0.3 km/h"))
        );
        assert!(!ids.contains(&AUTO) && !ids.contains(&CANDIDATE_BASE));
        assert_eq!(
            fields(&blocks),
            (10..14).map(|i| FIELD_BASE + i).collect::<Vec<_>>()
        );
        // At most one filled accent button on the page.
        let accent = blocks
            .iter()
            .filter(|(_, b)| {
                matches!(
                    b,
                    Block::Button {
                        kind: ButtonKind::Accent,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(accent, 1);
        assert!(
            blocks
                .iter()
                .any(|(_, b)| matches!(b, Block::Field { error: Some(e), .. } if e == "bad"))
        );
        assert!(blocks.iter().any(
            |(_, b)| matches!(b, Block::Note(t) if t.contains("Start/Resume has no shortcut"))
        ));
        assert!(
            blocks
                .iter()
                .any(|(_, b)| matches!(b, Block::Note(t) if t.contains("Portable") && t.contains("beside desksteps.exe") && t.contains("%LOCALAPPDATA%\\DeskSteps")))
        );
        assert!(!blocks.iter().any(
            |(_, b)| matches!(b, Block::Note(t) if t.contains("Backup") || t.contains("-wal"))
        ));
        assert!(blocks.iter().all(|(_, b)| !matches!(
            b,
            Block::Row {
                label: "Mode" | "Last telemetry",
                ..
            }
        )));
        // Disconnect appears only while connected.
        let idle = layout(
            Screen::Settings,
            600.0,
            &PageData::from_view(&View::default()),
        )
        .0;
        assert!(!buttons(&idle).contains(&DISCONNECT));
    }
    #[test]
    fn theme_choices_fit_and_only_the_saved_choice_is_highlighted() {
        for width in [260.0, 360.0, 600.0, 1100.0] {
            for preference in crate::appearance::ThemePreference::ALL {
                let d = PageData::from_view(&View {
                    theme_preference: preference,
                    ..Default::default()
                });
                let (blocks, _) = layout(Screen::Settings, width, &d);
                let choices: Vec<_> = blocks
                    .iter()
                    .filter_map(|(r, b)| match b {
                        Block::Button { id, kind, .. }
                            if (THEME_BASE..THEME_BASE + 3).contains(id) =>
                        {
                            Some((r, id, kind))
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(choices.len(), 3);
                for (r, id, kind) in &choices {
                    assert!(r.x >= 0.0 && r.right() <= width);
                    let chosen = crate::appearance::ThemePreference::ALL
                        [(**id - THEME_BASE) as usize]
                        == preference;
                    assert_eq!(**kind == ButtonKind::Selected, chosen);
                }
                for (i, (a, _, _)) in choices.iter().enumerate() {
                    for (b, _, _) in &choices[i + 1..] {
                        assert!(a.right() <= b.x || a.bottom() <= b.y);
                    }
                }
            }
        }
    }
    #[test]
    fn device_buttons_are_disabled_in_a_bluetooth_free_preview_and_scan_while_connected() {
        let preview = PageData::from_view(&View {
            no_bluetooth: true,
            candidates: vec![crate::application::Candidate {
                address: 1,
                name: "P".into(),
            }],
            ..Default::default()
        });
        for id in [AUTO, SCAN, DISCONNECT, CANDIDATE_BASE] {
            assert!(preview.disabled.contains(&id), "{id}");
        }
        assert!(
            !PageData::from_view(&View::default())
                .disabled
                .contains(&AUTO)
        );
        let sim = PageData::from_view(&View {
            no_bluetooth: true,
            development: true,
            ..Default::default()
        });
        assert!(!sim.disabled.contains(&AUTO));
        assert!(
            PageData::from_view(&View {
                connected: true,
                ..Default::default()
            })
            .disabled
            .contains(&SCAN)
        );
        assert!(
            layout(Screen::Settings, 600.0, &preview)
                .0
                .iter()
                .any(|(_, b)| matches!(b, Block::Note(t) if t.contains("Bluetooth is off")))
        );
    }
    #[test]
    fn targets_page_has_a_date_four_fields_one_save_and_inline_errors() {
        let mut d = PageData::from_view(&View::default());
        d.form.effective = "2026-10-23".into();
        let (blocks, height) = layout(Screen::Targets, 400.0, &d);
        assert_eq!(
            fields(&blocks),
            (0..4).map(|i| FIELD_BASE + i).collect::<Vec<_>>()
        );
        assert_eq!(buttons(&blocks), vec![EFFECTIVE, TARGETS_SAVE]);
        assert!(blocks.iter().any(|(_, b)| matches!(b, Block::Button { id, label, .. } if *id == EFFECTIVE && label == "2026-10-23")));
        let plain = height;
        d.form.errors[2] = Some("Step target must be a whole number.".into());
        d.form.errors[0] = Some("Use a date such as 2026-10-13.".into());
        d.form.message = Some("Saved from 2026-10-23.".into());
        let (with, taller) = layout(Screen::Targets, 400.0, &d);
        assert!(taller > plain + 30.0);
        assert!(with.iter().any(
            |(_, b)| matches!(b, Block::Field { id, error: Some(_), .. } if *id == FIELD_BASE + 1)
        ));
        assert!(
            with.iter()
                .any(|(_, b)| matches!(b, Block::Error(t) if t.contains("date")))
        );
        assert!(
            with.iter()
                .any(|(_, b)| matches!(b, Block::Note(t) if t.starts_with("Saved")))
        );
        // Fields keep the same order and never overlap each other or the Save button.
        let rects: Vec<Rect> = with
            .iter()
            .filter(|(_, b)| matches!(b, Block::Field { .. } | Block::Button { .. }))
            .map(|(r, _)| *r)
            .collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(
                    !(a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom())
                );
            }
        }
        let frame = field_frame(Rect::new(0.0, 100.0, 300.0, 80.0));
        assert_eq!(frame, Rect::new(0.0, 120.0, 300.0, 32.0));
        assert!(field_edit(Rect::new(0.0, 100.0, 300.0, 80.0)).h < 32.0);
        assert!(
            field_height(Some("Caption"), Some("Error"), 300.0)
                > field_height(Some("Caption"), None, 300.0)
        );
    }
    #[test]
    fn field_captions_and_errors_wrap_and_move_the_next_field_down() {
        let caption = "Optional, 20 to 150. Enter for app estimates; leave blank for pad calories.";
        let error = "Shortcuts require at least two of Ctrl, Alt and Shift".to_string();
        let mut narrow = Builder { out: Vec::new() };
        let end = field(
            &mut narrow,
            0.0,
            200.0,
            FIELD_BASE,
            "Weight",
            Some(caption),
            Some(&error),
        );
        let first = narrow.out[0].0;
        assert!(
            first.h > 20.0 + 32.0 + 18.0 + 18.0 + 8.0,
            "long hints need more than single-line slots"
        );
        let mut wide = Builder { out: Vec::new() };
        field(
            &mut wide,
            0.0,
            360.0,
            FIELD_BASE,
            "Weight",
            Some(caption),
            Some(&error),
        );
        assert!(first.h > wide.out[0].0.h);
        field(&mut narrow, end, 200.0, FIELD_BASE + 1, "Steps", None, None);
        assert!(narrow.out[1].0.y >= first.bottom());
        assert_eq!(
            field_frame(first).h,
            32.0,
            "wrapping must not resize the native input"
        );
    }
    #[test]
    fn settings_reports_step_and_device_without_mode_or_last_telemetry_rows() {
        let d = PageData::from_view(&View {
            development: true,
            no_bluetooth: true,
            increment: 5,
            selected: Some(0x0123456789ab),
            ..Default::default()
        });
        assert_eq!(d.step, "0.5 km/h");
        assert!(d.device.contains(':'));
        for data in [
            d,
            PageData::from_view(&View {
                no_bluetooth: true,
                last_sample: Some("09:38:12".into()),
                ..Default::default()
            }),
            PageData::from_view(&View::default()),
        ] {
            let (blocks, _) = layout(Screen::Settings, 600.0, &data);
            assert!(blocks.iter().all(|(_, b)| !matches!(
                b,
                Block::Row {
                    label: "Mode" | "Last telemetry",
                    ..
                }
            )));
        }
        let n = PageData::from_view(&View::default());
        assert_eq!(n.device, "none selected");
    }
    #[test]
    fn bar_slots_follow_the_chart_block() {
        let slots = bar_slots(Rect::new(10.0, 20.0, 300.0, 200.0), 7);
        assert_eq!(slots.len(), 7);
        assert!(
            slots
                .iter()
                .all(|s| s.x >= 10.0 && s.right() <= 310.0 && s.y >= 20.0 && s.bottom() <= 220.0)
        );
    }
}
