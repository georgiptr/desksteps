use crate::ui::native::wide;
use crate::{
    Options,
    application::{self, UserCommand as Input, View, Worker},
    control,
};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT,
            POINT, WPARAM,
        },
        System::{LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
        UI::{
            HiDpi::{
                AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                GetDpiForWindow, GetWindowDpiAwarenessContext,
            },
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NIM_SETVERSION, NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            },
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

mod data_folder;
mod hotkeys;
mod icon;
mod menu;
mod resources;
use super::dialogs::error_dialog;
use hotkeys::*;
use icon::*;
use menu::*;
use resources::{IconOwner, MutexOwner, WindowOwner};

const CALLBACK: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;
const CONNECT: usize = 1;
const DISCONNECT: usize = 2;
const FIND: usize = 3;
/// Settings-only action; deliberately absent from the tray menu.
pub(crate) const OPEN_DATA_FOLDER: usize = 5;
const EXIT: usize = 6;
const START_RESUME: usize = 7;
const PAUSE: usize = 8;
const SPEED_UP: usize = 9;
const SPEED_DOWN: usize = 10;
const DASHBOARD: usize = 11;
const COMPACT: usize = 12;
const SIMULATOR: usize = 13;
const CANDIDATE_BASE: usize = 100;

struct Context {
    worker: Option<Worker>,
    development: bool,
    simulator: crate::ui::simulator::Handle,
    database: PathBuf,
    icon: HICON,
    tip: RefCell<String>,
    taskbar_created: u32,
    selected_menu: RefCell<Vec<u64>>,
    device_chooser: Cell<Option<bool>>,
    closing: Cell<bool>,
    main: crate::ui::main_window::Handle,
    compact: crate::ui::compact::Handle,
    shortcuts: RefCell<Option<crate::shortcuts::Shortcuts>>,
    locked: Cell<bool>,
}
impl Drop for Context {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel_setup();
            let _ = worker.sender.send(Input::Exit);
            let _ = worker.join.join();
        }
    }
}
fn send(context: &Context, input: Input) {
    if let Some(worker) = &context.worker {
        let _ = worker.sender.send(input);
    }
}
fn send_control(context: &Context, command: control::Command) {
    if let Some(worker) = &context.worker {
        super::actions::request(&worker.sender, &worker.view, command);
    }
}
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *mut Context;
    if pointer.is_null() {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    }
    // Win32 menus/dialogs and DestroyWindow can synchronously re-enter this
    // callback. Use shared access and short interior-mutability borrows; an
    // outstanding &mut Context across those APIs would be unsound.
    let context = unsafe { &*pointer };
    if message == context.taskbar_created {
        let _ = unsafe { add_icon(window, context) };
        return LRESULT(0);
    }
    match message {
        application::UPDATE_MESSAGE => unsafe {
            sync_shortcuts(window, context);
            update_icon(window, context);
            context.main.update();
            context.compact.update();
            context.simulator.update();
        },
        WM_HOTKEY => {
            if !context.locked.get() {
                match wparam.0 {
                    500 => send_control(context, control::Command::Pause),
                    501 => send_control(context, control::Command::Increase),
                    502 => send_control(context, control::Command::Decrease),
                    503 => {
                        if let Some(worker) = &context.worker {
                            let _ = context.compact.toggle(window, worker);
                        }
                    }
                    _ => {}
                }
            }
        }
        WM_WTSSESSION_CHANGE => {
            if [7, 8].contains(&wparam.0) {
                context.locked.set(wparam.0 == 7);
                context.shortcuts.borrow_mut().take();
                sync_shortcuts(window, context);
            }
        }
        CALLBACK => {
            let notification = (lparam.0 as u32) & 0xffff;
            if notification == WM_CONTEXTMENU || notification == WM_RBUTTONUP {
                if let Err(error) = unsafe { menu(window, context) } {
                    error_dialog(&error.to_string());
                }
            } else if notification == NIN_SELECT || notification == WM_LBUTTONDBLCLK {
                let _ =
                    unsafe { PostMessageW(Some(window), WM_COMMAND, WPARAM(DASHBOARD), LPARAM(0)) };
            }
        }
        WM_COMMAND => match wparam.0 & 0xffff {
            OPEN_DATA_FOLDER => {
                if let Err(error) = data_folder::open(window, &context.database) {
                    error_dialog(&error);
                }
            }
            SIMULATOR => {
                if let Some(worker) = &context.worker
                    && let Err(error) = context.simulator.open(window, worker)
                {
                    error_dialog(&error);
                }
            }
            COMPACT => {
                if let Some(worker) = &context.worker
                    && let Err(error) = context.compact.toggle(window, worker)
                {
                    error_dialog(&error);
                }
            }
            DASHBOARD => {
                if let Some(worker) = &context.worker
                    && let Err(error) = context.main.open(window, worker)
                {
                    error_dialog(&error);
                }
            }
            START_RESUME => send_control(context, control::Command::StartResume),
            PAUSE => send_control(context, control::Command::Pause),
            SPEED_UP => send_control(context, control::Command::Increase),
            SPEED_DOWN => send_control(context, control::Command::Decrease),
            CONNECT => {
                if let Some(worker) = &context.worker {
                    let enabled = !worker.view.snapshot().automatic_connection;
                    if !enabled {
                        worker.cancel_setup();
                    }
                    send(context, Input::AutomaticConnection(enabled));
                }
            }
            DISCONNECT => {
                if let Some(worker) = &context.worker {
                    worker.cancel_setup();
                }
                send(context, Input::Disconnect);
            }
            FIND => {
                context.device_chooser.set(Some(true));
                send(context, Input::ActiveScan);
            }
            EXIT => {
                let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
            id if id >= CANDIDATE_BASE => {
                let selected = context
                    .selected_menu
                    .borrow()
                    .get(id - CANDIDATE_BASE)
                    .copied();
                if let Some(address) = selected {
                    context.device_chooser.set(Some(false));
                    // Choosing the checked entry only dismisses the chooser;
                    // do not disconnect and reconnect the current pad.
                    if context
                        .worker
                        .as_ref()
                        .is_some_and(|worker| worker.view.snapshot().selected != Some(address))
                    {
                        send(context, Input::Select(address));
                    }
                }
            }
            _ => {}
        },
        crate::ui::main_window::SELECT_DEVICE => {
            // Same rule as choosing an entry in the tray's device menu: choosing the
            // checked device only dismisses the chooser.
            let address = wparam.0 as u64;
            context.device_chooser.set(Some(false));
            if context
                .worker
                .as_ref()
                .is_some_and(|worker| worker.view.snapshot().selected != Some(address))
            {
                send(context, Input::Select(address));
            }
        }
        WM_TIMER => {
            let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        WM_CLOSE => {
            if !context.closing.replace(true) {
                context.main.close();
                context.compact.close();
                context.simulator.close();
                unsafe {
                    for id in 500..504 {
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey(
                            Some(window),
                            id,
                        );
                    }
                    let _ = windows::Win32::System::RemoteDesktop::WTSUnRegisterSessionNotification(
                        window,
                    );
                }
                if let Some(worker) = &context.worker {
                    worker.cancel_setup();
                }
                send(context, Input::Exit);
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(window, context, ""));
                    let _ = DestroyWindow(window);
                }
            }
        }
        WM_DESTROY => unsafe {
            PostQuitMessage(0);
        },
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}
pub fn run(
    options: Options,
    first_run: bool,
    behavior: crate::window_behavior::WindowBehavior,
) -> Result<(), String> {
    unsafe {
        let mutex = CreateMutexW(
            None,
            false,
            if options.development {
                w!("Local\\DeskSteps.Tray.Development")
            } else if options.no_bluetooth {
                w!("Local\\DeskSteps.Tray.Preview")
            } else {
                w!("Local\\DeskSteps.Tray")
            },
        )
        .map_err(|e| e.to_string())?;
        let _mutex_owner = MutexOwner(mutex);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Err("DeskSteps is already running. Use its tray icon.".into());
        }
        let instance = HINSTANCE(GetModuleHandleW(None).map_err(|e| e.to_string())?.0);
        let icon = icon(options.development).map_err(|e| e.to_string())?;
        let _icon_owner = IconOwner(icon);
        let mut context = Box::new(Context {
            worker: None,
            development: options.development,
            simulator: crate::ui::simulator::Handle::default(),
            database: options.database.clone(),
            icon,
            tip: RefCell::new(String::new()),
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            selected_menu: RefCell::new(vec![]),
            device_chooser: Cell::new(None),
            closing: Cell::new(false),
            main: crate::ui::main_window::Handle::default(),
            compact: crate::ui::compact::Handle::default(),
            shortcuts: RefCell::new(None),
            locked: Cell::new(false),
        });
        let mut window_owner = WindowOwner(None);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: w!("DeskStepsTrayWindow"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread().to_string());
        }
        // A hidden top-level window receives Shell callbacks and Explorer restart
        // broadcasts. It is not a message-only window and has no taskbar button.
        let title = wide(crate::identity::title(options.development));
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("DeskStepsTrayWindow"),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            Some((&mut *context as *mut Context).cast()),
        )
        .map_err(|e| e.to_string())?;
        window_owner.0 = Some(window);
        crate::ui::native::set_window_icon(window, icon);
        let per_monitor_v2 = AreDpiAwarenessContextsEqual(
            GetWindowDpiAwarenessContext(window),
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
        .as_bool();
        let window_dpi = GetDpiForWindow(window);
        if options.smoke_seconds.is_some() && !per_monitor_v2 {
            let _ = DestroyWindow(window);
            return Err("Executable did not create a PerMonitorV2 DPI-aware window".into());
        }
        add_icon(window, &context)?;
        windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification(window, 0)
            .map_err(|e| e.to_string())?;
        let smoke = options.smoke_seconds;
        let notification_window = window.0 as isize;
        context.worker = Some(application::start(options, move || {
            let _ = PostMessageW(
                Some(HWND(notification_window as *mut _)),
                application::UPDATE_MESSAGE,
                WPARAM(0),
                LPARAM(0),
            );
        }));
        match behavior.startup(first_run, context.development) {
            crate::window_behavior::Startup::Open => context
                .main
                .open(window, context.worker.as_ref().unwrap())?,
            crate::window_behavior::Startup::Minimized => context
                .main
                .open_minimized(window, context.worker.as_ref().unwrap())?,
            crate::window_behavior::Startup::Tray => {}
        }
        if context.development {
            context
                .simulator
                .open(window, context.worker.as_ref().unwrap())?;
        }
        // Capture the actual initial visibility before smoke opens both windows.
        let opened_on_launch = context
            .main
            .window()
            .is_some_and(|w| IsWindowVisible(w).as_bool() && !IsIconic(w).as_bool());
        let minimized_on_launch = context.main.window().is_some_and(|w| IsIconic(w).as_bool());
        let taskbar_window_on_launch = context.main.window().is_some_and(|w| {
            IsWindowVisible(w).as_bool()
                && GetWindowLongPtrW(w, GWL_EXSTYLE) as u32 & WS_EX_APPWINDOW.0 != 0
        });
        if let Some(seconds) = smoke {
            context
                .main
                .open(window, context.worker.as_ref().unwrap())?;
            context
                .compact
                .toggle(window, context.worker.as_ref().unwrap())?;
            for (name, child) in [
                ("tray owner", window),
                ("main window", context.main.window().unwrap()),
                ("compact", context.compact.window().unwrap()),
            ] {
                if name != "tray owner"
                    && GetWindowLongPtrW(child, GWL_EXSTYLE) as u32 & WS_EX_APPWINDOW.0 == 0
                {
                    return Err(format!("The {name} window has no taskbar presence"));
                }
                if crate::ui::native::read_text(child)
                    != crate::identity::title(context.development)
                {
                    return Err(format!("The {name} window title is not DeskSteps"));
                }
                for size in [ICON_SMALL, ICON_BIG] {
                    let actual = SendMessageW(child, WM_GETICON, Some(WPARAM(size as usize)), None);
                    if actual.0 != icon.0 as isize {
                        return Err(format!(
                            "The {name} window is missing the app icon ({size})"
                        ));
                    }
                }
            }
            // Exercise native menu construction as well as Shell icon registration.
            let test_menu = build_menu(&context.worker.as_ref().unwrap().view.snapshot(), None)
                .map_err(|e| e.to_string())?;
            DestroyMenu(test_menu).map_err(|e| e.to_string())?;
            SetTimer(Some(window), 1, seconds * 1000, None);
        }
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == 0 {
                break;
            }
            if result < 0 {
                return Err(windows::core::Error::from_thread().to_string());
            }
            if let Some(simulator) = context.simulator.window()
                && IsDialogMessageW(simulator, &message).as_bool()
            {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if let Some(worker) = context.worker.take() {
            let _ = worker.sender.send(Input::Exit);
            worker
                .join
                .join()
                .map_err(|_| "Bluetooth worker panicked".to_string())?;
            if smoke.is_some() {
                let state = worker.view.snapshot();
                if let Some(error) = &state.error {
                    return Err(error.clone());
                }
                let report = serde_json::json!({
                    "native_tray_created": true, "menu_created": true,
                    "bluetooth_disabled": state.no_bluetooth, "development": state.development,
                    "title": crate::identity::title(context.development), "clean_exit": true,
                    "per_monitor_v2": per_monitor_v2, "window_dpi": window_dpi,
                    "first_run": first_run, "opened_on_launch": opened_on_launch,
                    "minimized_on_launch": minimized_on_launch,
                    "taskbar_window_on_launch": taskbar_window_on_launch,
                    "taskbar_windows_checked": true,
                });
                std::fs::write(
                    context.database.with_extension("smoke-report.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
