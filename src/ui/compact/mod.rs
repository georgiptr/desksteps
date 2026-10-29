//! Compact remote: the live card on its own small window, drawn with the toolkit.
//! Same `LiveCard` rules as the main window; movement goes only through `actions::request`.
use crate::{
    application::{CommandSender, SnapshotReader, UserCommand as Input, View, Worker},
    control::Command,
    statistics::{Day, Request},
    ui::{
        kit::{
            input::{Id, Input as Pointer, Targets},
            layout::Rect,
            render::{Align, Painter, Renderer},
            theme::{self, TextStyle, Theme},
            widgets::{self, ButtonKind, State as WidgetState},
        },
        main_window::{
            apply_theme, dpi,
            frame::{MINUS, PAUSE, PLUS, START, tooltip_text},
            invalidate, point,
        },
        model::live::LiveCard,
    },
};
use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::AdjustWindowRectExForDpi, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

pub const UPDATE: u32 = WM_APP + 31;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIP_TIMER: usize = 1;
pub const WIDTH: f32 = 260.0;
pub const HEIGHT: f32 = 148.0;
pub const PIN: Id = 60;
pub const OPEN: Id = 61;
/// Tray command that opens the main window.
const TRAY_OPEN_MAIN: usize = 11;

/// Element rectangles for a client area of `w` x `h` logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub pill: Rect,
    pub pin: Rect,
    pub open: Rect,
    pub speed: Rect,
    pub minus: Rect,
    pub plus: Rect,
    pub start: Rect,
    pub pause: Rect,
    pub steps: Rect,
    pub bar: Rect,
}
impl Frame {
    pub fn new(w: f32, _h: f32) -> Self {
        let x = 10.0;
        let inner = (w - 20.0).max(0.0);
        let icon = 28.0;
        let open = Rect::new(x + inner - icon, 8.0, icon, 24.0);
        let pin = Rect::new(open.x - 4.0 - icon, 8.0, icon, 24.0);
        let gap = 6.0;
        let bw = ((inner - 3.0 * gap) / 4.0).max(0.0);
        let button = |i: f32| Rect::new(x + i * (bw + gap), 76.0, bw, 32.0);
        Self {
            pill: Rect::new(x, 9.0, (pin.x - x - 6.0).max(0.0), 22.0),
            pin,
            open,
            speed: Rect::new(x, 36.0, inner, 34.0),
            minus: button(0.0),
            plus: button(1.0),
            start: button(2.0),
            pause: button(3.0),
            steps: Rect::new(x, 114.0, inner, 16.0),
            bar: Rect::new(x, 132.0, inner, 6.0),
        }
    }
    /// Tab order: Pause, then the window buttons, then the movement controls, so the
    /// first Tab press never lands on Start or the speed buttons.
    pub fn targets(&self, card: &LiveCard) -> Targets {
        let mut t = Targets::default();
        t.push(PAUSE, self.pause, card.pause);
        t.push(OPEN, self.open, true);
        t.push(PIN, self.pin, true);
        t.push(MINUS, self.minus, card.minus);
        t.push(PLUS, self.plus, card.plus);
        t.push(START, self.start, card.start);
        t
    }
}
fn tip_text(id: Id, card: &LiveCard, pinned: bool) -> Option<String> {
    match id {
        PIN => Some(
            if pinned {
                "Stop keeping this window on top"
            } else {
                "Keep this window on top"
            }
            .into(),
        ),
        OPEN => Some("Open the main window".into()),
        _ => tooltip_text(id, card, false),
    }
}

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
    pub fn toggle(&self, owner: HWND, worker: &Worker) -> Result<(), String> {
        if let Some(window) = self.window() {
            unsafe {
                if !IsWindowVisible(window).as_bool() || IsIconic(window).as_bool() {
                    let _ = ShowWindow(window, SW_RESTORE);
                    let _ = SetForegroundWindow(window);
                } else {
                    let _ = DestroyWindow(window);
                }
            }
            return Ok(());
        }
        let window = unsafe {
            create(
                owner,
                worker.sender.clone(),
                worker.view.clone(),
                self.window.clone(),
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
    card: RefCell<LiveCard>,
    targets: RefCell<Targets>,
    today: RefCell<Option<Day>>,
    pin: Cell<bool>,
    tip: Cell<Option<Id>>,
    tracking: Cell<bool>,
    closing: Cell<bool>,
    failures: Cell<u32>,
    next_request: Cell<Instant>,
}
impl State {
    fn refresh_theme(&self, window: HWND, view: &View) {
        let th = theme::detect(view.development, view.theme_preference);
        self.theme.set(th);
        unsafe { apply_theme(window, &th) };
    }
    fn rescene(&self, view: &View) -> bool {
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
        let card = LiveCard::from_view(view, self.today.borrow().as_ref());
        let changed = *self.card.borrow() != card || self.targets.borrow().rect(PAUSE).is_none();
        if changed {
            let targets = Frame::new(WIDTH, HEIGHT).targets(&card);
            self.pointer.borrow_mut().retain_valid(&targets);
            *self.card.borrow_mut() = card;
            *self.targets.borrow_mut() = targets;
        }
        // Today's target comes from the statistics snapshot; ask only when nobody has.
        if view.dashboard.is_none()
            && !view.totals.day.is_empty()
            && !self.closing.get()
            && Instant::now() >= self.next_request.get()
        {
            self.next_request
                .set(Instant::now() + Duration::from_secs(1));
            let _ = self.sender.try_send(Input::DashboardVisible(true));
            let _ = self.sender.try_send(Input::Dashboard(Some(Request {
                day: view.totals.day.clone(),
                range: 1,
            })));
        }
        changed
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
unsafe fn create(
    owner: HWND,
    sender: CommandSender,
    view: SnapshotReader,
    slot: Rc<Cell<Option<HWND>>>,
) -> windows::core::Result<HWND> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: w!("DeskStepsCompact"),
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
            sender,
            theme: Cell::new(theme::detect(development, theme_preference)),
            theme_preference: Cell::new(theme_preference),
            card: RefCell::new(LiveCard::from_view(&view.snapshot(), None)),
            view,
            renderer: OnceCell::new(),
            pointer: RefCell::new(Pointer::default()),
            targets: RefCell::new(Targets::default()),
            today: RefCell::new(None),
            pin: Cell::new(false),
            tip: Cell::new(None),
            tracking: Cell::new(false),
            closing: Cell::new(false),
            failures: Cell::new(0),
            next_request: Cell::new(Instant::now()),
        });
        let ptr = Rc::into_raw(state.clone());
        let scale = dpi(owner);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        // Size the client area so the caption and borders do not eat the controls' space.
        let mut bounds = RECT {
            right: crate::ui::kit::layout::to_physical(WIDTH, scale),
            bottom: crate::ui::kit::layout::to_physical(HEIGHT, scale),
            ..Default::default()
        };
        let _ = AdjustWindowRectExForDpi(&mut bounds, style, false, WS_EX_APPWINDOW, scale);
        let title = crate::ui::native::wide(crate::identity::title(development));
        let result = CreateWindowExW(
            WS_EX_APPWINDOW,
            w!("DeskStepsCompact"),
            PCWSTR(title.as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
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
                return Err(error);
            }
        };
        state.slot.set(Some(window));
        crate::ui::native::inherit_window_icons(window, owner);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        Ok(window)
    }
}
unsafe fn toggle_pin(window: HWND, s: &State) {
    let pin = !s.pin.get();
    s.pin.set(pin);
    unsafe {
        let _ = SetWindowPos(
            window,
            Some(if pin { HWND_TOPMOST } else { HWND_NOTOPMOST }),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    invalidate(window);
}
unsafe fn activate(window: HWND, s: &State, id: Id) {
    let command = match id {
        START => Command::StartResume,
        PAUSE => Command::Pause,
        MINUS => Command::Decrease,
        PLUS => Command::Increase,
        PIN => return unsafe { toggle_pin(window, s) },
        OPEN => {
            unsafe {
                let _ = PostMessageW(Some(s.owner), WM_COMMAND, WPARAM(TRAY_OPEN_MAIN), LPARAM(0));
            }
            return;
        }
        _ => return,
    };
    crate::ui::actions::request(&s.sender, &s.view, command);
    invalidate(window);
}
fn draw(p: &Painter, th: &Theme, s: &State) {
    let f = Frame::new(WIDTH, HEIGHT);
    let c = s.card.borrow();
    let targets = s.targets.borrow();
    let pointer = *s.pointer.borrow();
    let st = |id| WidgetState::of(&pointer, &targets, id);
    if th.development {
        p.fill_rect(Rect::new(0.0, 0.0, WIDTH, 3.0), th.development_red);
    }
    let pill_w = widgets::pill_width(p, c.connection).min(f.pill.w);
    widgets::pill(
        p,
        th,
        Rect::new(f.pill.x, f.pill.y, pill_w, f.pill.h),
        c.connection,
        c.tone,
    );
    widgets::icon_button(
        p,
        th,
        f.pin,
        '\u{E718}',
        if s.pin.get() {
            ButtonKind::Accent
        } else {
            ButtonKind::Standard
        },
        st(PIN),
    );
    widgets::icon_button(p, th, f.open, '\u{E8A7}', ButtonKind::Standard, st(OPEN));
    let number_w = p.text_width(&c.speed, TextStyle::Display).ceil() + 2.0;
    p.text(
        Rect::new(f.speed.x, f.speed.y, number_w, f.speed.h),
        &c.speed,
        TextStyle::Display,
        th.text,
        Align::Start,
    );
    let rest = Rect::new(
        f.speed.x + number_w + 6.0,
        f.speed.y + 6.0,
        (f.speed.w - number_w - 6.0).max(0.0),
        f.speed.h - 8.0,
    );
    let (caption, pending) = (c.speed_caption, c.pending.as_deref());
    p.text(
        rest,
        caption,
        TextStyle::Caption,
        th.text_secondary,
        Align::Start,
    );
    if let Some(pending) = pending {
        p.text(rest, pending, TextStyle::Caption, th.accent, Align::End);
    }
    widgets::button(p, th, f.minus, &c.decrease, ButtonKind::Standard, st(MINUS));
    widgets::button(p, th, f.plus, &c.increase, ButtonKind::Standard, st(PLUS));
    widgets::button(p, th, f.start, "Start", ButtonKind::Accent, st(START));
    widgets::button(p, th, f.pause, "Pause", ButtonKind::Standard, st(PAUSE));
    p.text(
        f.steps,
        &c.steps,
        TextStyle::Caption,
        th.text_secondary,
        Align::Start,
    );
    if let Some(fraction) = c.target_fraction {
        widgets::progress_bar(p, th, f.bar, fraction, th.accent);
    }
    if let Some(id) = s.tip.get()
        && let Some(rect) = targets.rect(id)
        && let Some(text) = tip_text(id, &c, s.pin.get())
    {
        widgets::tooltip(p, th, rect, &text, Rect::new(0.0, 0.0, WIDTH, HEIGHT));
    }
}
unsafe fn paint_window(window: HWND, s: &State) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        BeginPaint(window, &mut ps);
        if let Some(renderer) = s.renderer.get() {
            let th = s.theme.get();
            let result = renderer.paint(th.background, |p| draw(p, &th, s));
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
unsafe fn key_down(window: HWND, s: &State, key: u16, repeat: bool) {
    s.hide_tip(window);
    match VIRTUAL_KEY(key) {
        VK_TAB => {
            let back = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
            s.pointer.borrow_mut().tab(&s.targets.borrow(), back);
            invalidate(window);
        }
        VK_RETURN | VK_SPACE if !repeat => {
            let id = s.pointer.borrow().key_activate(&s.targets.borrow());
            if let Some(id) = id {
                unsafe { activate(window, s, id) };
            }
        }
        VK_ESCAPE => {
            s.pointer.borrow_mut().focus_visible = false;
            invalidate(window);
        }
        _ => {}
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
                    apply_theme(window, &s.theme.get());
                    s.rescene(&s.view.snapshot());
                }
                Err(error) => {
                    crate::ui::dialogs::error_dialog(&format!("Graphics unavailable: {error}"));
                    return LRESULT(-1);
                }
            },
            WM_ERASEBKGND => return LRESULT(1),
            WM_PAINT => paint_window(window, s),
            WM_SIZE => {
                if wparam.0 != SIZE_MINIMIZED as usize
                    && let Some(renderer) = s.renderer.get()
                {
                    renderer.resize(
                        (lparam.0 & 0xffff) as u32,
                        ((lparam.0 >> 16) & 0xffff) as u32,
                    );
                    invalidate(window);
                }
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
                invalidate(window);
            }
            UPDATE => {
                let view = s.view.snapshot();
                let theme_changed =
                    s.theme_preference.replace(view.theme_preference) != view.theme_preference;
                if theme_changed {
                    s.refresh_theme(window, &view);
                }
                if s.rescene(&view) || theme_changed {
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
                let changed = s.pointer.borrow_mut().mouse_move(&s.targets.borrow(), x, y);
                if changed {
                    s.hide_tip(window);
                    if s.pointer.borrow().hover.is_some() {
                        SetTimer(Some(window), TIP_TIMER, 500, None);
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
                if let Some(id) = hover
                    && tip_text(id, &s.card.borrow(), s.pin.get()).is_some()
                {
                    s.tip.set(Some(id));
                    invalidate(window);
                }
            }
            WM_LBUTTONDOWN => {
                s.hide_tip(window);
                let (x, y) = point(window, lparam);
                s.pointer.borrow_mut().mouse_down(&s.targets.borrow(), x, y);
                if s.pointer.borrow().pressed.is_some() {
                    SetCapture(window);
                }
                invalidate(window);
            }
            WM_LBUTTONUP => {
                let (x, y) = point(window, lparam);
                let id = s.pointer.borrow_mut().mouse_up(&s.targets.borrow(), x, y);
                // After the press is resolved: this sends WM_CAPTURECHANGED.
                let _ = ReleaseCapture();
                invalidate(window);
                if let Some(id) = id {
                    activate(window, s, id);
                }
            }
            WM_CAPTURECHANGED => {
                if s.pointer.borrow_mut().pressed.take().is_some() {
                    invalidate(window);
                }
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
                s.tip.set(None);
                let _ = KillTimer(Some(window), TIP_TIMER);
                let _ = ShowWindow(window, SW_HIDE);
            }
            WM_CLOSE => {
                let _ = DestroyWindow(window);
            }
            WM_DESTROY => {
                s.closing.set(true);
                s.slot.set(None);
                let _ = KillTimer(Some(window), TIP_TIMER);
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                s.closing.set(true);
                s.slot.set(None);
                drop(Rc::from_raw(ptr));
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
    use crate::application::testing::channels;
    fn inside(outer: Rect, r: Rect) -> bool {
        r.x >= outer.x - 0.01
            && r.y >= outer.y - 0.01
            && r.right() <= outer.right() + 0.01
            && r.bottom() <= outer.bottom() + 0.01
    }
    fn overlap(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }
    #[test]
    fn every_element_fits_and_nothing_overlaps_at_the_default_and_wider_sizes() {
        for w in [WIDTH, 300.0, 420.0] {
            let f = Frame::new(w, HEIGHT);
            let window = Rect::new(0.0, 0.0, w, HEIGHT);
            let parts = [
                f.pill, f.pin, f.open, f.speed, f.minus, f.plus, f.start, f.pause, f.steps, f.bar,
            ];
            for p in parts {
                assert!(inside(window, p) && p.w > 0.0 && p.h > 0.0, "{w}: {p:?}");
            }
            let buttons = [f.minus, f.plus, f.start, f.pause, f.pin, f.open, f.pill];
            for (i, a) in buttons.iter().enumerate() {
                for b in &buttons[i + 1..] {
                    assert!(!overlap(*a, *b), "{w}: {a:?} {b:?}");
                }
            }
            assert!(f.minus.w >= 40.0, "buttons stay usable");
        }
    }
    #[test]
    fn tab_order_puts_pause_first_and_movement_controls_last() {
        let card = LiveCard::from_view(
            &View {
                connected: true,
                controls_enabled: true,
                speed_available: true,
                adjustable_speed: Some(3.0),
                fresh_speed: Some(0.0),
                ..Default::default()
            },
            None,
        );
        let t = Frame::new(WIDTH, HEIGHT).targets(&card);
        let ids: Vec<_> = t.0.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![PAUSE, OPEN, PIN, MINUS, PLUS, START]);
        let mut input = Pointer::default();
        assert_eq!(input.focus, None);
        assert_eq!(input.tab(&t, false), Some(PAUSE));
    }
    #[test]
    fn native_ui_compact_minimize_restores_and_close_only_dismisses_remote() {
        let initial = View {
            window_behavior: crate::window_behavior::WindowBehavior {
                start_minimized: true,
                minimize_to_tray: true,
                keep_running_on_close: false,
            },
            ..Default::default()
        };
        let (worker, _commands) = crate::application::testing::inert_worker(initial);
        unsafe {
            let owner = CreateWindowExW(
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
            .unwrap();
            let handle = Handle::default();
            handle.toggle(owner, &worker).unwrap();
            let window = handle.window().unwrap();
            SendMessageW(
                window,
                WM_SYSCOMMAND,
                Some(WPARAM(SC_MINIMIZE as usize)),
                None,
            );
            assert!(!IsWindowVisible(window).as_bool());
            handle.toggle(owner, &worker).unwrap();
            assert_eq!(handle.window(), Some(window));
            assert!(IsWindowVisible(window).as_bool());
            SendMessageW(window, WM_CLOSE, None, None);
            assert!(handle.window().is_none());
            let mut message = MSG::default();
            assert!(
                !PeekMessageW(&mut message, Some(owner), WM_CLOSE, WM_CLOSE, PM_REMOVE).as_bool()
            );
            let _ = DestroyWindow(owner);
        }
        worker.join.join().unwrap();
    }
    #[test]
    fn tooltips_describe_the_window_buttons() {
        let card = LiveCard::from_view(&View::default(), None);
        assert!(tip_text(PIN, &card, false).unwrap().starts_with("Keep"));
        assert!(tip_text(PIN, &card, true).unwrap().starts_with("Stop"));
        assert!(
            tip_text(OPEN, &card, false)
                .unwrap()
                .contains("main window")
        );
        assert!(
            tip_text(START, &card, false)
                .unwrap()
                .contains("selected by the pad")
        );
    }
    #[test]
    fn native_ui_compact_remote_pins_sends_commands_and_never_starts_focused() {
        let (sender, reader, raw, rx) = channels(
            16,
            View {
                connected: true,
                controls_enabled: true,
                speed_available: true,
                increment: 1,
                fresh_speed: Some(3.0),
                adjustable_speed: Some(3.0),
                generation: 9,
                ..Default::default()
            },
        );
        unsafe {
            let owner = CreateWindowExW(
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
            .unwrap();
            let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
            assert_eq!(crate::ui::native::read_text(window), "DeskSteps");
            assert_ne!(
                GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 & WS_EX_APPWINDOW.0,
                0
            );
            SendMessageW(
                window,
                WM_SYSCOMMAND,
                Some(WPARAM(SC_MINIMIZE as usize)),
                None,
            );
            assert!(IsIconic(window).as_bool());
            let _ = ShowWindow(window, SW_RESTORE);
            let s = &*(GetWindowLongPtrW(window, GWLP_USERDATA) as *const State);
            // Nothing is focused, and the first Tab goes to Pause, not a movement control.
            assert_eq!(s.pointer.borrow().focus, None);
            key_down(window, s, VK_TAB.0, false);
            assert_eq!(s.pointer.borrow().focus, Some(PAUSE));
            key_down(window, s, VK_ESCAPE.0, false);
            // A mouse press on Pause sends one command and never arms Enter afterwards.
            let scale = dpi(window) as f32 / 96.0;
            let pause = s.targets.borrow().rect(PAUSE).unwrap();
            let lp = LPARAM(
                (((pause.y + 3.0) * scale) as isize) << 16 | ((pause.x + 3.0) * scale) as isize,
            );
            SendMessageW(window, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(lp));
            SendMessageW(window, WM_LBUTTONUP, None, Some(lp));
            assert!(matches!(
                rx.recv().unwrap(),
                Input::Control {
                    command: Command::Pause,
                    generation: 9
                }
            ));
            key_down(window, s, VK_RETURN.0, false);
            assert!(
                s.pointer
                    .borrow()
                    .key_activate(&s.targets.borrow())
                    .is_none()
            );
            // Pin toggles the topmost style.
            let topmost = |w| GetWindowLongPtrW(w, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
            assert!(!topmost(window));
            activate(window, s, PIN);
            assert!(topmost(window));
            activate(window, s, PIN);
            assert!(!topmost(window));
            // Open asks the tray window for the main window.
            activate(window, s, OPEN);
            // Statistics are requested once when none are published.
            raw.lock().unwrap().totals.day = "2026-10-23".into();
            SendMessageW(window, UPDATE, None, None);
            assert!(matches!(rx.recv().unwrap(), Input::DashboardVisible(true)));
            assert!(matches!(rx.recv().unwrap(), Input::Dashboard(Some(_))));
            SendMessageW(window, WM_PAINT, None, None);
            for preference in crate::appearance::ThemePreference::ALL {
                raw.lock().unwrap().theme_preference = preference;
                SendMessageW(window, UPDATE, None, None);
                assert_eq!(s.theme.get().mode, theme::detect(false, preference).mode);
                SendMessageW(window, WM_THEMECHANGED, None, None);
                assert_eq!(s.theme.get().mode, theme::detect(false, preference).mode);
                SendMessageW(window, WM_PAINT, None, None);
            }
            SendMessageW(window, WM_CLOSE, None, None);
            assert!(!IsWindow(Some(window)).as_bool());
            let _ = DestroyWindow(owner);
        }
    }
}
