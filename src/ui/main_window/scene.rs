//! Everything needed to draw and hit-test one frame, computed purely from a snapshot.
use super::{
    frame::Frame,
    pages::{self, Block, PageData, Screen},
};
use crate::{
    application::View,
    statistics::{Dashboard, Day, Request},
    ui::{
        kit::{
            input::{Id, Targets},
            layout::{Rect, Scroll},
        },
        model::{
            day::{DayView, HistoryView},
            live::LiveCard,
        },
    },
};

/// Window state that selects what the pages show.
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Day viewed on Today; `None` follows today.
    pub selected: Option<String>,
    /// Day opened from History.
    pub detail: Option<String>,
    pub range: u32,
    pub focus: Option<Id>,
    /// Last dashboard day that was today, for the live card.
    pub today: Option<Day>,
    pub form: pages::Form,
}
/// The statistics request the screen needs; `None` until today's date is known.
pub fn desired_request(view: &View, screen: Screen, ctx: &Context) -> Option<Request> {
    let today = view.totals.day.clone();
    if today.is_empty() {
        return None;
    }
    Some(match screen {
        Screen::Today => Request {
            day: ctx.selected.clone().unwrap_or(today),
            range: 1,
        },
        Screen::HistoryDay => Request {
            day: ctx.detail.clone().unwrap_or(today),
            range: 1,
        },
        Screen::History => Request {
            day: today,
            range: ctx.range,
        },
        Screen::Targets | Screen::Settings => Request {
            day: today,
            range: 1,
        },
    })
}
/// The published dashboard, if it answers `request`. History needs the whole range.
pub fn answer<'a>(view: &'a View, screen: Screen, request: &Request) -> Option<&'a Dashboard> {
    view.dashboard.as_deref().filter(|d| {
        d.day.date == request.day
            && (screen != Screen::History || d.days.len() == request.range as usize)
    })
}
pub struct Scene {
    pub frame: Frame,
    pub card: LiveCard,
    pub data: PageData,
    pub blocks: Vec<(Rect, Block)>,
    pub content_height: f32,
    pub scroll: f32,
    pub targets: Targets,
    /// Native EDIT fields on this screen: id and rectangle in window space.
    pub fields: Vec<(Id, Rect)>,
    /// False while the statistics for the screen are still being fetched.
    pub loaded: bool,
}
impl Scene {
    /// `scroll` is clamped to the screen's content and returned in the scene.
    pub fn new(view: &View, size: (f32, f32), screen: Screen, scroll: f32, ctx: &Context) -> Self {
        let (card, data) = Self::content(view, screen, ctx);
        Self::layout(view, size, screen, scroll, ctx, card, data)
    }
    /// Worker ticks may change diagnostics without changing anything drawn.
    /// Local input/resize still uses `new` so focus, scroll and geometry update.
    pub fn updated(
        &self,
        view: &View,
        size: (f32, f32),
        screen: Screen,
        scroll: f32,
        ctx: &Context,
    ) -> Option<Self> {
        let (card, data) = Self::content(view, screen, ctx);
        if self.card == card && self.data == data {
            return None;
        }
        Some(Self::layout(view, size, screen, scroll, ctx, card, data))
    }
    fn content(view: &View, screen: Screen, ctx: &Context) -> (LiveCard, PageData) {
        let card = LiveCard::from_view(view, ctx.today.as_ref());
        let mut data = PageData::from_view(view);
        data.range = ctx.range;
        data.form = ctx.form.clone();
        data.form.message = view.ui_message.clone();
        if data.form.effective.is_empty() {
            data.form.effective = view.totals.day.clone();
        }
        if let Some(request) = desired_request(view, screen, ctx)
            && let Some(dash) = answer(view, screen, &request)
        {
            match screen {
                Screen::Today | Screen::HistoryDay => data.day = Some(DayView::new(dash)),
                Screen::History => data.history = Some(HistoryView::new(dash)),
                _ => {}
            }
        }
        (card, data)
    }
    fn layout(
        view: &View,
        size: (f32, f32),
        screen: Screen,
        scroll: f32,
        ctx: &Context,
        card: LiveCard,
        data: PageData,
    ) -> Self {
        let frame = Frame::new(size.0, size.1, view.development);
        let (blocks, content_height) = pages::layout(screen, frame.page_width(), &data);
        let mut s = Scroll { offset: scroll };
        s.clamp(content_height, frame.body.h);
        let rows: Vec<Id> = blocks
            .iter()
            .filter_map(|(_, b)| match b {
                Block::DayRow { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        let focus_row = ctx.focus.filter(|f| rows.contains(f));
        let can_next = data.day.as_ref().is_some_and(|d| !d.is_today);
        let mut page_targets: Vec<(Id, Rect, bool, bool)> = Vec::new();
        let mut fields = Vec::new();
        for (rect, block) in &blocks {
            let at = Self::to_window(&frame, s.offset, *rect);
            match block {
                Block::Button { id, .. } => {
                    page_targets.push((*id, at, !data.disabled.contains(id), true))
                }
                Block::Toggle { id, .. } | Block::Candidate { id, .. } => {
                    page_targets.push((*id, at, !data.disabled.contains(id), true))
                }
                Block::Stepper { down, up, .. } => {
                    let (a, c) = pages::stepper_buttons(at);
                    page_targets.push((*down, a, !data.disabled.contains(down), true));
                    page_targets.push((*up, c, !data.disabled.contains(up), true));
                }
                Block::Field { id, .. } => {
                    page_targets.push((*id, pages::field_frame(at), true, true));
                    fields.push((*id, pages::field_edit(at)));
                }
                Block::IconButton { id, .. } => {
                    page_targets.push((*id, at, *id != pages::NEXT || can_next, true))
                }
                Block::DayRow { id, .. } => page_targets.push((
                    *id,
                    at,
                    true,
                    focus_row == Some(*id) || (focus_row.is_none() && *id == pages::ROW_BASE),
                )),
                Block::Chart => {
                    let n = data.history.as_ref().map_or(0, |h| h.bars.len());
                    for (i, slot) in pages::bar_slots(at, n).into_iter().enumerate() {
                        page_targets.push((pages::BAR_BASE + i as Id, slot, true, false));
                    }
                }
                _ => {}
            }
        }
        let targets = frame.targets(&card, &page_targets);
        let loaded = match screen {
            Screen::Today | Screen::HistoryDay => data.day.is_some(),
            Screen::History => data.history.is_some(),
            Screen::Targets | Screen::Settings => true,
        };
        Self {
            loaded,
            frame,
            card,
            data,
            blocks,
            content_height,
            scroll: s.offset,
            targets,
            fields,
        }
    }
    pub fn to_window(frame: &Frame, scroll: f32, rect: Rect) -> Rect {
        rect.offset(frame.body.x, frame.body.y - scroll)
    }
    #[cfg(test)]
    pub fn max_scroll(&self) -> f32 {
        Scroll::max(self.content_height, self.frame.body.h)
    }
    /// A block's rectangle in content space, for scrolling it into view.
    pub fn content_rect(&self, id: Id) -> Option<Rect> {
        self.blocks.iter().find_map(|(r, b)| match b {
            Block::DayRow { id: row, .. } if *row == id => Some(*r),
            _ => None,
        })
    }
    /// Date a chart bar or history row stands for.
    pub fn date_of(&self, id: Id) -> Option<String> {
        let h = self.data.history.as_ref()?;
        if (pages::ROW_BASE..pages::BAR_BASE).contains(&id) {
            return h
                .rows
                .get((id - pages::ROW_BASE) as usize)
                .map(|r| r.date.clone());
        }
        h.bars
            .get(id.checked_sub(pages::BAR_BASE)? as usize)
            .map(|b| b.date.clone())
    }
    pub fn is_bar(id: Id) -> bool {
        (pages::BAR_BASE..pages::BAR_BASE + 200).contains(&id)
    }
    pub fn tooltip(&self, id: Id) -> Option<String> {
        if Self::is_bar(id) {
            let h = self.data.history.as_ref()?;
            return h
                .bars
                .get((id - pages::BAR_BASE) as usize)
                .map(|b| b.tooltip.clone());
        }
        super::frame::tooltip_text(id, &self.card, self.frame.collapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        statistics::{Preferences, Session},
        ui::main_window::pages::*,
    };
    fn view_with(day: &str, dash: Option<Dashboard>) -> View {
        let mut v = View::default();
        v.totals.day = day.into();
        v.dashboard = dash.map(Into::into);
        v
    }
    fn ctx() -> Context {
        Context {
            range: 30,
            ..Default::default()
        }
    }
    fn dashboard(date: &str, days: usize) -> Dashboard {
        Dashboard {
            day: Day {
                date: date.into(),
                steps: 12,
                samples: 1,
                preferences: Preferences {
                    steps: Some(100),
                    ..Default::default()
                },
                ..Default::default()
            },
            today: "2026-10-23".into(),
            timezone: "UTC".into(),
            days: (0..days)
                .map(|i| Day {
                    date: format!("2026-09-{:02}", i % 28 + 1),
                    samples: 1,
                    ..Default::default()
                })
                .collect(),
            sessions: vec![Session {
                id: 1,
                start: "2026-10-23T09:00:00Z".into(),
                end: "2026-10-23T09:10:00Z".into(),
                state: "Finished".into(),
                steps: 12,
                active_ms: 1000,
                kcal: None,
                gap: false,
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    #[test]
    fn requests_follow_the_screen_and_never_exist_before_today_is_known() {
        let c = Context {
            selected: Some("2026-10-01".into()),
            detail: Some("2026-10-02".into()),
            range: 90,
            ..Default::default()
        };
        assert!(desired_request(&View::default(), Screen::Today, &c).is_none());
        let v = view_with("2026-10-23", None);
        let ask = |s| {
            desired_request(&v, s, &c)
                .map(|r| (r.day, r.range))
                .unwrap()
        };
        assert_eq!(ask(Screen::Today), ("2026-10-01".into(), 1));
        assert_eq!(ask(Screen::HistoryDay), ("2026-10-02".into(), 1));
        assert_eq!(ask(Screen::History), ("2026-10-23".into(), 90));
        assert_eq!(ask(Screen::Settings), ("2026-10-23".into(), 1));
        let follow = desired_request(&v, Screen::Today, &ctx()).unwrap();
        assert_eq!(follow.day, "2026-10-23");
    }
    #[test]
    fn updates_ignore_diagnostic_ticks_but_keep_progress_coverage_and_control_changes() {
        let mut v = view_with("2026-10-23", Some(dashboard("2026-10-23", 1)));
        let c = ctx();
        let size = (820.0, 580.0);
        let original = Scene::new(&v, size, Screen::Today, 0.0, &c);
        v.last_sample = Some("12:34:56".into());
        v.simulator_status = "clock tick".into();
        v.totals.samples += 1;
        std::sync::Arc::make_mut(v.dashboard.as_mut().unwrap())
            .day
            .samples += 1;
        assert!(original.updated(&v, size, Screen::Today, 0.0, &c).is_none());
        std::sync::Arc::make_mut(v.dashboard.as_mut().unwrap())
            .day
            .steps += 1;
        let progress = original.updated(&v, size, Screen::Today, 0.0, &c).unwrap();
        assert_ne!(original.data, progress.data);
        std::sync::Arc::make_mut(v.dashboard.as_mut().unwrap()).sessions[0].gap = true;
        assert!(progress.updated(&v, size, Screen::Today, 0.0, &c).is_some());
        v.error = Some("Connection lost".into());
        assert!(progress.updated(&v, size, Screen::Today, 0.0, &c).is_some());
    }
    #[test]
    fn pages_only_use_a_dashboard_that_answers_the_request() {
        // The selected day is the past day: today's dashboard must not be shown for it.
        let c = Context {
            selected: Some("2026-10-22".into()),
            ..ctx()
        };
        let v = view_with("2026-10-23", Some(dashboard("2026-10-23", 1)));
        let s = Scene::new(&v, (820.0, 580.0), Screen::Today, 0.0, &c);
        assert!(s.data.day.is_none());
        let v = view_with("2026-10-23", Some(dashboard("2026-10-22", 1)));
        let s = Scene::new(&v, (820.0, 580.0), Screen::Today, 0.0, &c);
        let day = s.data.day.unwrap();
        assert_eq!(day.date, "2026-10-22");
        assert!(!day.is_today);
        assert_eq!(day.tiles[0].value, "12");
        // History needs the whole range, not just any answer for today.
        let v = view_with("2026-10-23", Some(dashboard("2026-10-23", 7)));
        assert!(
            Scene::new(&v, (820.0, 580.0), Screen::History, 0.0, &ctx())
                .data
                .history
                .is_none()
        );
        let v = view_with("2026-10-23", Some(dashboard("2026-10-23", 30)));
        let s = Scene::new(&v, (820.0, 580.0), Screen::History, 0.0, &ctx());
        assert_eq!(s.data.history.unwrap().rows.len(), 30);
    }
    #[test]
    fn tiles_and_rail_use_the_same_day_source() {
        let mut v = view_with("2026-10-23", Some(dashboard("2026-10-23", 1)));
        v.totals.recorded_steps = 9999;
        let today = v.dashboard.as_ref().map(|d| d.day.clone());
        let c = Context { today, ..ctx() };
        let s = Scene::new(&v, (820.0, 580.0), Screen::Today, 0.0, &c);
        assert_eq!(s.data.day.unwrap().tiles[0].value, "12");
        assert_eq!(s.card.steps, "12/100 steps · 12%");
    }
    #[test]
    fn scroll_is_clamped_and_moves_page_targets() {
        let view = View::default();
        let top = Scene::new(&view, (520.0, 420.0), Screen::Settings, 0.0, &ctx());
        assert!(top.content_height > top.frame.body.h);
        let y0 = top.targets.rect(FOLDER);
        let far = Scene::new(&view, (520.0, 420.0), Screen::Settings, 10_000.0, &ctx());
        assert_eq!(far.scroll, far.max_scroll());
        assert!(far.max_scroll() > 0.0);
        let y1 = far.targets.rect(FOLDER).unwrap();
        assert!(y0.is_none_or(|r| r.y > y1.y));
        assert!(y1.bottom() <= far.frame.body.bottom() + 0.01);
    }
    #[test]
    fn short_pages_do_not_scroll() {
        let s = Scene::new(
            &View::default(),
            (1000.0, 800.0),
            Screen::Targets,
            500.0,
            &ctx(),
        );
        assert_eq!((s.scroll, s.max_scroll()), (0.0, 0.0));
        assert!(!s.frame.collapsed);
    }
    #[test]
    fn scenes_exist_for_every_screen_and_size() {
        for screen in [
            Screen::Today,
            Screen::History,
            Screen::HistoryDay,
            Screen::Targets,
            Screen::Settings,
        ] {
            for size in [
                (520.0, 420.0),
                (720.0, 470.0),
                (820.0, 580.0),
                (1900.0, 1000.0),
            ] {
                let s = Scene::new(&View::default(), size, screen, 0.0, &ctx());
                assert!(s.targets.0.len() >= 8);
                assert!(!s.blocks.is_empty());
            }
        }
    }
    #[test]
    fn history_exposes_rows_and_bars_as_targets_with_one_row_tab_stop() {
        let v = view_with("2026-10-23", Some(dashboard("2026-10-23", 30)));
        let s = Scene::new(&v, (820.0, 900.0), Screen::History, 0.0, &ctx());
        let bars = s
            .targets
            .0
            .iter()
            .filter(|t| (BAR_BASE..BAR_BASE + 100).contains(&t.id))
            .count();
        assert_eq!(bars, 30);
        let stops: Vec<_> = s
            .targets
            .0
            .iter()
            .filter(|t| t.id >= ROW_BASE && t.id < BAR_BASE && t.tab_stop)
            .map(|t| t.id)
            .collect();
        assert_eq!(stops, vec![ROW_BASE]);
        assert!(
            s.targets
                .0
                .iter()
                .filter(|t| t.id >= BAR_BASE)
                .all(|t| !t.tab_stop)
        );
        // A focused row becomes the single stop.
        let c = Context {
            focus: Some(ROW_BASE + 3),
            ..ctx()
        };
        let s = Scene::new(&v, (820.0, 900.0), Screen::History, 0.0, &c);
        let stops: Vec<_> = s
            .targets
            .0
            .iter()
            .filter(|t| t.id >= ROW_BASE && t.id < BAR_BASE && t.tab_stop)
            .map(|t| t.id)
            .collect();
        assert_eq!(stops, vec![ROW_BASE + 3]);
        // Dates resolve for bars (oldest first) and rows (newest first).
        let last = s.date_of(BAR_BASE + 29).unwrap();
        assert_eq!(s.date_of(ROW_BASE), Some(last));
        assert!(s.date_of(BAR_BASE + 30).is_none() && s.date_of(5).is_none());
        assert!(s.tooltip(BAR_BASE).unwrap().contains("steps"));
        assert!(s.tooltip(START_ID).is_some());
        assert!(s.content_rect(ROW_BASE).is_some() && s.content_rect(BAR_BASE).is_none());
    }
    const START_ID: Id = crate::ui::main_window::frame::START;
    #[test]
    fn next_day_is_disabled_on_today() {
        let c = Context {
            selected: None,
            ..ctx()
        };
        let v = view_with("2026-10-23", Some(dashboard("2026-10-23", 1)));
        let s = Scene::new(&v, (820.0, 580.0), Screen::Today, 0.0, &c);
        assert!(s.targets.enabled(PREV) && !s.targets.enabled(NEXT) && s.targets.enabled(DATE));
        assert!(s.targets.rect(TODAY_BUTTON).is_none());
        let c = Context {
            selected: Some("2026-10-22".into()),
            ..ctx()
        };
        let v = view_with("2026-10-23", Some(dashboard("2026-10-22", 1)));
        let s = Scene::new(&v, (820.0, 580.0), Screen::Today, 0.0, &c);
        assert!(s.targets.enabled(NEXT) && s.targets.enabled(TODAY_BUTTON));
    }
    /// Rounds a logical rectangle to whole device pixels the way the renderer and the
    /// native fields do.
    fn physical(r: Rect, dpi: u32) -> (i32, i32, i32, i32) {
        use crate::ui::kit::layout::to_physical as px;
        (
            px(r.x, dpi),
            px(r.y, dpi),
            px(r.right(), dpi),
            px(r.bottom(), dpi),
        )
    }
    #[test]
    fn every_screen_keeps_its_key_elements_inside_the_window_and_apart_at_96_144_and_216_dpi() {
        use crate::ui::kit::layout::{to_logical, to_physical};
        let mut view = View::default();
        view.totals.day = "2026-10-23".into();
        view.dashboard = Some(dashboard("2026-10-23", 30).into());
        for dpi in [96u32, 144, 216] {
            // Client sizes a user can reach at this scale: the minimum and some larger ones.
            for (lw, lh) in [
                (520.0, 420.0),
                (720.0, 470.0),
                (820.0, 580.0),
                (1400.0, 900.0),
            ] {
                let (pw, ph) = (to_physical(lw, dpi), to_physical(lh, dpi));
                let size = (to_logical(pw, dpi), to_logical(ph, dpi));
                for screen in [
                    Screen::Today,
                    Screen::History,
                    Screen::HistoryDay,
                    Screen::Targets,
                    Screen::Settings,
                ] {
                    let c = Context {
                        range: 30,
                        ..Default::default()
                    };
                    let scene = Scene::new(&view, size, screen, 0.0, &c);
                    let f = &scene.frame;
                    // Rail and header controls are never scrolled: they must be fully on screen,
                    // at least a few device pixels in size, and must not touch each other.
                    let mut fixed = vec![
                        f.pause, f.nav[0], f.nav[1], f.nav[2], f.nav[3], f.pill, f.speed, f.title,
                        f.body,
                    ];
                    fixed.extend([f.minus, f.plus, f.start]);
                    let mut boxes = Vec::new();
                    for r in fixed {
                        let (l, t, rr, b) = physical(r, dpi);
                        assert!(
                            l >= 0 && t >= 0 && rr <= pw && b <= ph,
                            "{dpi} {lw}x{lh} {screen:?}: {r:?} -> {:?}",
                            (l, t, rr, b)
                        );
                        assert!(
                            rr - l >= 8 && b - t >= 8,
                            "{dpi} {lw}x{lh}: {r:?} too small"
                        );
                        boxes.push((l, t, rr, b));
                    }
                    let controls: Vec<_> = [
                        f.pause, f.nav[0], f.nav[1], f.nav[2], f.nav[3], f.minus, f.plus, f.start,
                    ]
                    .iter()
                    .map(|r| physical(*r, dpi))
                    .collect();
                    for (i, a) in controls.iter().enumerate() {
                        for b in &controls[i + 1..] {
                            let apart = a.2 <= b.0 || b.2 <= a.0 || a.3 <= b.1 || b.3 <= a.1;
                            assert!(apart, "{dpi} {lw}x{lh} {screen:?}: {a:?} overlaps {b:?}");
                        }
                    }
                    // Page targets that are visible lie inside the body.
                    let body = physical(f.body, dpi);
                    for t in &scene.targets.0 {
                        if t.id >= 30 {
                            let (l, tp, r, b) = physical(t.rect, dpi);
                            assert!(
                                l >= body.0 - 1
                                    && tp >= body.1 - 1
                                    && r <= body.2 + 1
                                    && b <= body.3 + 1,
                                "{dpi} {screen:?} target {} {:?}",
                                t.id,
                                (l, tp, r, b)
                            );
                        }
                    }
                }
            }
        }
    }
}
