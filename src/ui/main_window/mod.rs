//! Main window: navigation rail, live card and pages, drawn with Direct2D.
//! Movement commands go only through `ui::actions::request`.
use crate::{
    application::{CommandSender, SnapshotReader, UserCommand as Input, Worker},
    control::Command,
    statistics::Day,
    ui::{
        kit::{
            input::{Id, Input as Pointer},
            layout::{Scroll, to_logical},
            render::Renderer,
            theme::{self, Theme},
        },
        model::page::Page,
    },
};
use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, PCWSTR, w},
};

mod access;
mod calendar;
mod forms;
pub(crate) mod frame;
mod pages;
mod paint;
mod scene;
use frame::*;
use pages::{
    AUTO, BACK, BAR_BASE, CANDIDATE_BASE, DATE, DISCONNECT, EFFECTIVE, FIELD_BASE, FOLDER,
    KEEP_RUNNING_ON_CLOSE, MINIMIZE_TO_TRAY, NEXT, PREV, RANGE_BASE, RANGES, RECOVER_GAPS,
    ROW_BASE, SCAN, SHORTCUTS_SAVE, START_MINIMIZED, STEP_DOWN, STEP_UP, Screen, TARGETS_SAVE,
    THEME_BASE, TODAY_BUTTON, WINDOWS_STARTUP,
};
use scene::{Context, Scene};

pub const UPDATE: u32 = WM_APP + 40;
/// Tray window commands the Settings page reuses, so the tray menu and the page behave alike.
const TRAY_CONNECT: usize = 1;
const TRAY_DISCONNECT: usize = 2;
const TRAY_FIND: usize = 3;
/// Posted to the tray window with a device address in `wparam`.
pub const SELECT_DEVICE: u32 = WM_APP + 3;
#[derive(Clone, Copy, PartialEq, Eq)]
enum CalendarFor {
    Day,
    Effective,
}
const TIP_TIMER: usize = 1;
const WM_MOUSELEAVE: u32 = 0x02A3;

