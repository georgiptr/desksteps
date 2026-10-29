//! Development-only native panel. All mutations are typed worker requests.
use crate::{
    application::{CommandSender, SIM_ACTIONS, SimAction, SnapshotReader, UserCommand, Worker},
    ui::native::{FontSlot, set_text, wide},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};
const UPDATE: u32 = WM_APP + 45;
const FIRST: usize = 800;
#[derive(Default)]
pub(crate) struct Handle(Rc<Cell<Option<HWND>>>);
impl Handle {
    pub fn window(&self) -> Option<HWND> {
        self.0
            .get()
            .filter(|h| unsafe { IsWindow(Some(*h)) }.as_bool())
    }
    pub fn open(&self, owner: HWND, worker: &Worker) -> Result<(), String> {
        if !worker.view.snapshot().development {
            return Ok(());
        }
        if let Some(window) = self.window() {
            unsafe {
                let _ = ShowWindow(window, SW_RESTORE);
                let _ = SetForegroundWindow(window);
            }
            return Ok(());
        }
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).map_err(|e| e.to_string())?.0);
            let _ = RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance,
                lpszClassName: w!("DeskStepsSimulator"),
                hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
                hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
                ..Default::default()
            });
            let state = Rc::new(State {
                slot: self.0.clone(),
                sender: worker.sender.clone(),
                view: worker.view.clone(),
                children: RefCell::new(vec![]),
                status: RefCell::new(String::new()),
                font: FontSlot::new(),
                released: Cell::new(false),
            });
            let ptr = Rc::into_raw(state.clone());
            let dpi = GetDpiForWindow(owner).max(96);
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
            let mut bounds = RECT {
                right: 620 * dpi as i32 / 96,
                bottom: 570 * dpi as i32 / 96,
                ..Default::default()
            };
            let _ = AdjustWindowRectExForDpi(&mut bounds, style, false, WS_EX_CONTROLPARENT, dpi);
            let title = wide(&format!(
                "{} — Simulator",
                crate::identity::DEVELOPMENT_TITLE
            ));
            let window = CreateWindowExW(
                WS_EX_CONTROLPARENT,
                w!("DeskStepsSimulator"),
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
            )
            .map_err(|e| {
                if !state.released.get() {
                    drop(Rc::from_raw(ptr));
                }
                e.to_string()
            })?;
            self.0.set(Some(window));
            crate::ui::native::inherit_window_icons(window, owner);
            let _ = ShowWindow(window, SW_SHOW);
            let _ = SetForegroundWindow(window);
        }
        Ok(())
    }
    pub fn update(&self) {
        if let Some(h) = self.window() {
            unsafe {
                let _ = PostMessageW(Some(h), UPDATE, WPARAM(0), LPARAM(0));
            }
        }
    }
    pub fn close(&self) {
        if let Some(h) = self.0.take() {
            unsafe {
                let _ = DestroyWindow(h);
            }
        }
    }
}
struct State {
    slot: Rc<Cell<Option<HWND>>>,
    sender: CommandSender,
    view: SnapshotReader,
    children: RefCell<Vec<HWND>>,
    status: RefCell<String>,
    font: FontSlot,
    released: Cell<bool>,
}
unsafe fn layout(window: HWND, s: &State) {
    unsafe {
        let dpi = GetDpiForWindow(window).max(96) as i32;
        let scale = |n: i32| n * dpi / 96;
        let mut rect = RECT::default();
        let _ = GetClientRect(window, &mut rect);
        let width = (rect.right * 96 / dpi).max(400);
        let font = s.font.replace(CreateFontW(
            -scale(14),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        ));
        for (i, child) in s.children.borrow().iter().enumerate() {
            let (x, y, w, h) = if i == 0 {
                (14, 12, width - 28, 118)
            } else {
                let n = i - 1;
                let half = (width - 42) / 2;
                (
                    14 + (n % 2) as i32 * (half + 14),
                    142 + (n / 2) as i32 * 48,
                    half,
                    38,
                )
            };
            let _ = MoveWindow(*child, scale(x), scale(y), scale(w), scale(h), true);
            SendMessageW(
                *child,
                WM_SETFONT,
                Some(WPARAM(s.font.get().0 as usize)),
                Some(LPARAM(1)),
            );
        }
        drop(font);
    }
}
unsafe extern "system" fn proc(window: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            SetWindowLongPtrW(
                window,
                GWLP_USERDATA,
                (*(lp.0 as *const CREATESTRUCTW)).lpCreateParams as isize,
            );
            return DefWindowProcW(window, message, wp, lp);
        }
        let ptr = GetWindowLongPtrW(window, GWLP_USERDATA) as *const State;
        if ptr.is_null() {
            return DefWindowProcW(window, message, wp, lp);
        }
        Rc::increment_strong_count(ptr);
        let s = Rc::from_raw(ptr);
        match message {
            WM_CREATE => {
                let instance = HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0);
                for (i,label) in std::iter::once("Synthetic data only. Real Bluetooth is disabled.\r\nUse the normal app controls to operate the simulated belt.")
                    .chain(SIM_ACTIONS.iter().map(|(_,label)| *label)).enumerate() {
                    let label=wide(label);
                    match CreateWindowExW(WINDOW_EX_STYLE(0),if i==0 {w!("STATIC")} else {w!("BUTTON")},PCWSTR(label.as_ptr()),
                        WS_CHILD|WS_VISIBLE|if i==0 {WINDOW_STYLE(0)} else {WS_TABSTOP|WINDOW_STYLE(BS_PUSHBUTTON as u32)},
                        0,0,0,0,Some(window),Some(HMENU((FIRST+i) as *mut _)),Some(instance),None) {
                        Ok(child)=>s.children.borrow_mut().push(child), Err(_)=>return LRESULT(-1),
                    }
                }
                layout(window, &s);
            }
            UPDATE => {
                let view = s.view.snapshot();
                if *s.status.borrow() != view.simulator_status {
                    *s.status.borrow_mut() = view.simulator_status.clone();
                    if let Some(status) = s.children.borrow().first() {
                        set_text(
                            *status,
                            &format!(
                                "Synthetic data only. Real Bluetooth is disabled.\r\nUse the normal Start/Resume, Pause and speed controls.\r\n{}",
                                view.simulator_status.replace(" | ", " · ")
                            ),
                        );
                    }
                }
            }
            WM_SIZE => {
                if !s.children.borrow().is_empty() {
                    layout(window, &s);
                }
            }
            WM_DPICHANGED => {
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    window,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                layout(window, &s);
            }
            WM_COMMAND => {
                let id = (wp.0 & 0xffff).wrapping_sub(FIRST + 1);
                if let Some((action, _)) = SIM_ACTIONS.get(id) {
                    if *action == SimAction::ResetData
                        && !crate::ui::dialogs::confirm(
                            "Reset synthetic development history and settings to the sample dataset?\n\nThis affects only development data. Normal walking history is separate.",
                        )
                    {
                        return LRESULT(0);
                    }
                    if let Err(error) = s.sender.try_send(UserCommand::Simulator(*action)) {
                        crate::ui::dialogs::error_dialog(&error);
                    }
                }
            }
            WM_CLOSE => {
                let _ = DestroyWindow(window);
            }
            WM_NCDESTROY => {
                s.released.set(true);
                s.slot.set(None);
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                drop(Rc::from_raw(ptr));
                return DefWindowProcW(window, message, wp, lp);
            }
            _ => return DefWindowProcW(window, message, wp, lp),
        }
        LRESULT(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_ui_simulator_is_development_only_and_buttons_dispatch_worker_requests() {
        unsafe {
            let owner = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Synthetic simulator owner"),
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
            let icon = LoadIconW(None, IDI_WARNING).unwrap();
            crate::ui::native::set_window_icon(owner, icon);
            let handle = Handle::default();
            let (normal, _) = crate::application::testing::inert_worker(Default::default());
            handle.open(owner, &normal).unwrap();
            assert!(handle.window().is_none());
            let (worker, commands) =
                crate::application::testing::inert_worker(crate::application::View {
                    development: true,
                    no_bluetooth: true,
                    ..Default::default()
                });
            handle.open(owner, &worker).unwrap();
            let window = handle.window().unwrap();
            assert_eq!(
                crate::ui::native::read_text(window),
                format!("{} — Simulator", crate::identity::DEVELOPMENT_TITLE)
            );
            assert_eq!(
                SendMessageW(window, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None).0,
                icon.0 as isize
            );
            let mut client = RECT::default();
            GetClientRect(window, &mut client).unwrap();
            let mut previous = None;
            for (i, (action, _)) in SIM_ACTIONS.iter().enumerate() {
                let button = GetDlgItem(Some(window), (FIRST + 1 + i) as i32).unwrap();
                let mut r = RECT::default();
                GetWindowRect(button, &mut r).unwrap();
                let mut points = [
                    windows::Win32::Foundation::POINT {
                        x: r.left,
                        y: r.top,
                    },
                    windows::Win32::Foundation::POINT {
                        x: r.right,
                        y: r.bottom,
                    },
                ];
                MapWindowPoints(None, Some(window), &mut points);
                assert!(
                    points[0].x >= 0
                        && points[0].y >= 0
                        && points[1].x <= client.right
                        && points[1].y <= client.bottom
                );
                if i % 2 == 1 {
                    assert!(points[0].x >= previous.unwrap());
                }
                previous = Some(points[1].x);
                if *action != SimAction::ResetData {
                    SendMessageW(
                        window,
                        WM_COMMAND,
                        Some(WPARAM(FIRST + 1 + i)),
                        Some(LPARAM(button.0 as isize)),
                    );
                    assert!(
                        matches!(commands.recv().unwrap(),UserCommand::Simulator(a) if a==*action)
                    );
                }
            }
            handle.close();
            assert!(handle.window().is_none());
            handle.open(owner, &worker).unwrap();
            handle.close();
            DestroyWindow(owner).unwrap();
        }
    }
}
