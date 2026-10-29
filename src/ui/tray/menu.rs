//! Tray menu implementation.
use super::*;
pub(super) unsafe fn append(
    menu: HMENU,
    id: usize,
    text: &str,
    enabled: bool,
) -> windows::core::Result<()> {
    let text = wide(text);
    unsafe {
        AppendMenuW(
            menu,
            if enabled {
                MF_STRING
            } else {
                MF_STRING | MF_GRAYED
            },
            id,
            PCWSTR(text.as_ptr()),
        )
    }
}
pub(super) unsafe fn align_owner_to_cursor(window: HWND) -> windows::core::Result<POINT> {
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point)?;
        // Native menus/dialogs inherit their owner's DPI. Move the invisible
        // owner to the invoking monitor, including when monitors use different
        // scales. There is no custom window content or font to resize.
        SetWindowPos(
            window,
            None,
            point.x,
            point.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
    }
    Ok(point)
}
fn device_choices(view: &View, choosing: Option<bool>) -> Vec<(u64, String)> {
    // First-use multiple matches still need a choice. Once a default is saved,
    // only an explicit Scan reveals the chooser again.
    if !choosing.unwrap_or(view.selected.is_none()) || view.candidates.is_empty() {
        return vec![];
    }
    crate::ui::model::device::choices(view)
}
pub(super) unsafe fn build_menu(
    view: &View,
    choosing: Option<bool>,
) -> windows::core::Result<HMENU> {
    let popup = unsafe { CreatePopupMenu()? };
    unsafe {
        append(popup, 0, &view.status, false)?;
        append(
            popup,
            0,
            &format!("Today: {} recorded steps", view.totals.recorded_steps),
            false,
        )?;
        append(popup, DASHBOARD, "Open", true)?;
        if view.development {
            append(popup, SIMULATOR, "Open Simulator", true)?;
        }
        append(popup, COMPACT, "Open Compact", true)?;
        AppendMenuW(popup, MF_SEPARATOR, 0, PCWSTR::null())?;
        append(
            popup,
            CONNECT,
            "Connect Automatically",
            !view.no_bluetooth || view.development,
        )?;
        if view.automatic_connection {
            CheckMenuItem(popup, CONNECT as u32, (MF_BYCOMMAND | MF_CHECKED).0);
        }
        if view.connected {
            append(
                popup,
                DISCONNECT,
                "Disconnect",
                !view.no_bluetooth || view.development,
            )?;
        }
        append(
            popup,
            FIND,
            "Scan",
            (!view.no_bluetooth || view.development) && !view.connected,
        )?;
        let choices = device_choices(view, choosing);
        if !choices.is_empty() {
            let choose = CreatePopupMenu()?;
            for (i, (candidate, label)) in choices.iter().enumerate() {
                append(
                    choose,
                    CANDIDATE_BASE + i,
                    label,
                    !view.no_bluetooth || view.development,
                )?;
                if view.selected == Some(*candidate) {
                    CheckMenuItem(
                        choose,
                        (CANDIDATE_BASE + i) as u32,
                        (MF_BYCOMMAND | MF_CHECKED).0,
                    );
                }
            }
            AppendMenuW(popup, MF_POPUP, choose.0 as usize, w!("Choose device"))?;
        }
        AppendMenuW(popup, MF_SEPARATOR, 0, PCWSTR::null())?;
        append(popup, EXIT, "Exit", true)?;
    }
    Ok(popup)
}
pub(super) unsafe fn menu(window: HWND, context: &Context) -> windows::core::Result<()> {
    let Some(worker) = &context.worker else {
        return Ok(());
    };
    let view = worker.view.snapshot();
    let choosing = context.device_chooser.get();
    let popup = unsafe { build_menu(&view, choosing)? };
    *context.selected_menu.borrow_mut() = device_choices(&view, choosing)
        .into_iter()
        .map(|(address, _)| address)
        .collect();
    unsafe {
        let point = align_owner_to_cursor(window)?;
        let _ = SetForegroundWindow(window);
        let _ = TrackPopupMenu(popup, TPM_RIGHTBUTTON, point.x, point.y, None, window, None);
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        DestroyMenu(popup)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices(selected: Option<u64>, addresses: &[u64]) -> View {
        View {
            selected,
            candidates: addresses
                .iter()
                .map(|&address| application::Candidate {
                    address,
                    name: "Synthetic pad".into(),
                })
                .collect(),
            ..Default::default()
        }
    }

    unsafe fn chooser(menu: HMENU) -> Option<HMENU> {
        for index in 0..unsafe { GetMenuItemCount(Some(menu)) } {
            let submenu = unsafe { GetSubMenu(menu, index) };
            if !submenu.0.is_null() {
                let mut text = [0; 128];
                let count =
                    unsafe { GetMenuStringW(menu, index as u32, Some(&mut text), MF_BYPOSITION) };
                assert_eq!(
                    String::from_utf16_lossy(&text[..count as usize]),
                    "Choose device"
                );
                return Some(submenu);
            }
        }
        None
    }

    #[test]
    fn native_ui_device_choices_hide_after_selection_and_mark_only_the_saved_device() {
        // Inspect real Win32 menus, using synthetic snapshots and no transport.
        for addresses in [&[][..], &[1][..], &[2][..], &[1, 2][..]] {
            for selected in [None, Some(1), Some(2), Some(3)] {
                for choosing in [None, Some(false), Some(true)] {
                    for no_bluetooth in [false, true] {
                        let mut view = devices(selected, addresses);
                        view.no_bluetooth = no_bluetooth;
                        let visible =
                            !addresses.is_empty() && choosing.unwrap_or(selected.is_none());
                        unsafe {
                            let menu = build_menu(&view, choosing).unwrap();
                            let submenu = chooser(menu);
                            assert_eq!(submenu.is_some(), visible);
                            if let Some(submenu) = submenu {
                                let choices = device_choices(&view, choosing);
                                let expected_count = addresses.len()
                                    + usize::from(
                                        selected.is_some_and(|s| !addresses.contains(&s)),
                                    );
                                assert_eq!(GetMenuItemCount(Some(submenu)), expected_count as i32);
                                for (i, (candidate, label)) in choices.iter().enumerate() {
                                    let id = (CANDIDATE_BASE + i) as u32;
                                    let state = GetMenuState(submenu, id, MF_BYCOMMAND);
                                    assert_eq!(
                                        state & MF_CHECKED.0 != 0,
                                        selected == Some(*candidate)
                                    );
                                    assert_eq!(state & MF_GRAYED.0 != 0, no_bluetooth);
                                    let mut text = [0; 128];
                                    let count =
                                        GetMenuStringW(submenu, id, Some(&mut text), MF_BYCOMMAND);
                                    assert_eq!(
                                        String::from_utf16_lossy(&text[..count as usize]),
                                        *label
                                    );
                                }
                                if selected.is_some_and(|s| !addresses.contains(&s)) {
                                    assert!(
                                        choices.last().unwrap().1.starts_with("Saved device (")
                                    );
                                }
                            }
                            DestroyMenu(menu).unwrap();
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn native_ui_scan_choose_rescan_dispatch_preserves_current_connection_and_menu_addresses() {
        let (worker, commands) = application::testing::inert_worker(devices(Some(3), &[1, 2]));
        let context = Context {
            worker: Some(worker),
            development: false,
            simulator: crate::ui::simulator::Handle::default(),
            database: "unused-synthetic.sqlite".into(),
            icon: HICON::default(),
            tip: RefCell::new(String::new()),
            taskbar_created: u32::MAX,
            selected_menu: RefCell::new(vec![]),
            device_chooser: Cell::new(None),
            closing: Cell::new(false),
            main: crate::ui::main_window::Handle::default(),
            compact: crate::ui::compact::Handle::default(),
            shortcuts: RefCell::new(None),
            locked: Cell::new(false),
        };
        unsafe {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Synthetic tray dispatcher"),
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
            SetWindowLongPtrW(window, GWLP_USERDATA, (&context as *const Context) as isize);
            let view = &context.worker.as_ref().unwrap().view;
            assert!(device_choices(&view.snapshot(), context.device_chooser.get()).is_empty());
            for selected_index in [2, 1] {
                window_proc(window, WM_COMMAND, WPARAM(FIND), LPARAM(0));
                assert!(matches!(commands.recv().unwrap(), Input::ActiveScan));
                let choices = device_choices(&view.snapshot(), context.device_chooser.get());
                assert_eq!(
                    choices.iter().map(|(a, _)| *a).collect::<Vec<_>>(),
                    vec![1, 2, 3]
                );
                *context.selected_menu.borrow_mut() = choices.iter().map(|(a, _)| *a).collect();
                // A stale/nonexistent command must not dismiss valid choices.
                window_proc(window, WM_COMMAND, WPARAM(CANDIDATE_BASE + 9), LPARAM(0));
                assert_eq!(context.device_chooser.get(), Some(true));
                window_proc(
                    window,
                    WM_COMMAND,
                    WPARAM(CANDIDATE_BASE + selected_index),
                    LPARAM(0),
                );
                if selected_index == 1 {
                    assert!(matches!(commands.recv().unwrap(), Input::Select(2)));
                }
                // The sentinel must be next: choosing the checked saved device
                // must not enqueue Select (which disconnects the current link).
                context
                    .worker
                    .as_ref()
                    .unwrap()
                    .sender
                    .send(Input::Exit)
                    .unwrap();
                assert!(matches!(commands.recv().unwrap(), Input::Exit));
                assert!(device_choices(&view.snapshot(), context.device_chooser.get()).is_empty());
                // Still hidden even before the worker publishes its saved target.
                application::testing::publish(view, |v| v.selected = None);
                assert!(device_choices(&view.snapshot(), context.device_chooser.get()).is_empty());
                application::testing::publish(view, |v| v.selected = Some(3));
            }
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            DestroyWindow(window).unwrap();
        }
    }

    #[test]
    fn native_ui_tray_menu_labels_checkbox_and_disconnect_follow_snapshot() {
        // Synthetic snapshots only: no worker or Bluetooth adapter is created.
        for connected in [false, true] {
            for automatic_connection in [false, true] {
                for no_bluetooth in [false, true] {
                    let view = View {
                        connected,
                        automatic_connection,
                        no_bluetooth,
                        ..Default::default()
                    };
                    unsafe {
                        let menu = build_menu(&view, None).unwrap();
                        for (id, expected) in [
                            (DASHBOARD, "Open"),
                            (COMPACT, "Open Compact"),
                            (CONNECT, "Connect Automatically"),
                            (FIND, "Scan"),
                        ] {
                            let mut text = [0; 128];
                            let count =
                                GetMenuStringW(menu, id as u32, Some(&mut text), MF_BYCOMMAND);
                            assert_eq!(String::from_utf16_lossy(&text[..count as usize]), expected);
                        }
                        // ID 4 was the removed tray Diagnostics command.
                        assert_eq!(GetMenuState(menu, 4, MF_BYCOMMAND), u32::MAX);
                        // ID 5 was the removed tray Open data folder command.
                        assert_eq!(GetMenuState(menu, 5, MF_BYCOMMAND), u32::MAX);
                        let disconnect = GetMenuState(menu, DISCONNECT as u32, MF_BYCOMMAND);
                        assert_eq!(disconnect != u32::MAX, connected);
                        let automatic = GetMenuState(menu, CONNECT as u32, MF_BYCOMMAND);
                        assert_eq!(automatic & MF_CHECKED.0 != 0, automatic_connection);
                        assert_eq!(automatic & MF_GRAYED.0 != 0, no_bluetooth);
                        let scan = GetMenuState(menu, FIND as u32, MF_BYCOMMAND);
                        assert_eq!(scan & MF_GRAYED.0 != 0, no_bluetooth || connected);
                        DestroyMenu(menu).unwrap();
                    }
                }
            }
        }
    }
}
