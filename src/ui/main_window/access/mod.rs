//! UI Automation support: the custom-drawn window is described to screen readers as a flat
//! list of named elements. This module is the pure description; `provider` is the COM side.
//! Invoking a button asks the window to run the same activation as a click, so Start/Resume
//! still goes through `actions::request` and its availability checks.
use super::{
    frame::{self, MINUS, PAUSE, PLUS, START},
    pages::{self, Block, Screen},
    scene::Scene,
};
use crate::ui::{
    kit::input::Id,
    model::{day::SessionRow, page::Page},
};

pub mod provider;

/// Posted to the window: run the activation for target `wparam` (UI Automation Invoke/Toggle).
pub const ACCESS_INVOKE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 43;
/// Posted to the window: move the toolkit focus to target `wparam`.
pub const ACCESS_FOCUS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 44;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Button,
    Tab,
    Text,
    ListItem,
    Toggle,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable within one published tree; used for runtime ids.
    pub key: u32,
    /// The toolkit target this element activates, if any.
    pub target: Option<Id>,
    pub name: String,
    pub help: Option<String>,
    pub kind: Kind,
    /// Logical window coordinates: x, y, width, height.
    pub rect: [f32; 4],
    pub enabled: bool,
    pub focused: bool,
    pub offscreen: bool,
    /// Toggle state for switches.
    pub on: Option<bool>,
}
fn rect(r: crate::ui::kit::layout::Rect) -> [f32; 4] {
    [r.x, r.y, r.w, r.h]
}
fn intersects(a: crate::ui::kit::layout::Rect, b: crate::ui::kit::layout::Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}
fn session_name(s: &SessionRow) -> String {
    let mut name = if s.live {
        format!("Live session since {}, {}", s.start, s.state)
    } else {
        format!("Session {} to {}", s.start, s.end)
    };
    name.push_str(&format!(
        ", {}, {} steps, {} walking minutes, {} kcal",
        s.duration
            .trim_start_matches("Live · ")
            .replace(" · ", ", "),
        s.steps,
        s.minutes,
        s.kcal
    ));
    if let Some(gap) = &s.gap_line {
        name.push_str(", ");
        name.push_str(&gap.replace(" · ", ", "));
    }
    if let Some(recovery) = &s.recovery {
        name.push_str(", ");
        name.push_str(recovery);
    }
    name
}
/// Describes the scene in reading order: the live card, the rail, then the page.
pub fn build(scene: &Scene, page: Page, screen: Screen, focus: Option<Id>) -> Vec<Item> {
    let f = &scene.frame;
    let card = &scene.card;
    let mut items: Vec<Item> = Vec::new();
    let mut next = 100_000u32;
    let enabled = |id: Id| scene.targets.enabled(id);
    let mut text = |items: &mut Vec<Item>, name: String, r: [f32; 4], offscreen: bool| {
        next += 1;
        items.push(Item {
            key: next,
            target: None,
            name,
            help: None,
            kind: Kind::Text,
            rect: r,
            enabled: true,
            focused: false,
            offscreen,
            on: None,
        });
    };
    // Live card.
    text(
        &mut items,
        format!("Connection: {}", card.connection),
        rect(f.pill),
        false,
    );
    let speed = if card.speed == "—" {
        "Speed unknown".to_string()
    } else {
        format!("{} {}", card.speed, card.speed_caption)
    };
    let speed = match &card.pending {
        Some(p) => format!("{speed}. {p}"),
        None => speed,
    };
    text(&mut items, speed, rect(f.speed), false);
    let step = format!("{:.1}", scene.data.increment as f32 / 10.0);
    let button = |items: &mut Vec<Item>,
                  id: Id,
                  name: String,
                  help: Option<String>,
                  r: [f32; 4],
                  kind: Kind,
                  on: Option<bool>,
                  offscreen: bool| {
        items.push(Item {
            key: id,
            target: Some(id),
            name,
            help,
            kind,
            rect: r,
            enabled: enabled(id),
            focused: focus == Some(id),
            offscreen,
            on,
        });
    };
    let help = |id| frame::tooltip_text(id, card, f.collapsed);
    button(
        &mut items,
        MINUS,
        format!("Decrease speed by {step} km/h"),
        help(MINUS),
        rect(f.minus),
        Kind::Button,
        None,
        false,
    );
    button(
        &mut items,
        PLUS,
        format!("Increase speed by {step} km/h"),
        help(PLUS),
        rect(f.plus),
        Kind::Button,
        None,
        false,
    );
    button(
        &mut items,
        START,
        "Start or resume".into(),
        help(START),
        rect(f.start),
        Kind::Button,
        None,
        false,
    );
    button(
        &mut items,
        PAUSE,
        "Pause".into(),
        help(PAUSE),
        rect(f.pause),
        Kind::Button,
        None,
        false,
    );
    text(&mut items, card.steps.clone(), rect(f.steps), f.collapsed);
    for p in Page::ALL {
        let id = frame::nav_id(p);
        let name = if p == page {
            format!("{}, current page", p.label())
        } else {
            p.label().to_string()
        };
        button(
            &mut items,
            id,
            name,
            None,
            rect(f.nav[p.index()]),
            Kind::Tab,
            None,
            false,
        );
    }
    // Page content, in block order.
    let body = f.body;
    let data = &scene.data;
    for (r, block) in &scene.blocks {
        let at = Scene::to_window(f, scene.scroll, *r);
        let off = !intersects(at, body);
        let rr = rect(at);
        match block {
            Block::Tile { index, .. } => {
                if let Some(t) = data.day.as_ref().map(|d| &d.tiles[*index]) {
                    let mut name = format!("{}: {}", t.title, t.value);
                    for part in [&t.target, &t.remaining] {
                        if !part.is_empty() {
                            name.push_str(", ");
                            name.push_str(part);
                        }
                    }
                    text(&mut items, name, rr, off);
                }
            }
            Block::Section(t) => text(&mut items, t.to_string(), rr, off),
            Block::Note(t) | Block::Error(t) | Block::Label(t) => {
                text(&mut items, t.clone(), rr, off)
            }
            Block::Row { label, value, .. } => {
                text(&mut items, format!("{label}: {value}"), rr, off)
            }
            Block::Summary(t) => text(&mut items, format!("Sessions summary: {t}"), rr, off),
            Block::Timeline => {
                if let Some(d) = data.day.as_ref() {
                    text(
                        &mut items,
                        format!(
                            "Session timeline: {}. The sessions below list the same times.",
                            d.summary
                        ),
                        rr,
                        off,
                    );
                }
            }
            Block::Session { index, .. } => {
                if let Some(s) = data.day.as_ref().and_then(|d| d.sessions.get(*index)) {
                    text(&mut items, session_name(s), rr, off);
                }
            }
            Block::Chart => text(
                &mut items,
                "Steps per day chart. Use the list of days for the same numbers.".into(),
                rr,
                off,
            ),
            Block::Button { id, label, .. } => {
                let name = match *id {
                    pages::DATE => format!("Choose date, {label}"),
                    pages::EFFECTIVE => format!("Choose the effective date, {label}"),
                    x if (pages::THEME_BASE..pages::THEME_BASE + 3).contains(&x) => {
                        let preference = crate::appearance::ThemePreference::ALL
                            [(x - pages::THEME_BASE) as usize];
                        format!(
                            "{label} theme{}",
                            if data.theme_preference == preference {
                                ", selected"
                            } else {
                                ""
                            }
                        )
                    }
                    _ => label.clone(),
                };
                button(&mut items, *id, name, None, rr, Kind::Button, None, off);
            }
            Block::IconButton { id, .. } => {
                let name = match *id {
                    pages::PREV => "Previous day",
                    pages::NEXT => "Next day",
                    _ => "Button",
                };
                button(
                    &mut items,
                    *id,
                    name.into(),
                    None,
                    rr,
                    Kind::Button,
                    None,
                    off,
                );
            }
            Block::Toggle { id, label, on } => button(
                &mut items,
                *id,
                label.clone(),
                None,
                rr,
                Kind::Toggle,
                Some(*on),
                off,
            ),
            Block::Candidate { id, index, .. } => {
                if let Some((_, label, chosen)) = data.devices.candidates.get(*index) {
                    let name = if *chosen {
                        format!("{label}, saved default")
                    } else {
                        label.clone()
                    };
                    button(&mut items, *id, name, None, rr, Kind::ListItem, None, off);
                }
            }
            Block::Stepper { down, up, value } => {
                let (a, c) = pages::stepper_buttons(at);
                text(&mut items, format!("Speed step {value}"), rr, off);
                button(
                    &mut items,
                    *down,
                    "Decrease speed step".into(),
                    None,
                    rect(a),
                    Kind::Button,
                    None,
                    off,
                );
                button(
                    &mut items,
                    *up,
                    "Increase speed step".into(),
                    None,
                    rect(c),
                    Kind::Button,
                    None,
                    off,
                );
            }
            Block::DayRow { id, index, .. } => {
                if let Some(row) = data.history.as_ref().and_then(|h| h.rows.get(*index)) {
                    let name = format!(
                        "{}: {} steps, {} min, {} sessions, target {}, {}",
                        row.date, row.steps, row.minutes, row.sessions, row.target, row.coverage
                    );
                    button(&mut items, *id, name, None, rr, Kind::ListItem, None, off);
                }
            }
            // Fields are native controls with their own automation; bars are covered by the
            // day list; panels and headers carry no information.
            Block::Field { .. } | Block::Panel | Block::DayHead => {}
        }
    }
    let _ = screen;
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::View,
        statistics::{Dashboard, Day, Preferences},
        ui::main_window::scene::Context,
    };
    fn view() -> View {
        let mut v = View {
            connected: true,
            controls_enabled: true,
            speed_available: true,
            increment: 5,
            fresh_speed: Some(3.0),
            adjustable_speed: Some(3.0),
            automatic_connection: true,
            ..Default::default()
        };
        v.totals.day = "2026-10-23".into();
        v.dashboard = Some(
            Dashboard {
                day: Day {
                    date: "2026-10-23".into(),
                    steps: 700,
                    samples: 1,
                    preferences: Preferences {
                        steps: Some(1000),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                today: "2026-10-23".into(),
                timezone: "UTC".into(),
                days: (0..3)
                    .map(|i| Day {
                        date: format!("2026-10-{}", 21 + i),
                        steps: 10,
                        samples: 1,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }
            .into(),
        );
        v
    }
    fn items(screen: Screen, page: Page, v: &View, focus: Option<Id>) -> Vec<Item> {
        let c = Context {
            range: v.dashboard.as_ref().map_or(30, |d| d.days.len() as u32),
            ..Default::default()
        };
        let scene = Scene::new(v, (820.0, 900.0), screen, 0.0, &c);
        build(&scene, page, screen, focus)
    }
    fn find<'a>(items: &'a [Item], name: &str) -> &'a Item {
        items
            .iter()
            .find(|i| i.name.contains(name))
            .unwrap_or_else(|| {
                panic!(
                    "no item named {name}: {:?}",
                    items.iter().map(|i| &i.name).collect::<Vec<_>>()
                )
            })
    }
    #[test]
    fn every_element_has_a_name_and_keys_are_unique() {
        for (screen, page) in [
            (Screen::Today, Page::Today),
            (Screen::History, Page::History),
            (Screen::Targets, Page::Targets),
            (Screen::Settings, Page::Settings),
        ] {
            let list = items(screen, page, &view(), None);
            assert!(list.iter().all(|i| !i.name.trim().is_empty()), "{screen:?}");
            let mut keys: Vec<_> = list.iter().map(|i| i.key).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), list.len(), "{screen:?}: duplicate keys");
        }
    }
    #[test]
    fn movement_buttons_are_named_with_their_state_and_pause_stays_enabled() {
        let list = items(Screen::Today, Page::Today, &view(), None);
        let start = find(&list, "Start or resume");
        assert!(!start.enabled, "Start is disabled while the belt moves");
        assert!(
            start
                .help
                .as_deref()
                .unwrap()
                .contains("selected by the pad")
        );
        assert!(find(&list, "Pause").enabled);
        assert!(find(&list, "Decrease speed by 0.5 km/h").enabled);
        assert_eq!(find(&list, "Pause").kind, Kind::Button);
        let idle = View {
            fresh_speed: Some(0.0),
            ..view()
        };
        assert!(
            find(
                &items(Screen::Today, Page::Today, &idle, None),
                "Start or resume"
            )
            .enabled
        );
    }
    #[test]
    fn navigation_marks_the_current_page_and_tiles_read_as_text() {
        let list = items(Screen::History, Page::History, &view(), None);
        let tabs: Vec<_> = list.iter().filter(|i| i.kind == Kind::Tab).collect();
        assert_eq!(tabs.len(), 4);
        assert!(find(&list, "History, current page").target.is_some());
        let today = items(Screen::Today, Page::Today, &view(), None);
        let steps = find(&today, "Steps: 700");
        assert_eq!(steps.kind, Kind::Text);
        assert!(steps.name.contains("700/1,000 · 70%"));
        assert!(steps.name.contains("300 remaining"));
    }
    #[test]
    fn history_rows_are_list_items_and_the_chart_points_to_them() {
        let list = items(
            Screen::History,
            Page::History,
            &view(),
            Some(pages::ROW_BASE),
        );
        let rows: Vec<_> = list.iter().filter(|i| i.kind == Kind::ListItem).collect();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].name.starts_with("2026-10-23"));
        assert!(rows[0].focused && !rows[1].focused);
        assert!(find(&list, "Steps per day chart").target.is_none());
    }
    #[test]
    fn settings_toggle_reports_its_state_and_candidates_their_default() {
        let mut v = view();
        v.candidates = vec![crate::application::Candidate {
            address: 1,
            name: "Pad".into(),
        }];
        v.selected = Some(1);
        let list = items(Screen::Settings, Page::Settings, &v, None);
        assert_eq!(find(&list, "Minimize to tray").on, Some(false));
        assert_eq!(find(&list, "Keep running when closed").on, Some(true));
        assert_eq!(find(&list, "Start minimized").on, Some(true));
        assert!(!find(&list, "Start with Windows").enabled);
        v.windows_startup_available = true;
        v.windows_startup = true;
        let list = items(Screen::Settings, Page::Settings, &v, None);
        assert!(find(&list, "Start with Windows").enabled);
        assert_eq!(find(&list, "Start with Windows").on, Some(true));
        let toggle = find(&list, "Connect automatically");
        assert_eq!((toggle.kind, toggle.on), (Kind::Toggle, Some(true)));
        assert!(find(&list, "saved default").target.is_some());
        assert!(find(&list, "Speed step 0.5 km/h").target.is_none());
        assert_eq!(
            find(&list, "Increase speed step").enabled,
            !find(&list, "Increase speed step").offscreen
        );
        assert!(find(&list, "Automatic theme, selected").enabled);
        assert_eq!(
            find(&list, "Dark theme").target,
            Some(pages::THEME_BASE + 2)
        );
        let ctx = Context::default();
        let scene = Scene::new(&v, (820.0, 900.0), Screen::Settings, 0.0, &ctx);
        let controls = scene
            .blocks
            .iter()
            .find_map(|(r, b)| matches!(b, Block::Section("Controls")).then_some(r.y))
            .unwrap();
        let scene = Scene::new(&v, (820.0, 900.0), Screen::Settings, controls, &ctx);
        let list = build(&scene, Page::Settings, Screen::Settings, None);
        assert!(find(&list, "Increase speed step").enabled);
    }
    #[test]
    fn sessions_read_naturally_and_the_live_one_keeps_its_state_and_recovery() {
        let mut v = view();
        let d = std::sync::Arc::make_mut(v.dashboard.as_mut().unwrap());
        d.day.preferences.weight = Some(80.0);
        for (i, (a, b)) in [("09:00", "09:30"), ("11:12", "11:42")].iter().enumerate() {
            d.sessions.push(crate::statistics::Session {
                id: i as i64 + 1,
                start: format!("2026-10-23T{a}:00Z"),
                end: format!("2026-10-23T{b}:00Z"),
                state: "Continuity uncertain".into(),
                steps: 1825,
                active_ms: 1_440_000,
                kcal: Some(60.5),
                gap: i == 0,
                recovered_steps: 5 * (1 - i as i64),
                recovered_ms: 60_000 * (1 - i as i64),
                ..Default::default()
            });
        }
        d.current = d.sessions.get(1).cloned();
        let list = items(Screen::Today, Page::Today, &v, None);
        let first = find(
            &list,
            "Session 09:00 to 09:30, 30 min, 1,825 steps, 24 walking minutes, 61 kcal, Recording gap, 5 steps recovered, Recovered: 5 steps",
        );
        assert_eq!(first.kind, Kind::Text);
        let live = find(
            &list,
            "Live session since 11:12, Continuity uncertain, 30 min",
        );
        assert!(live.name.ends_with("61 kcal") && !live.name.contains("gap"));
        assert!(find(&list, "Session timeline: 2 sessions").target.is_none());
        assert!(find(&list, "Sessions summary: 2 sessions").target.is_none());
    }
    #[test]
    fn disabled_connection_controls_are_exposed_as_disabled() {
        let v = View {
            no_bluetooth: true,
            ..view()
        };
        let list = items(Screen::Settings, Page::Settings, &v, None);
        assert!(!find(&list, "Connect automatically").enabled);
        assert!(!find(&list, "Scan").enabled);
    }
    #[test]
    fn items_outside_the_body_are_marked_offscreen() {
        let c = Context {
            range: 30,
            ..Default::default()
        };
        let scene = Scene::new(&view(), (520.0, 420.0), Screen::Settings, 0.0, &c);
        let list = build(&scene, Page::Settings, Screen::Settings, None);
        assert!(list.iter().any(|i| i.offscreen));
        assert!(!find(&list, "Pause").offscreen);
    }
}