pub struct Handle {
    window: Rc<Cell<Option<HWND>>>,
}
impl Default for Handle {
    fn default() -> Self {
        Self {
            window: Rc::new(Cell::new(None)),
        }
    }
}
impl Handle {
    pub fn open(&self, owner: HWND, worker: &Worker) -> Result<(), String> {
        self.open_with_show(owner, worker, SW_SHOW)
    }
    pub fn open_minimized(&self, owner: HWND, worker: &Worker) -> Result<(), String> {
        self.open_with_show(owner, worker, SW_SHOWMINNOACTIVE)
    }
    fn open_with_show(
        &self,
        owner: HWND,
        worker: &Worker,
        show: SHOW_WINDOW_CMD,
    ) -> Result<(), String> {
        if let Some(window) = self.window() {
            unsafe {
                let _ = ShowWindow(window, SW_RESTORE);
                let _ = SetForegroundWindow(window);
            }
            return Ok(());
        }
        let window = unsafe {
            create_with_show(
                owner,
                worker.sender.clone(),
                worker.view.clone(),
                self.window.clone(),
                show,
            )
        }
        .map_err(|e| e.to_string())?;
        self.window.set(Some(window));
        Ok(())
    }
    pub fn update(&self) {
        if let Some(window) = self.window() {
            unsafe {
                let _ = PostMessageW(Some(window), UPDATE, WPARAM(0), LPARAM(0));
            }
        }
    }
    pub fn close(&self) {
        if let Some(window) = self.window.take()
            && unsafe { IsWindow(Some(window)) }.as_bool()
        {
            unsafe {
                let _ = DestroyWindow(window);
            }
        }
    }
    pub fn window(&self) -> Option<HWND> {
        self.window
            .get()
            .filter(|w| unsafe { IsWindow(Some(*w)) }.as_bool())
    }
}
struct State {
    slot: Rc<Cell<Option<HWND>>>,
    owner: HWND,
    sender: CommandSender,
    view: SnapshotReader,
    renderer: OnceCell<Renderer>,
    theme: Cell<Theme>,
    theme_preference: Cell<crate::appearance::ThemePreference>,
    pointer: RefCell<Pointer>,
    scene: RefCell<Scene>,
    page: Cell<Page>,
    /// Day shown on Today; `None` follows today.
    selected: RefCell<Option<String>>,
    /// Day opened from History.
    detail: RefCell<Option<String>>,
    range: Cell<u32>,
    today: RefCell<Option<Day>>,
    sent: RefCell<Option<(String, u32, Instant)>>,
    calendar: Cell<Option<HWND>>,
    scroll: Cell<[f32; 5]>,
    size: Cell<(f32, f32)>,
    tip: Cell<Option<Id>>,
    tracking: Cell<bool>,
    closing: Cell<bool>,
    failures: Cell<u32>,
    window: Cell<HWND>,
    edits: RefCell<Vec<(Id, HWND)>>,
    font: crate::ui::native::FontSlot,
    brush: Cell<HBRUSH>,
    form: RefCell<pages::Form>,
    filled: Cell<[bool; 2]>,
    calendar_for: Cell<CalendarFor>,
    access: OnceCell<std::sync::Arc<access::provider::Shared>>,
}
impl State {
    fn screen(&self) -> Screen {
        Screen::of(self.page.get(), self.detail.borrow().is_some())
    }
    fn context(&self) -> Context {
        Context {
            selected: self.selected.borrow().clone(),
            detail: self.detail.borrow().clone(),
            range: self.range.get(),
            focus: self.pointer.borrow().focus,
            today: self.today.borrow().clone(),
            form: self.form.borrow().clone(),
        }
    }
    fn rescene(&self, view: &crate::application::View) {
        self.update_scene(view, true);
    }
    fn update_scene(&self, view: &crate::application::View, force: bool) -> bool {
        if self.theme_preference.replace(view.theme_preference) != view.theme_preference {
            self.refresh_theme(self.window.get(), view);
        }
        // Remember today's dashboard day so the live card keeps its target while
        // another day or the history range is shown.
        {
            let mut today = self.today.borrow_mut();
            if let Some(d) = view
                .dashboard
                .as_ref()
                .filter(|d| d.day.date == view.totals.day)
            {
                *today = Some(d.day.clone());
            } else if today.as_ref().is_some_and(|d| d.date != view.totals.day) {
                *today = None;
            }
        }
        let screen = self.screen();
        let ctx = self.context();
        let mut scroll = self.scroll.get();
        let scene = if force {
            Some(Scene::new(
                view,
                self.size.get(),
                screen,
                scroll[screen.slot()],
                &ctx,
            ))
        } else {
            self.scene
                .borrow()
                .updated(view, self.size.get(), screen, scroll[screen.slot()], &ctx)
        };
        let Some(scene) = scene else {
            forms::fill(self, view, screen);
            // Retry unanswered requests even if there is nothing new to paint.
            self.keep_statistics(view, screen, &ctx);
            return false;
        };
        // A screen that is still loading is short; keep the remembered position meanwhile.
        if scene.loaded {
            scroll[screen.slot()] = scene.scroll;
            self.scroll.set(scroll);
        }
        self.pointer.borrow_mut().retain_valid(&scene.targets);
        *self.scene.borrow_mut() = scene;
        forms::fill(self, view, screen);
        let window = self.window.get();
        if !window.0.is_null() {
            unsafe { forms::place(window, self) };
        }
        self.keep_statistics(view, screen, &ctx);
        true
    }
    fn refresh(&self, window: HWND) {
        self.rescene(&self.view.snapshot());
        invalidate(window);
    }
    fn refresh_theme(&self, window: HWND, view: &crate::application::View) {
        let th = theme::detect(view.development, view.theme_preference);
        self.theme.set(th);
        unsafe {
            apply_theme(window, &th);
            forms::apply_theme_to_edits(self);
        }
    }
    fn send(&self, input: Input) {
        if !self.closing.get() || matches!(input, Input::Dashboard(None)) {
            let _ = self.sender.try_send(input);
        }
    }
    /// Requests the statistics the screen shows: at once when they change, and again
    /// each second while no matching answer has been published.
    fn keep_statistics(&self, view: &crate::application::View, screen: Screen, ctx: &Context) {
        let Some(request) = scene::desired_request(view, screen, ctx) else {
            return;
        };
        let key = (request.day.clone(), request.range);
        let answered = scene::answer(view, screen, &request).is_some();
        let due = match self.sent.borrow().as_ref() {
            None => true,
            Some((day, range, at)) => {
                (day, *range) != (&key.0, key.1)
                    || (!answered && at.elapsed() >= Duration::from_secs(1))
            }
        };
        if due {
            *self.sent.borrow_mut() = Some((key.0, key.1, Instant::now()));
            self.send(Input::DashboardVisible(true));
            self.send(Input::Dashboard(Some(request)));
        }
    }
    fn hide_tip(&self, window: HWND) {
        unsafe {
            let _ = KillTimer(Some(window), TIP_TIMER);
        }
        if self.tip.take().is_some() {
            invalidate(window);
        }
    }
}
pub(crate) fn invalidate(window: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(window), None, false);
    }
}
pub(crate) unsafe fn dpi(window: HWND) -> u32 {
    unsafe { GetDpiForWindow(window).max(96) }
}
pub(crate) unsafe fn apply_theme(window: HWND, th: &Theme) {
    let bgr = |c: theme::Color| COLORREF(c.r as u32 | (c.g as u32) << 8 | (c.b as u32) << 16);
    unsafe {
        let dark = BOOL(th.is_dark() as i32);
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const BOOL).cast(),
            4,
        );
        if !th.high_contrast_mode() {
            for (attribute, color) in [(DWMWA_CAPTION_COLOR, th.rail), (DWMWA_TEXT_COLOR, th.text)]
            {
                let value = bgr(color);
                let _ =
                    DwmSetWindowAttribute(window, attribute, (&value as *const COLORREF).cast(), 4);
            }
        }
    }
}
unsafe fn create_with_show(
    owner: HWND,
    sender: CommandSender,
    view: SnapshotReader,
    slot: Rc<Cell<Option<HWND>>>,
    show: SHOW_WINDOW_CMD,
) -> windows::core::Result<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: w!("DeskStepsMain"),
            style: CS_DBLCLKS,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        // Repeated registration in the same process is harmless.
        let _ = RegisterClassW(&class);
        let development = view.snapshot().development;
        let theme_preference = view.snapshot().theme_preference;
        let state = Rc::new(State {
            slot,
            owner,
            sender: sender.clone(),
            theme: Cell::new(theme::detect(development, theme_preference)),
            theme_preference: Cell::new(theme_preference),
            scene: RefCell::new(Scene::new(
                &view.snapshot(),
                (DEFAULT_WIDTH, DEFAULT_HEIGHT),
                Screen::Today,
                0.0,
                &Context {
                    range: 30,
                    ..Default::default()
                },
            )),
            view,
            renderer: OnceCell::new(),
            pointer: RefCell::new(Pointer::default()),
            page: Cell::new(Page::Today),
            selected: RefCell::new(None),
            detail: RefCell::new(None),
            range: Cell::new(30),
            today: RefCell::new(None),
            sent: RefCell::new(None),
            calendar: Cell::new(None),
            scroll: Cell::new([0.0; 5]),
            size: Cell::new((DEFAULT_WIDTH, DEFAULT_HEIGHT)),
            tip: Cell::new(None),
            tracking: Cell::new(false),
            closing: Cell::new(false),
            failures: Cell::new(0),
            window: Cell::new(HWND::default()),
            edits: RefCell::new(Vec::new()),
            font: crate::ui::native::FontSlot::new(),
            brush: Cell::new(HBRUSH::default()),
            form: RefCell::new(pages::Form::default()),
            filled: Cell::new([false; 2]),
            calendar_for: Cell::new(CalendarFor::Day),
            access: OnceCell::new(),
        });
        let ptr = Rc::into_raw(state.clone());
        let _ = sender.try_send(Input::DashboardVisible(true));
        let scale = dpi(owner);
        let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
        let mut outer = RECT {
            right: crate::ui::kit::layout::to_physical(DEFAULT_WIDTH, scale),
            bottom: crate::ui::kit::layout::to_physical(DEFAULT_HEIGHT, scale),
            ..Default::default()
        };
        let _ = AdjustWindowRectExForDpi(&mut outer, style, false, WS_EX_APPWINDOW, scale);
        let title = crate::ui::native::wide(crate::identity::title(development));
        // WM_NCCREATE takes ownership; WM_NCDESTROY releases the state.
        let result = CreateWindowExW(
            WS_EX_APPWINDOW,
            w!("DeskStepsMain"),
            PCWSTR(title.as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            outer.right - outer.left,
            outer.bottom - outer.top,
            Some(owner),
            None,
            Some(instance),
            Some(ptr.cast()),
        );
        let window = match result {
            Ok(window) => window,
            Err(error) => {
                if !state.closing.get() {
                    drop(Rc::from_raw(ptr));
                }
                let _ = sender.try_send(Input::DashboardVisible(false));
                return Err(error);
            }
        };
        crate::ui::native::inherit_window_icons(window, owner);
        let _ = ShowWindow(window, show);
        let _ = UpdateWindow(window);
        if show != SW_SHOWMINNOACTIVE {
            let _ = SetForegroundWindow(window);
        }
        Ok(window)
    }
}
unsafe fn select_page(window: HWND, s: &State, page: Page) {
    s.page.set(page);
    // Choosing a page again returns to its top level: today, or the History list.
    s.detail.borrow_mut().take();
    if page == Page::Today {
        s.selected.borrow_mut().take();
    }
    s.hide_tip(window);
    s.refresh(window);
}
fn today_of(s: &State) -> String {
    s.view.snapshot().totals.day
}
/// Shows `date` on Today. Today itself follows the clock instead of pinning a date.
unsafe fn show_day(window: HWND, s: &State, date: &str) {
    *s.selected.borrow_mut() = (date != today_of(s)).then(|| date.to_string());
    let mut all = s.scroll.get();
    all[Screen::Today.slot()] = 0.0;
    s.scroll.set(all);
    s.hide_tip(window);
    s.refresh(window);
}
unsafe fn open_day(window: HWND, s: &State, date: String) {
    *s.detail.borrow_mut() = Some(date);
    let mut all = s.scroll.get();
    all[Screen::HistoryDay.slot()] = 0.0;
    s.scroll.set(all);
    s.hide_tip(window);
    s.refresh(window);
}
unsafe fn step_day(window: HWND, s: &State, delta: i64) {
    let shown = s.selected.borrow().clone().unwrap_or_else(|| today_of(s));
    if let Ok(date) = crate::statistics::date(&shown)
        && let Some(next) = date.checked_add_signed(chrono::Duration::days(delta))
    {
        unsafe { show_day(window, s, &next.to_string()) };
    }
}
unsafe fn toggle_calendar(window: HWND, s: &State, target: CalendarFor) {
    if let Some(open) = s.calendar.take()
        && unsafe { IsWindow(Some(open)) }.as_bool()
    {
        unsafe {
            let _ = DestroyWindow(open);
        }
        return;
    }
    let (shown, anchor) = match target {
        CalendarFor::Day => (
            s.selected.borrow().clone().unwrap_or_else(|| today_of(s)),
            DATE,
        ),
        CalendarFor::Effective => (s.scene.borrow().data.form.effective.clone(), EFFECTIVE),
    };
    let Ok(date) = crate::statistics::date(&shown) else {
        return;
    };
    let Some(button) = s.scene.borrow().targets.rect(anchor) else {
        return;
    };
    s.calendar_for.set(target);
    unsafe {
        let scale = dpi(window) as f32 / 96.0;
        let mut at = POINT {
            x: (button.x * scale) as i32,
            y: (button.bottom() * scale) as i32 + 4,
        };
        let _ = ClientToScreen(window, &mut at);
        if let Ok(popup) = calendar::open(window, at, date) {
            s.calendar.set(Some(popup));
        }
    }
}
/// `by_key` is true for Enter/Space: a list row opens its day only then (or on double-click).
/// Runs a tray menu command; the tray owns the worker handle and the device chooser state.
fn tray_command(s: &State, command: usize) {
    unsafe {
        let _ = PostMessageW(Some(s.owner), WM_COMMAND, WPARAM(command), LPARAM(0));
    }
}
unsafe fn activate(window: HWND, s: &State, id: Id, by_key: bool) {
    if let Some(page) = nav_page(id) {
        unsafe { select_page(window, s, page) };
        return;
    }
    if (ROW_BASE..BAR_BASE).contains(&id) {
        let date = s.scene.borrow().date_of(id);
        if by_key && let Some(date) = date {
            unsafe { open_day(window, s, date) };
        }
        return;
    }
    if Scene::is_bar(id) {
        let date = s.scene.borrow().date_of(id);
        if let Some(date) = date {
            unsafe { open_day(window, s, date) };
        }
        return;
    }
    let command = match id {
        START => Command::StartResume,
        PAUSE => Command::Pause,
        MINUS => Command::Decrease,
        PLUS => Command::Increase,
        PREV => return unsafe { step_day(window, s, -1) },
        NEXT => return unsafe { step_day(window, s, 1) },
        DATE => return unsafe { toggle_calendar(window, s, CalendarFor::Day) },
        EFFECTIVE => return unsafe { toggle_calendar(window, s, CalendarFor::Effective) },
        TARGETS_SAVE => return unsafe { forms::save_targets(window, s) },
        SHORTCUTS_SAVE => return unsafe { forms::save_shortcuts(window, s) },
        AUTO => return tray_command(s, TRAY_CONNECT),
        START_MINIMIZED => {
            s.send(Input::WindowPreference(
                crate::window_behavior::Preference::StartMinimized(
                    !s.scene.borrow().data.window_behavior.start_minimized,
                ),
            ));
            return;
        }
        WINDOWS_STARTUP => {
            let data = &s.scene.borrow().data;
            if data.windows_startup_available {
                s.send(Input::WindowsStartup(!data.windows_startup));
            }
            return;
        }
        MINIMIZE_TO_TRAY => {
            s.send(Input::WindowPreference(
                crate::window_behavior::Preference::MinimizeToTray(
                    !s.scene.borrow().data.window_behavior.minimize_to_tray,
                ),
            ));
            return;
        }
        KEEP_RUNNING_ON_CLOSE => {
            s.send(Input::WindowPreference(
                crate::window_behavior::Preference::KeepRunningOnClose(
                    !s.scene.borrow().data.window_behavior.keep_running_on_close,
                ),
            ));
            return;
        }
        RECOVER_GAPS => {
            s.send(Input::RecoverGaps(!s.scene.borrow().data.recover_gaps));
            return;
        }
        x if (THEME_BASE..THEME_BASE + 3).contains(&x) => {
            s.send(Input::Theme(
                crate::appearance::ThemePreference::ALL[(x - THEME_BASE) as usize],
            ));
            return;
        }
        SCAN => return tray_command(s, TRAY_FIND),
        DISCONNECT => return tray_command(s, TRAY_DISCONNECT),
        FOLDER => return tray_command(s, crate::ui::tray::OPEN_DATA_FOLDER),
        STEP_DOWN | STEP_UP => {
            let now = s.scene.borrow().data.increment as i32;
            let next = (now + if id == STEP_UP { 1 } else { -1 }).clamp(1, 10);
            if next != now {
                s.send(Input::Increment(next as u8));
            }
            invalidate(window);
            return;
        }
        x if (CANDIDATE_BASE..CANDIDATE_BASE + 64).contains(&x) => {
            let address = s
                .scene
                .borrow()
                .data
                .devices
                .candidates
                .get((x - CANDIDATE_BASE) as usize)
                .map(|c| c.0);
            if let Some(address) = address {
                unsafe {
                    let _ = PostMessageW(
                        Some(s.owner),
                        SELECT_DEVICE,
                        WPARAM(address as usize),
                        LPARAM(0),
                    );
                }
            }
            return;
        }
        TODAY_BUTTON => return unsafe { show_day(window, s, &today_of(s)) },
        BACK => {
            s.detail.borrow_mut().take();
            s.hide_tip(window);
            s.refresh(window);
            return;
        }
        x if (RANGE_BASE..RANGE_BASE + RANGES.len() as Id).contains(&x) => {
            s.range.set(RANGES[(x - RANGE_BASE) as usize]);
            s.refresh(window);
            return;
        }
        _ => return,
    };
    crate::ui::actions::request(&s.sender, &s.view, command);
    invalidate(window);
}
unsafe fn scroll_by(window: HWND, s: &State, delta: f32) {
    let (content, view) = {
        let scene = s.scene.borrow();
        (scene.content_height, scene.frame.body.h)
    };
    let page = s.screen().slot();
    let mut all = s.scroll.get();
    let mut scroll = Scroll { offset: all[page] };
    scroll.by(delta, content, view);
    all[page] = scroll.offset;
    s.scroll.set(all);
    s.hide_tip(window);
    s.refresh(window);
}
pub(crate) fn point(window: HWND, lparam: LPARAM) -> (f32, f32) {
    let scale = unsafe { dpi(window) };
    (
        to_logical((lparam.0 & 0xffff) as i16 as i32, scale),
        to_logical(((lparam.0 >> 16) & 0xffff) as i16 as i32, scale),
    )
}
unsafe fn key_down(window: HWND, s: &State, key: u16, repeat: bool) {
    let held = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } < 0;
    s.hide_tip(window);
    if held(VK_CONTROL) && (b'1' as u16..=b'4' as u16).contains(&key) {
        if let Some(page) = Page::from_index((key - b'1' as u16) as usize) {
            unsafe { select_page(window, s, page) };
        }
        return;
    }
    let ids = nav_ids();
    let body = s.scene.borrow().frame.body.h;
    match VIRTUAL_KEY(key) {
        VK_TAB => unsafe { tab_focus(window, s, held(VK_SHIFT)) },
        VK_RETURN | VK_SPACE if !repeat => {
            let id = {
                let scene = s.scene.borrow();
                s.pointer.borrow_mut().key_activate(&scene.targets)
            };
            invalidate(window);
            if let Some(id) = id {
                unsafe { activate(window, s, id, true) };
            }
        }
        VK_UP | VK_DOWN => {
            let delta = if VIRTUAL_KEY(key) == VK_UP { -1 } else { 1 };
            let focus = s.pointer.borrow().focus;
            let moved = if focus.is_some_and(|f| (ROW_BASE..BAR_BASE).contains(&f)) {
                unsafe { step_row(window, s, focus.unwrap(), delta) };
                true
            } else {
                let scene = s.scene.borrow();
                let mut pointer = s.pointer.borrow_mut();
                pointer.focus.is_some_and(|f| ids.contains(&f))
                    && pointer.step_within(&scene.targets, &ids, delta).is_some()
            };
            if moved {
                invalidate(window);
            } else {
                let delta = if VIRTUAL_KEY(key) == VK_UP {
                    -40.0
                } else {
                    40.0
                };
                unsafe { scroll_by(window, s, delta) };
            }
        }
        VK_PRIOR => unsafe { scroll_by(window, s, -body) },
        VK_NEXT => unsafe { scroll_by(window, s, body) },
        VK_HOME => unsafe { scroll_by(window, s, -1e6) },
        VK_END => unsafe { scroll_by(window, s, 1e6) },
        VK_ESCAPE => {
            if s.detail.borrow().is_some() {
                unsafe { activate(window, s, BACK, true) };
            } else {
                s.pointer.borrow_mut().focus_visible = false;
                invalidate(window);
            }
        }
        _ => {}
    }
}
/// Moves focus to the neighbouring history row, scrolling it into view first so it exists
/// as a target when the scene is rebuilt.
unsafe fn step_row(window: HWND, s: &State, from: Id, delta: i32) {
    let (next, rect, content, view) = {
        let scene = s.scene.borrow();
        let rows: Vec<Id> = scene
            .blocks
            .iter()
            .filter_map(|(_, b)| match b {
                pages::Block::DayRow { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        let at = rows.iter().position(|r| *r == from).unwrap_or(0) as i32;
        let next = rows[(at + delta).clamp(0, rows.len() as i32 - 1) as usize];
        (
            next,
            scene.content_rect(next),
            scene.content_height,
            scene.frame.body.h,
        )
    };
    let slot = s.screen().slot();
    let mut all = s.scroll.get();
    let mut scroll = Scroll { offset: all[slot] };
    if let Some(rect) = rect {
        scroll.reveal(rect, content, view);
    }
    all[slot] = scroll.offset;
    s.scroll.set(all);
    {
        let mut pointer = s.pointer.borrow_mut();
        pointer.focus = Some(next);
        pointer.focus_visible = true;
    }
    s.refresh(window);
}
unsafe fn tab_focus(window: HWND, s: &State, backwards: bool) {
    {
        let scene = s.scene.borrow();
        s.pointer.borrow_mut().tab(&scene.targets, backwards);
    }
    unsafe { forms::sync_focus(window, s) };
    invalidate(window);
}
unsafe fn paint_window(window: HWND, s: &State) {
    if let Some(shared) = s.access.get() {
        let items = {
            let scene = s.scene.borrow();
            let focus = s.pointer.borrow().focus;
            access::build(&scene, s.page.get(), s.screen(), focus)
        };
        shared.publish(items, unsafe { dpi(window) } as f32 / 96.0);
    }
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        BeginPaint(window, &mut ps);
        if let Some(renderer) = s.renderer.get() {
            let th = s.theme.get();
            let result = {
                let scene = s.scene.borrow();
                let pointer = *s.pointer.borrow();
                renderer.paint(th.background, |p| {
                    paint::draw(
                        p,
                        &th,
                        &scene,
                        &pointer,
                        s.page.get(),
                        s.screen(),
                        s.tip.get(),
                    )
                })
            };
            if result.is_err() && s.failures.get() < 3 {
                s.failures.set(s.failures.get() + 1);
                invalidate(window);
            } else if result.is_ok() {
                s.failures.set(0);
            }
        }
        let _ = EndPaint(window, &ps);
    }
}
unsafe extern "system" fn proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
            return DefWindowProcW(window, message, wparam, lparam);
        }
        let ptr = GetWindowLongPtrW(window, GWLP_USERDATA) as *const State;
        if ptr.is_null() {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        // Modal dialogs pump messages; keep the state alive for every active callback.
        Rc::increment_strong_count(ptr);
        let guard = Rc::from_raw(ptr);
        let s = &*guard;
        match message {
            WM_CREATE => match Renderer::new(window, dpi(window)) {
                Ok(renderer) => {
                    let _ = s.renderer.set(renderer);
                    s.window.set(window);
                    let _ = s.access.set(access::provider::Shared::new(window));
                    apply_theme(window, &s.theme.get());
                    if let Err(error) = forms::create_edits(window, s) {
                        crate::ui::dialogs::error_dialog(&error.to_string());
                        return LRESULT(-1);
                    }
                }
                Err(error) => {
                    crate::ui::dialogs::error_dialog(&format!("Graphics unavailable: {error}"));
                    return LRESULT(-1);
                }
            },
            WM_GETOBJECT => {
                if let Some(shared) = s.access.get()
                    && let Some(result) =
                        access::provider::get_object(shared, window, wparam, lparam)
                {
                    return result;
                }
                return DefWindowProcW(window, message, wparam, lparam);
            }
            access::ACCESS_INVOKE => {
                // Same availability checks and command path as a click or Enter.
                let id = wparam.0 as Id;
                if s.scene.borrow().targets.enabled(id) {
                    activate(window, s, id, true);
                }
            }
            access::ACCESS_FOCUS => {
                let id = wparam.0 as Id;
                if s.scene.borrow().targets.enabled(id) {
                    {
                        let mut pointer = s.pointer.borrow_mut();
                        pointer.focus = Some(id);
                        pointer.focus_visible = false;
                    }
                    forms::sync_focus(window, s);
                    invalidate(window);
                }
            }
            WM_ERASEBKGND => return LRESULT(1),
            forms::CTLCOLOREDIT => return forms::colors(s, HDC(wparam.0 as *mut _)),
            WM_COMMAND => {
                forms::notification(s, wparam.0 & 0xffff, wparam.0 >> 16);
            }
            forms::FIELD_TAB => tab_focus(window, s, wparam.0 != 0),
            WM_PAINT => paint_window(window, s),
            WM_SIZE => {
                if wparam.0 != SIZE_MINIMIZED as usize {
                    let (w, h) = (
                        (lparam.0 & 0xffff) as i32,
                        ((lparam.0 >> 16) & 0xffff) as i32,
                    );
                    if let Some(renderer) = s.renderer.get() {
                        renderer.resize(w as u32, h as u32);
                    }
                    let scale = dpi(window);
                    s.size.set((to_logical(w, scale), to_logical(h, scale)));
                    s.refresh(window);
                }
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                let scale = dpi(window);
                let mut r = RECT {
                    right: crate::ui::kit::layout::to_physical(MIN_WIDTH, scale),
                    bottom: crate::ui::kit::layout::to_physical(MIN_HEIGHT, scale),
                    ..Default::default()
                };
                let style = WINDOW_STYLE(GetWindowLongPtrW(window, GWL_STYLE) as u32);
                let _ = AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), scale);
                info.ptMinTrackSize.x = r.right - r.left;
                info.ptMinTrackSize.y = r.bottom - r.top;
            }
            WM_DPICHANGED => {
                if let Some(renderer) = s.renderer.get() {
                    renderer.set_dpi((wparam.0 & 0xffff) as u32);
                }
                let r = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    window,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                forms::apply_font(window, s);
                s.refresh(window);
            }
            UPDATE => {
                let view = s.view.snapshot();
                if s.update_scene(&view, false) {
                    invalidate(window);
                }
            }
            WM_MOUSEMOVE => {
                if !s.tracking.replace(true) {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: window,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut track);
                }
                let (x, y) = point(window, lparam);
                let changed = {
                    let scene = s.scene.borrow();
                    s.pointer.borrow_mut().mouse_move(&scene.targets, x, y)
                };
                if changed {
                    s.hide_tip(window);
                    match s.pointer.borrow().hover {
                        // Chart bars explain themselves at once.
                        Some(id) if Scene::is_bar(id) => s.tip.set(Some(id)),
                        Some(_) => {
                            SetTimer(Some(window), TIP_TIMER, 500, None);
                        }
                        None => {}
                    }
                    invalidate(window);
                }
            }
            WM_MOUSELEAVE => {
                s.tracking.set(false);
                s.hide_tip(window);
                if s.pointer.borrow_mut().mouse_leave() {
                    invalidate(window);
                }
            }
            WM_TIMER if wparam.0 == TIP_TIMER => {
                let _ = KillTimer(Some(window), TIP_TIMER);
                let hover = s.pointer.borrow().hover;
                let scene = s.scene.borrow();
                if let Some(id) = hover
                    && tooltip_text(id, &scene.card, scene.frame.collapsed).is_some()
                {
                    s.tip.set(Some(id));
                    invalidate(window);
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                s.hide_tip(window);
                let _ = SetFocus(Some(window));
                let (x, y) = point(window, lparam);
                let row = {
                    let scene = s.scene.borrow();
                    s.pointer.borrow_mut().mouse_down(&scene.targets, x, y);
                    scene
                        .targets
                        .hit(x, y)
                        .filter(|id| (ROW_BASE..BAR_BASE).contains(id))
                };
                if s.pointer.borrow().pressed.is_some() {
                    SetCapture(window);
                }
                invalidate(window);
                if message == WM_LBUTTONDBLCLK {
                    let date = row.and_then(|id| s.scene.borrow().date_of(id));
                    if let Some(date) = date {
                        open_day(window, s, date);
                    }
                }
            }
            WM_LBUTTONUP => {
                let (x, y) = point(window, lparam);
                let id = {
                    let scene = s.scene.borrow();
                    s.pointer.borrow_mut().mouse_up(&scene.targets, x, y)
                };
                // After the press is resolved: this sends WM_CAPTURECHANGED.
                let _ = ReleaseCapture();
                invalidate(window);
                if let Some(id) = id {
                    activate(window, s, id, false);
                }
            }
            calendar::PICKED => {
                if let Some(date) = calendar::decode(wparam.0) {
                    if s.calendar_for.get() == CalendarFor::Effective {
                        let mut form = s.form.borrow_mut();
                        form.effective = date.to_string();
                        form.errors[0] = None;
                        drop(form);
                        s.refresh(window);
                    } else {
                        show_day(window, s, &date.to_string());
                    }
                }
            }
            WM_CAPTURECHANGED => {
                if s.pointer.borrow_mut().pressed.take().is_some() {
                    invalidate(window);
                }
            }
            WM_MOUSEWHEEL => {
                let ticks = ((wparam.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
                scroll_by(window, s, -ticks * 48.0);
            }
            WM_KEYDOWN => key_down(window, s, wparam.0 as u16, lparam.0 & (1 << 30) != 0),
            WM_SETTINGCHANGE
            | WM_THEMECHANGED
            | WM_SYSCOLORCHANGE
            | WM_DWMCOLORIZATIONCOLORCHANGED => {
                s.refresh_theme(window, &s.view.snapshot());
                invalidate(window);
            }
            WM_SYSCOMMAND
                if wparam.0 & 0xfff0 == SC_MINIMIZE as usize
                    && s.view.snapshot().window_behavior.minimize_to_tray =>
            {
                s.hide_tip(window);
                let _ = ShowWindow(window, SW_HIDE);
            }
            WM_CLOSE => {
                if s.view.snapshot().window_behavior.keep_running_on_close {
                    let _ = DestroyWindow(window);
                } else {
                    // The tray owner shuts down both windows and joins the recorder.
                    let _ = PostMessageW(Some(s.owner), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            WM_DESTROY => {
                s.closing.set(true);
                s.slot.set(None);
                let _ = KillTimer(Some(window), TIP_TIMER);
                access::provider::disconnect(window);
                if let Some(popup) = s.calendar.take() {
                    let _ = DestroyWindow(popup);
                }
                let _ = s.sender.try_send(Input::DashboardVisible(false));
                s.send(Input::Dashboard(None));
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                forms::free_brush(s);
                s.closing.set(true);
                s.slot.set(None);
                let owned = Rc::from_raw(ptr);
                drop(owned);
                return DefWindowProcW(window, message, wparam, lparam);
            }
            _ => return DefWindowProcW(window, message, wparam, lparam),
        }
        LRESULT(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod screenshots;
    use crate::application::{View, testing::channels};
    use crate::ui::native::read_text;
    unsafe fn create(
        owner: HWND,
        sender: CommandSender,
        view: SnapshotReader,
        slot: Rc<Cell<Option<HWND>>>,
    ) -> windows::core::Result<HWND> {
        unsafe { create_with_show(owner, sender, view, slot, SW_SHOW) }
    }
    unsafe fn owner() -> HWND {
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Fixture"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        }
    }
    fn view() -> View {
        View {
            connected: true,
            controls_enabled: true,
            speed_available: true,
            increment: 1,
            fresh_speed: Some(3.0),
            adjustable_speed: Some(3.0),
            generation: 5,
            ..Default::default()
        }
    }
    unsafe fn state<'a>(window: HWND) -> &'a State {
        unsafe { &*(GetWindowLongPtrW(window, GWLP_USERDATA) as *const State) }
    }
    #[test]
    fn native_ui_main_window_skips_redundant_paints_but_updates_freshness_theme_and_forms() {
        let (sender, reader, raw, _rx) = channels(64, view());
        unsafe {
            let owner = owner();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            let s = state(window);
            for page in Page::ALL {
                select_page(window, s, page);
                let _ = ValidateRect(Some(window), None);
                raw.lock().unwrap().simulator_status = "A new diagnostic clock tick".into();
                raw.lock().unwrap().last_sample = Some("12:34:56".into());
                SendMessageW(window, UPDATE, None, None);
                assert!(
                    !GetUpdateRect(window, None, false).as_bool(),
                    "unchanged page {page:?} was invalidated"
                );
            }
            raw.lock().unwrap().fresh_speed = None;
            raw.lock().unwrap().adjustable_speed = None;
            SendMessageW(window, UPDATE, None, None);
            assert!(GetUpdateRect(window, None, false).as_bool());
            assert!(!s.scene.borrow().card.plus);
            let _ = ValidateRect(Some(window), None);
            raw.lock().unwrap().theme_preference = crate::appearance::ThemePreference::Dark;
            SendMessageW(window, UPDATE, None, None);
            assert!(GetUpdateRect(window, None, false).as_bool());
            raw.lock().unwrap().totals.day = "2026-10-23".into();
            select_page(window, s, Page::Targets);
            raw.lock().unwrap().dashboard = Some(
                crate::statistics::Dashboard {
                    day: Day {
                        date: "2026-10-23".into(),
                        preferences: crate::statistics::Preferences {
                            weight: Some(72.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    today: "2026-10-23".into(),
                    ..Default::default()
                }
                .into(),
            );
            let late = raw.lock().unwrap().clone();
            assert!(
                !s.update_scene(&late, false),
                "late form defaults need no main-window repaint"
            );
            assert_eq!(forms::text(s, FIELD_BASE), "72");
            forms::put_text(s, FIELD_BASE + 1, "777");
            raw.lock().unwrap().dashboard = Some(dash("2026-10-23", 1).into());
            SendMessageW(window, UPDATE, None, None);
            assert_eq!(
                forms::text(s, FIELD_BASE + 1),
                "777",
                "a later statistics refresh must preserve unsaved input"
            );
            assert!(s.scene.borrow().card.steps.contains("1,000"));
            assert_eq!(s.pointer.borrow().focus, None);
            SendMessageW(window, WM_CLOSE, None, None);
            let _ = DestroyWindow(owner);
        }
    }
    #[test]
    fn native_ui_main_window_pages_resize_input_theme_and_close() {
        let (sender, reader, raw, rx) = channels(16, view());
        unsafe {
            let owner = owner();
            let slot = Rc::new(Cell::new(None));
            let window = create(owner, sender, reader, slot.clone()).unwrap();
            assert_eq!(slot.get(), None, "Handle owns the slot, not create");
            assert_eq!(read_text(window), "DeskSteps");
            let s = state(window);
            assert!(s.renderer.get().is_some());
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            // Every page is reachable by Ctrl+number navigation and by activation.
            for page in Page::ALL {
                activate(window, s, nav_id(page), false);
                assert_eq!(s.page.get(), page);
                SendMessageW(window, WM_PAINT, None, None);
                assert!(s.renderer.get().unwrap().has_target());
            }
            // A real press and release on a rail item navigates.
            activate(window, s, nav_id(Page::Today), false);
            let at = s
                .scene
                .borrow()
                .targets
                .rect(nav_id(Page::History))
                .unwrap();
            let scale = dpi(window) as f32 / 96.0;
            let l =
                LPARAM((((at.y + 5.0) * scale) as isize) << 16 | ((at.x + 5.0) * scale) as isize);
            SendMessageW(window, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(l));
            assert_eq!(s.page.get(), Page::Today, "activation waits for release");
            SendMessageW(window, WM_LBUTTONUP, None, Some(l));
            assert_eq!(s.page.get(), Page::History);
            // Resize across the rail breakpoint.
            for (w, h) in [(560, 440), (1400, 900), (820, 580)] {
                let _ = SetWindowPos(window, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER);
                let size = s.size.get();
                assert!(size.0 > 0.0 && size.1 > 0.0);
                assert_eq!(s.scene.borrow().frame.collapsed, size.0 < 720.0);
                UpdateWindow(window).ok().ok();
            }
            // Movement buttons use the shared request path and the published generation.
            activate(window, s, PAUSE, false);
            assert!(matches!(
                rx.recv().unwrap(),
                Input::Control {
                    command: Command::Pause,
                    generation: 5
                }
            ));
            activate(window, s, PLUS, false);
            assert!(matches!(
                rx.recv().unwrap(),
                Input::Control {
                    command: Command::Increase,
                    generation: 5
                }
            ));
            // Start is disabled while the belt moves; the request path still refuses it.
            assert!(!s.scene.borrow().card.start);
            // Keyboard: Tab focuses the first enabled target, arrows move within the rail.
            key_down(window, s, VK_TAB.0, false);
            assert!(s.pointer.borrow().focus_visible);
            let focus = s.pointer.borrow().focus.unwrap();
            assert!(s.scene.borrow().targets.enabled(focus));
            key_down(window, s, b'3' as u16, false);
            // Theme change keeps painting.
            SendMessageW(window, WM_SETTINGCHANGE, None, None);
            SendMessageW(window, WM_PAINT, None, None);
            // Snapshot updates refresh the live card and request statistics for the target.
            raw.lock().unwrap().totals.day = "2026-10-23".into();
            SendMessageW(window, UPDATE, None, None);
            assert_eq!(s.scene.borrow().card.connection, "Connected");
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            assert!(matches!(rx.recv().unwrap(), Input::Dashboard(Some(_))));
            // Simulated device loss is recovered on the next paint.
            s.renderer.get().unwrap().discard_target();
            SendMessageW(window, WM_PAINT, None, None);
            assert!(s.renderer.get().unwrap().has_target());
            SendMessageW(window, WM_CLOSE, None, None);
            assert!(!IsWindow(Some(window)).as_bool());
            let _ = DestroyWindow(owner);
        }
    }
    fn dash(day: &str, range: usize) -> crate::statistics::Dashboard {
        use crate::statistics::{Dashboard, Day, Preferences};
        let end = crate::statistics::date(day).unwrap();
        let days: Vec<Day> = (0..range)
            .rev()
            .map(|back| Day {
                date: (end - chrono::Duration::days(back as i64)).to_string(),
                steps: 100 + back as i64,
                samples: 1,
                sessions: 1,
                ..Default::default()
            })
            .collect();
        let mut last = days.last().cloned().unwrap_or_default();
        last.date = day.into();
        last.steps = 321;
        last.preferences = Preferences {
            steps: Some(1000),
            ..Default::default()
        };
        Dashboard {
            day: last,
            days,
            today: "2026-10-23".into(),
            timezone: "UTC".into(),
            ..Default::default()
        }
    }
    /// Reads the (day, range) of the next statistics request; each is preceded by a
    /// DashboardVisible(true).
    fn next_request(rx: &crate::application::testing::Commands) -> (String, u32) {
        assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
        match rx.recv().unwrap() {
            Input::Dashboard(Some(r)) => (r.day, r.range),
            _ => panic!("expected a statistics request"),
        }
    }
    #[test]
    fn native_ui_main_window_today_history_day_detail_and_keyboard_navigation() {
        let (sender, reader, raw, rx) = channels(64, view());
        unsafe {
            let owner = owner();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            let s = state(window);
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            let serve = |day: &str, range: usize| {
                raw.lock().unwrap().dashboard = Some(dash(day, range).into());
                SendMessageW(window, UPDATE, None, None);
            };
            // Today follows the clock and asks for today only.
            raw.lock().unwrap().totals.day = "2026-10-23".into();
            SendMessageW(window, UPDATE, None, None);
            assert_eq!(next_request(&rx), ("2026-10-23".into(), 1));
            assert!(s.scene.borrow().data.day.is_none(), "no answer yet");
            serve("2026-10-23", 1);
            assert_eq!(s.scene.borrow().data.day.as_ref().unwrap().title, "Today");
            assert_eq!(
                s.scene.borrow().data.day.as_ref().unwrap().tiles[0].value,
                "321"
            );
            // The rail reads the same day: 321 of 1,000.
            assert_eq!(s.scene.borrow().card.steps, "321/1,000 steps · 32%");
            // Previous day: loading until the worker answers, and the rail keeps its target.
            activate(window, s, PREV, false);
            assert_eq!(next_request(&rx), ("2026-10-22".into(), 1));
            assert!(s.scene.borrow().data.day.is_none());
            assert_eq!(s.scene.borrow().card.steps, "321/1,000 steps · 32%");
            serve("2026-10-22", 1);
            let title = s.scene.borrow().data.day.as_ref().unwrap().title.clone();
            assert_eq!(title, "Thu 22 Oct 2026");
            assert!(s.scene.borrow().targets.enabled(NEXT));
            // Calendar pick, then Back to today.
            SendMessageW(window, calendar::PICKED, Some(WPARAM(20261020)), None);
            assert_eq!(next_request(&rx), ("2026-10-20".into(), 1));
            activate(window, s, TODAY_BUTTON, false);
            assert_eq!(next_request(&rx), ("2026-10-23".into(), 1));
            assert!(s.selected.borrow().is_none(), "follows today again");
            serve("2026-10-23", 1);
            assert!(
                !s.scene.borrow().targets.enabled(NEXT),
                "nothing after today"
            );
            // History: 30 days by default, 90 on request.
            activate(window, s, nav_id(Page::History), false);
            assert_eq!(next_request(&rx), ("2026-10-23".into(), 30));
            serve("2026-10-23", 30);
            assert_eq!(
                s.scene.borrow().data.history.as_ref().unwrap().rows.len(),
                30
            );
            activate(window, s, RANGE_BASE + 2, false);
            assert_eq!(next_request(&rx), ("2026-10-23".into(), 90));
            assert!(s.scene.borrow().data.history.is_none());
            serve("2026-10-23", 90);
            assert_eq!(
                s.scene.borrow().data.history.as_ref().unwrap().bars.len(),
                90
            );
            SendMessageW(window, WM_PAINT, None, None);
            // Hovering a bar explains it at once; the window shows the chart at the top.
            let bar = s
                .scene
                .borrow()
                .targets
                .rect(BAR_BASE + 10)
                .expect("chart is visible");
            let scale = dpi(window) as f32 / 96.0;
            let l =
                LPARAM((((bar.y + 5.0) * scale) as isize) << 16 | ((bar.x + 1.0) * scale) as isize);
            SendMessageW(window, WM_MOUSEMOVE, None, Some(l));
            assert_eq!(s.pointer.borrow().hover, Some(BAR_BASE + 10));
            assert_eq!(s.tip.get(), Some(BAR_BASE + 10), "bar tooltip is immediate");
            assert!(
                s.scene
                    .borrow()
                    .tooltip(BAR_BASE + 10)
                    .unwrap()
                    .contains("steps")
            );
            SendMessageW(window, WM_PAINT, None, None);
            // Scroll, then click a bar: its day opens and Back restores the scroll.
            scroll_by(window, s, 200.0);
            let scrolled = s.scroll.get()[Screen::History.slot()];
            assert!(scrolled > 0.0);
            assert!(s.tip.get().is_none(), "scrolling hides the tooltip");
            // After scrolling the chart may be off screen; open the bar by id as a click would.
            activate(window, s, BAR_BASE + 10, false);
            let opened = s.detail.borrow().clone().unwrap();
            assert_eq!(next_request(&rx), (opened.clone(), 1));
            serve(&opened, 1);
            assert_eq!(s.screen(), Screen::HistoryDay);
            assert_eq!(s.scene.borrow().data.day.as_ref().unwrap().date, opened);
            key_down(window, s, VK_ESCAPE.0, false);
            assert!(s.detail.borrow().is_none());
            assert_eq!(next_request(&rx), ("2026-10-23".into(), 90));
            serve("2026-10-23", 90);
            assert_eq!(s.scroll.get()[Screen::History.slot()], scrolled);
            // Keyboard: arrows move through rows, Enter opens, and Tab leaves the list.
            scroll_by(window, s, -1e6);
            key_down(window, s, VK_TAB.0, false);
            let mut guard = 0;
            while s.pointer.borrow().focus != Some(ROW_BASE) && guard < 40 {
                key_down(window, s, VK_TAB.0, false);
                guard += 1;
            }
            assert_eq!(
                s.pointer.borrow().focus,
                Some(ROW_BASE),
                "first row is a tab stop"
            );
            for _ in 0..40 {
                key_down(window, s, VK_DOWN.0, false);
            }
            let focus = s.pointer.borrow().focus.unwrap();
            assert_eq!(focus, ROW_BASE + 40);
            assert!(
                s.scene.borrow().targets.enabled(focus),
                "row scrolled into view"
            );
            assert!(s.scroll.get()[Screen::History.slot()] > 0.0);
            key_down(window, s, VK_TAB.0, false);
            assert!(
                s.pointer
                    .borrow()
                    .focus
                    .is_some_and(|f| !(ROW_BASE..BAR_BASE).contains(&f))
            );
            s.pointer.borrow_mut().focus = Some(ROW_BASE + 40);
            let expected = s.scene.borrow().date_of(ROW_BASE + 40).unwrap();
            key_down(window, s, VK_RETURN.0, false);
            assert_eq!(s.detail.borrow().clone(), Some(expected));
            // A single click selects a row but never opens it; double-click does.
            key_down(window, s, VK_ESCAPE.0, false);
            serve("2026-10-23", 90);
            scroll_by(window, s, 1e6);
            let row = s.scene.borrow().targets.rect(ROW_BASE + 89).unwrap();
            let scale = dpi(window) as f32 / 96.0;
            let l =
                LPARAM((((row.y + 3.0) * scale) as isize) << 16 | ((row.x + 3.0) * scale) as isize);
            SendMessageW(window, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(l));
            SendMessageW(window, WM_LBUTTONUP, None, Some(l));
            assert!(s.detail.borrow().is_none());
            assert_eq!(s.pointer.borrow().focus, Some(ROW_BASE + 89));
            SendMessageW(window, WM_LBUTTONDBLCLK, Some(WPARAM(1)), Some(l));
            SendMessageW(window, WM_LBUTTONUP, None, Some(l));
            assert!(s.detail.borrow().is_some());
            SendMessageW(window, WM_CLOSE, None, None);
            let _ = DestroyWindow(owner);
        }
    }
    fn answer_dialog(title: &'static str, button: usize) -> std::thread::JoinHandle<bool> {
        std::thread::spawn(move || unsafe {
            let t = crate::ui::native::wide(title);
            for _ in 0..100 {
                if let Ok(h) = FindWindowW(w!("#32770"), PCWSTR(t.as_ptr())) {
                    let _ = PostMessageW(Some(h), WM_COMMAND, WPARAM(button), LPARAM(0));
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            false
        })
    }
    #[test]
    fn native_ui_main_window_opens_unfocused_and_movement_controls_never_take_initial_focus() {
        let (sender, reader, _raw, rx) = channels(16, view());
        unsafe {
            let owner = owner();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            let s = state(window);
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            let movement = [START, MINUS, PLUS];
            assert_eq!(s.pointer.borrow().focus, None, "nothing is focused on open");
            assert!(!s.pointer.borrow().focus_visible);
            assert!(
                s.pointer
                    .borrow()
                    .key_activate(&s.scene.borrow().targets)
                    .is_none()
            );
            // Switching pages never moves focus onto a movement control.
            for page in Page::ALL {
                activate(window, s, nav_id(page), false);
                let focus = s.pointer.borrow().focus;
                assert!(focus.is_none_or(|f| !movement.contains(&f)), "{page:?}");
            }
            // Enter or Space with no ring does nothing.
            key_down(window, s, VK_RETURN.0, false);
            key_down(window, s, VK_SPACE.0, false);
            // The first Tab lands on Pause, never on a movement control.
            activate(window, s, nav_id(Page::Today), false);
            key_down(window, s, VK_TAB.0, false);
            assert_eq!(s.pointer.borrow().focus, Some(PAUSE));
            // A mouse press on Start leaves no ring, so Enter cannot repeat it.
            s.pointer.borrow_mut().focus_visible = false;
            let scene_start = s.scene.borrow().targets.rect(START).unwrap();
            let scale = dpi(window) as f32 / 96.0;
            let l = LPARAM(
                (((scene_start.y + 3.0) * scale) as isize) << 16
                    | ((scene_start.x + 3.0) * scale) as isize,
            );
            s.scene.borrow_mut().card.start = false;
            SendMessageW(window, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(l));
            SendMessageW(window, WM_LBUTTONUP, None, Some(l));
            assert!(
                s.pointer
                    .borrow()
                    .key_activate(&s.scene.borrow().targets)
                    .is_none()
            );
            key_down(window, s, VK_RETURN.0, false);
            key_down(window, s, VK_SPACE.0, false);
            SendMessageW(window, WM_CLOSE, None, None);
            let _ = DestroyWindow(owner);
        }
        // Start was disabled for the press (the belt moves), so nothing but statistics
        // requests reached the recorder.
        drop(rx);
    }
    #[test]
    fn native_ui_main_window_targets_form_validates_inline_confirms_and_survives_close_in_modal() {
        let (sender, reader, raw, rx) = channels(32, view());
        let spare = sender.clone();
        unsafe {
            let owner = owner();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            let s = state(window);
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            raw.lock().unwrap().totals.day = "2026-10-23".into();
            activate(window, s, nav_id(Page::Targets), false);
            assert_eq!(next_request(&rx), ("2026-10-23".into(), 1));
            raw.lock().unwrap().dashboard = Some(dash("2026-10-23", 1).into());
            SendMessageW(window, UPDATE, None, None);
            // Fields are filled once from today's saved settings and are visible.
            assert_eq!(forms::text(s, FIELD_BASE + 1), "1000");
            let edit = forms::edit(s, FIELD_BASE + 1).unwrap();
            assert!(IsWindowVisible(edit).as_bool());
            assert!(!IsWindowVisible(forms::edit(s, FIELD_BASE + 10).unwrap()).as_bool());
            // Invalid input shows each message next to its field and sends nothing.
            forms::put_text(s, FIELD_BASE, "19");
            forms::put_text(s, FIELD_BASE + 1, "1.5");
            forms::put_text(s, FIELD_BASE + 2, "x");
            activate(window, s, TARGETS_SAVE, false);
            let f = s.scene.borrow().data.form.clone();
            assert_eq!(
                f.errors[1].as_deref(),
                Some("Weight must be between 20 and 150 kg, or blank.")
            );
            assert_eq!(
                f.errors[2].as_deref(),
                Some("Step target must be a whole number.")
            );
            assert_eq!(
                f.errors[3].as_deref(),
                Some("Enter a number using a decimal point, or leave blank.")
            );
            assert!(f.errors[0].is_none() && f.errors[4].is_none());
            // Valid input asks first; cancelling sends nothing.
            for (i, v) in ["80", "5000", "30", "100"].iter().enumerate() {
                forms::put_text(s, FIELD_BASE + i as Id, v);
            }
            let cancel = answer_dialog("Save dated settings", 2);
            activate(window, s, TARGETS_SAVE, false);
            assert!(cancel.join().unwrap(), "confirmation was shown");
            assert!(
                s.scene
                    .borrow()
                    .data
                    .form
                    .errors
                    .iter()
                    .all(Option::is_none)
            );
            assert!(matches!(rx.recv().unwrap(), Input::ClearUiMessage));
            // Confirming sends the typed preferences.
            let ok = answer_dialog("Save dated settings", 1);
            activate(window, s, TARGETS_SAVE, false);
            assert!(ok.join().unwrap());
            assert!(matches!(rx.recv().unwrap(), Input::ClearUiMessage));
            match rx.recv().unwrap() {
                Input::SavePreferences(p) => {
                    assert_eq!(p.effective, "2026-10-23");
                    assert_eq!(
                        (p.weight, p.steps, p.minutes, p.calories),
                        (Some(80.0), Some(5000), Some(30.0), Some(100.0))
                    );
                }
                _ => panic!("expected the saved preferences"),
            }
            // Closing the window while the confirmation is open writes nothing.
            let raw_window = window.0 as isize;
            let closer = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(150));
                let _ = PostMessageW(
                    Some(HWND(raw_window as *mut _)),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            });
            activate(window, s, TARGETS_SAVE, false);
            closer.join().unwrap();
            assert!(!IsWindow(Some(window)).as_bool());
            assert!(matches!(rx.recv().unwrap(), Input::ClearUiMessage));
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(false)));
            assert!(matches!(rx.recv().unwrap(), Input::Dashboard(None)));
            spare.try_send(Input::Exit).unwrap();
            assert!(
                matches!(rx.recv().unwrap(), Input::Exit),
                "no preference write followed the close"
            );
            let _ = DestroyWindow(owner);
        }
    }
    #[test]
    fn native_ui_main_window_minimize_restore_and_close_follow_preferences() {
        let (sender, reader, raw, _rx) = channels(256, view());
        unsafe {
            let owner = owner();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            assert!(IsWindowVisible(window).as_bool());
            assert_ne!(
                GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 & WS_EX_APPWINDOW.0,
                0
            );
            // The default uses the normal Windows minimized state.
            SendMessageW(
                window,
                WM_SYSCOMMAND,
                Some(WPARAM(SC_MINIMIZE as usize)),
                None,
            );
            assert!(IsIconic(window).as_bool());
            let _ = ShowWindow(window, SW_RESTORE);
            raw.lock().unwrap().window_behavior.minimize_to_tray = true;
            SendMessageW(
                window,
                WM_SYSCOMMAND,
                Some(WPARAM(SC_MINIMIZE as usize)),
                None,
            );
            assert!(IsWindow(Some(window)).as_bool());
            assert!(!IsWindowVisible(window).as_bool());
            let _ = ShowWindow(window, SW_RESTORE);
            assert!(IsWindowVisible(window).as_bool());
            raw.lock().unwrap().window_behavior.keep_running_on_close = false;
            SendMessageW(window, WM_CLOSE, None, None);
            let mut message = MSG::default();
            assert!(
                PeekMessageW(&mut message, Some(owner), WM_CLOSE, WM_CLOSE, PM_REMOVE).as_bool()
            );
            // Exit is delegated to the owner; the child does not shut down the worker alone.
            assert!(IsWindow(Some(window)).as_bool());
            raw.lock().unwrap().window_behavior.keep_running_on_close = true;
            SendMessageW(window, WM_CLOSE, None, None);
            assert!(!IsWindow(Some(window)).as_bool());
            assert!(
                !PeekMessageW(&mut message, Some(owner), WM_CLOSE, WM_CLOSE, PM_REMOVE).as_bool()
            );
            let _ = DestroyWindow(owner);
        }
    }
    #[test]
    fn native_ui_main_window_settings_shortcuts_step_and_device_controls() {
        let mut v = view();
        v.shortcuts.bindings[0] = "Ctrl+Alt+P".into();
        v.candidates = vec![crate::application::Candidate {
            address: 7,
            name: "Pad".into(),
        }];
        let (sender, reader, raw, rx) = channels(32, v);
        unsafe {
            let owner = owner();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            let s = state(window);
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            activate(window, s, nav_id(Page::Settings), false);
            for (i, preference) in crate::appearance::ThemePreference::ALL
                .into_iter()
                .enumerate()
            {
                activate(window, s, THEME_BASE + i as Id, true);
                assert!(matches!(rx.recv().unwrap(), Input::Theme(p) if p == preference));
                raw.lock().unwrap().theme_preference = preference;
                SendMessageW(window, UPDATE, None, None);
                let expected = theme::detect(false, preference).mode;
                assert_eq!(s.theme.get().mode, expected);
                assert_eq!(s.scene.borrow().data.theme_preference, preference);
                // Windows broadcasts must respect the saved override too.
                SendMessageW(window, WM_SETTINGCHANGE, None, None);
                assert_eq!(s.theme.get().mode, expected);
                SendMessageW(window, WM_PAINT, None, None);
            }
            // Fields start with the saved bindings; Start/Resume has no field.
            assert_eq!(forms::text(s, FIELD_BASE + 10), "Ctrl+Alt+P");
            assert_eq!(s.scene.borrow().fields.len(), 4);
            // Invalid and duplicate entries show on the field or the form.
            forms::put_text(s, FIELD_BASE + 11, "P");
            activate(window, s, SHORTCUTS_SAVE, false);
            let f = s.scene.borrow().data.form.clone();
            assert_eq!(
                f.shortcut_errors[1].as_deref(),
                Some("Shortcuts require at least two of Ctrl, Alt and Shift")
            );
            forms::put_text(s, FIELD_BASE + 11, "ctrl+alt+p");
            activate(window, s, SHORTCUTS_SAVE, false);
            assert_eq!(
                s.scene.borrow().data.form.shortcut_general.as_deref(),
                Some("Each shortcut must have a different combination")
            );
            // A valid set is sent to the recorder.
            forms::put_text(s, FIELD_BASE + 11, "Ctrl+Alt+Up");
            activate(window, s, SHORTCUTS_SAVE, false);
            match rx.recv().unwrap() {
                Input::Shortcuts(c) => assert_eq!(c.bindings[1], "Ctrl+Alt+Up"),
                _ => panic!("expected shortcuts"),
            }
            assert!(s.scene.borrow().data.form.shortcuts_saved);
            // The step chips use the shared Increment command.
            activate(window, s, WINDOWS_STARTUP, false);
            // Disabled activation must leave no command before the next enabled one.
            raw.lock().unwrap().windows_startup_available = true;
            SendMessageW(window, UPDATE, None, None);
            activate(window, s, WINDOWS_STARTUP, false);
            assert!(matches!(rx.recv().unwrap(), Input::WindowsStartup(true)));
            activate(window, s, MINIMIZE_TO_TRAY, false);
            assert!(matches!(
                rx.recv().unwrap(),
                Input::WindowPreference(crate::window_behavior::Preference::MinimizeToTray(true))
            ));
            activate(window, s, START_MINIMIZED, false);
            assert!(matches!(
                rx.recv().unwrap(),
                Input::WindowPreference(crate::window_behavior::Preference::StartMinimized(false))
            ));
            activate(window, s, KEEP_RUNNING_ON_CLOSE, false);
            assert!(matches!(
                rx.recv().unwrap(),
                Input::WindowPreference(crate::window_behavior::Preference::KeepRunningOnClose(
                    false
                ))
            ));
            activate(window, s, RECOVER_GAPS, false);
            assert!(matches!(rx.recv().unwrap(), Input::RecoverGaps(true)));
            activate(window, s, STEP_UP, false);
            assert!(matches!(rx.recv().unwrap(), Input::Increment(2)));
            activate(window, s, STEP_DOWN, false);
            // Already at the lowest step in this fixture: nothing is sent.
            // Device buttons are tray commands: they reach the owner, never the worker.
            for id in [AUTO, SCAN, CANDIDATE_BASE] {
                activate(window, s, id, false);
            }
            // The Settings-only folder action must reach the shared owner handler.
            activate(window, s, FOLDER, false);
            let mut message = MSG::default();
            let mut folder_requests = 0;
            while PeekMessageW(&mut message, Some(owner), WM_COMMAND, WM_COMMAND, PM_REMOVE)
                .as_bool()
            {
                if message.wParam.0 == crate::ui::tray::OPEN_DATA_FOLDER {
                    folder_requests += 1;
                }
            }
            assert_eq!(folder_requests, 1);
            // Tab moves into a field and back out; Enter inside a field does nothing.
            let id = FIELD_BASE + 10;
            s.pointer.borrow_mut().focus = Some(id);
            forms::sync_focus(window, s);
            assert_eq!(GetFocus(), forms::edit(s, id).unwrap());
            SendMessageW(window, forms::FIELD_TAB, None, None);
            assert_ne!(s.pointer.borrow().focus, Some(id));
            assert_eq!(
                GetFocus(),
                window
                    .is_invalid()
                    .then(HWND::default)
                    .unwrap_or(GetFocus())
            );
            SendMessageW(window, WM_CLOSE, None, None);
            let _ = DestroyWindow(owner);
        }
    }
    #[test]
    fn native_ui_main_window_paints_every_screen_in_every_theme_and_scale() {
        let (sender, reader, raw, _rx) = channels(64, view());
        raw.lock().unwrap().totals.day = "2026-10-23".into();
        raw.lock().unwrap().dashboard = Some(dash("2026-10-23", 30).into());
        unsafe {
            let owner = owner();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            let s = state(window);
            SendMessageW(window, UPDATE, None, None);
            let hc = theme::HighContrastColors {
                window: theme::Color::rgb(0),
                text: theme::Color::rgb(0xffffff),
                hotlight: theme::Color::rgb(0xffff00),
                gray_text: theme::Color::rgb(0x3fff23),
                highlight: theme::Color::rgb(0x1aebff),
                highlight_text: theme::Color::rgb(0),
                face: theme::Color::rgb(0),
                face_text: theme::Color::rgb(0xffffff),
            };
            let themes = [
                Theme::new(theme::Mode::Light, None, false),
                Theme::new(theme::Mode::Dark, None, true),
                Theme::high_contrast(hc, false),
            ];
            let renderer = s.renderer.get().unwrap();
            for dpi in [96, 144, 216] {
                renderer.set_dpi(dpi);
                for th in themes {
                    s.theme.set(th);
                    forms::apply_theme_to_edits(s);
                    for (page, detail) in [
                        (Page::Today, false),
                        (Page::History, false),
                        (Page::History, true),
                        (Page::Targets, false),
                        (Page::Settings, false),
                    ] {
                        s.page.set(page);
                        *s.detail.borrow_mut() = detail.then(|| "2026-10-22".to_string());
                        s.refresh(window);
                        let scene = s.scene.borrow();
                        let pointer = *s.pointer.borrow();
                        let result = renderer.paint(th.background, |p| {
                            paint::draw(p, &th, &scene, &pointer, page, s.screen(), s.tip.get())
                        });
                        assert!(
                            result.is_ok(),
                            "{page:?} detail={detail} dpi={dpi} {:?}: {result:?}",
                            th.mode
                        );
                    }
                }
            }
            SendMessageW(window, WM_CLOSE, None, None);
            let _ = DestroyWindow(owner);
        }
    }
    #[test]
    fn native_ui_main_window_automation_invoke_submits_unknown_start_and_keeps_pause_available() {
        // Start state unknown: the pad is connected but its belt state is not.
        let (sender, reader, _raw, rx) = channels(
            16,
            View {
                fresh_speed: None,
                ..view()
            },
        );
        unsafe {
            let owner = owner();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            let s = state(window);
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            SendMessageW(window, WM_PAINT, None, None);
            let shared = s.access.get().unwrap();
            let items = shared.items();
            let start = items.iter().find(|i| i.name == "Start or resume").unwrap();
            let pause = items.iter().find(|i| i.name == "Pause").unwrap();
            assert!(start.enabled && pause.enabled);
            // Unknown state sends Start/Resume directly for the published generation.
            SendMessageW(
                window,
                access::ACCESS_INVOKE,
                Some(WPARAM(START as usize)),
                None,
            );
            assert!(matches!(
                rx.recv().unwrap(),
                Input::Control {
                    command: Command::StartResume,
                    generation: 5
                }
            ));
            s.sender.try_send(Input::Exit).unwrap();
            assert!(matches!(rx.recv().unwrap(), Input::Exit));
            // Pause needs no confirmation.
            SendMessageW(
                window,
                access::ACCESS_INVOKE,
                Some(WPARAM(PAUSE as usize)),
                None,
            );
            assert!(matches!(
                rx.recv().unwrap(),
                Input::Control {
                    command: Command::Pause,
                    generation: 5
                }
            ));
            // Focus requests move the toolkit focus without arming Enter.
            SendMessageW(
                window,
                access::ACCESS_FOCUS,
                Some(WPARAM(PAUSE as usize)),
                None,
            );
            assert_eq!(s.pointer.borrow().focus, Some(PAUSE));
            assert!(
                s.pointer
                    .borrow()
                    .key_activate(&s.scene.borrow().targets)
                    .is_none()
            );
            // Targets that do not exist or are disabled are ignored.
            SendMessageW(
                window,
                access::ACCESS_INVOKE,
                Some(WPARAM(MINUS as usize)),
                None,
            );
            SendMessageW(window, access::ACCESS_INVOKE, Some(WPARAM(9999)), None);
            // The root object request is answered without a client attached.
            let _ = SendMessageW(window, WM_GETOBJECT, Some(WPARAM(0)), Some(LPARAM(-25)));
            SendMessageW(window, WM_CLOSE, None, None);
            let _ = DestroyWindow(owner);
        }
    }
    #[test]
    fn native_ui_main_window_handle_reuses_and_closes_the_window() {
        let (sender, reader, _raw, rx) = channels(16, view());
        unsafe {
            let owner = owner();
            let slot = Rc::new(Cell::new(None));
            let window = create(owner, sender, reader, slot.clone()).unwrap();
            slot.set(Some(window));
            let handle = Handle { window: slot };
            assert_eq!(handle.window(), Some(window));
            handle.update();
            handle.close();
            assert_eq!(handle.window(), None);
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(false)));
            assert!(matches!(rx.recv().unwrap(), Input::Dashboard(None)));
            let _ = DestroyWindow(owner);
        }
    }
}
