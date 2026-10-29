//! Opt-in screenshots of the actual native renderer, using synthetic channels.
use super::*;

#[test]
#[ignore = "Exports public documentation screenshots using inert synthetic snapshots"]
fn native_ui_screenshots_for_readme() {
    use crate::statistics::{Preferences, Session};
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
    };
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/readme-screenshots");
    std::fs::create_dir_all(&folder).unwrap();
    let mut dashboard = dash("2026-10-23", 30);
    let preferences = Preferences {
        effective: "2026-10-01".into(),
        weight: Some(75.0),
        steps: Some(5000),
        minutes: Some(60.0),
        calories: Some(200.0),
    };
    for (i, day) in dashboard.days.iter_mut().enumerate() {
        day.steps = 1800 + (i as i64 * 613 % 4300);
        day.active_ms = day.steps * 700;
        day.kcal = Some(day.steps as f64 * 0.04);
        day.preferences = preferences.clone();
        day.sessions = 2;
    }
    dashboard.day.steps = 3450;
    dashboard.day.active_ms = 42 * 60_000;
    dashboard.day.kcal = Some(143.0);
    dashboard.day.preferences = preferences;
    dashboard.day.sessions = 2;
    dashboard.day.samples = 2520;
    *dashboard.days.last_mut().unwrap() = dashboard.day.clone();
    dashboard.sessions = [
        ("09:00", "09:25", 2050, 25, 85.0),
        ("11:25", "11:42", 1400, 17, 58.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (start, end, steps, minutes, kcal))| Session {
        id: i as i64 + 1,
        start: format!("2026-10-23T{start}:00Z"),
        end: format!("2026-10-23T{end}:00Z"),
        state: "Closed".into(),
        steps,
        active_ms: minutes * 60_000,
        kcal: Some(kcal),
        ..Default::default()
    })
    .collect();
    let mut initial = view();
    initial.status = "Connected".into();
    initial.selected = Some(0x020000000001);
    initial.selected_name = Some("WalkingPad Z1".into());
    initial.automatic_connection = true;
    initial.recover_gaps = true;
    initial.theme_preference = crate::appearance::ThemePreference::Light;
    // Show the normal Settings controls through inert channels, without registry writes.
    initial.windows_startup_available = true;
    initial.totals.day = dashboard.today.clone();
    initial.totals.recorded_steps = dashboard.day.steps;
    initial.dashboard = Some(dashboard.into());
    let (sender, reader, _raw, _rx) = channels(256, initial.clone());
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let owner = owner();
        let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
        let s = state(window);
        SendMessageW(window, UPDATE, None, None);
        let scale = dpi(window) as f32 / 96.0;
        println!("Documentation screenshot DPI: {}", dpi(window));
        for (suffix, width, height) in [("", 860.0, 680.0), ("-narrow", 520.0, 720.0)] {
            SetWindowPos(
                window,
                None,
                0,
                0,
                (width * scale) as i32,
                (height * scale) as i32,
                SWP_NOMOVE | SWP_NOZORDER,
            )
            .unwrap();
            for (page, name) in [
                (Page::Today, "today"),
                (Page::History, "history"),
                (Page::Targets, "targets"),
                (Page::Settings, "settings"),
            ] {
                select_page(window, s, page);
                s.scroll.set([0.0; 5]);
                s.refresh(window);
                capture(window, &folder.join(format!("{name}{suffix}.bmp")));
            }
        }
        SendMessageW(window, WM_CLOSE, None, None);
        let (worker, _commands) = crate::application::testing::inert_worker(initial);
        let compact = crate::ui::compact::Handle::default();
        compact.toggle(owner, &worker).unwrap();
        let compact_window = compact.window().unwrap();
        SendMessageW(compact_window, crate::ui::compact::UPDATE, None, None);
        capture(compact_window, &folder.join("compact.bmp"));
        compact.close();
        worker.join.join().unwrap();
        let _ = DestroyWindow(owner);
        SetThreadDpiAwarenessContext(previous_dpi);
    }
}

unsafe fn capture(window: HWND, path: &std::path::Path) {
    unsafe {
        assert!(
            RedrawWindow(
                Some(window),
                None,
                None,
                RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN
            )
            .as_bool()
        );
        windows::Win32::Graphics::Dwm::DwmFlush().unwrap();
        let mut rect = RECT::default();
        GetClientRect(window, &mut rect).unwrap();
        let (width, height) = (rect.right, rect.bottom);
        let source = GetDC(Some(window));
        let destination = CreateCompatibleDC(Some(source));
        let bitmap = CreateCompatibleBitmap(source, width, height);
        let previous = SelectObject(destination, bitmap.into());
        BitBlt(
            destination,
            0,
            0,
            width,
            height,
            Some(source),
            0,
            0,
            SRCCOPY,
        )
        .unwrap();
        SelectObject(destination, previous);
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        assert_eq!(
            GetDIBits(
                destination,
                bitmap,
                0,
                height as u32,
                Some(pixels.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS
            ),
            height
        );
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0),
            "Screenshot is blank"
        );
        let mut bmp = Vec::new();
        bmp.extend(b"BM");
        bmp.extend((54 + pixels.len() as u32).to_le_bytes());
        bmp.extend(0u32.to_le_bytes());
        bmp.extend(54u32.to_le_bytes());
        let header = std::slice::from_raw_parts(
            (&info.bmiHeader as *const BITMAPINFOHEADER).cast::<u8>(),
            40,
        );
        bmp.extend(header);
        bmp.extend(pixels);
        std::fs::write(path, bmp).unwrap();
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(destination);
        ReleaseDC(Some(window), source);
    }
}

#[test]
#[ignore = "Exports native screenshots for visual review; use an isolated offline test"]
fn native_ui_screenshots_of_targets_settings_and_calories() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ui-screenshots");
    std::fs::create_dir_all(&folder).unwrap();
    let (sender, reader, raw, _rx) = channels(256, view());
    // Model the normal Settings surface without registering any real startup entry.
    raw.lock().unwrap().windows_startup_available = true;
    raw.lock().unwrap().selected = Some(1);
    raw.lock().unwrap().selected_name = Some("Synthetic saved WalkingPad".into());
    raw.lock().unwrap().totals.day = "2026-10-23".into();
    let mut dashboard = dash("2026-10-23", 7);
    dashboard.day.preferences = crate::statistics::Preferences {
        weight: Some(80.0),
        steps: Some(2000),
        minutes: Some(200.0),
        calories: Some(400.0),
        ..Default::default()
    };
    dashboard.day.steps = 2;
    dashboard.day.active_ms = 120 * 60_000;
    dashboard.day.kcal = Some(120.0);
    dashboard.day.pad_calories.total = Some(60.0);
    raw.lock().unwrap().dashboard = Some(dashboard.into());
    unsafe {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
        };
        let previous_dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let owner = owner();
        let window = create(owner, sender, reader, Rc::new(Cell::new(None))).unwrap();
        let s = state(window);
        SendMessageW(window, UPDATE, None, None);
        let scale = dpi(window) as f32 / 96.0;
        println!("Screenshot DPI: {}", dpi(window));
        for (name, width, height) in [("normal", 800.0, 600.0), ("narrow", 520.0, 720.0)] {
            SetWindowPos(
                window,
                None,
                0,
                0,
                (width * scale) as i32,
                (height * scale) as i32,
                SWP_NOMOVE | SWP_NOZORDER,
            )
            .unwrap();
            for page in [Page::Targets, Page::Settings, Page::Today] {
                select_page(window, s, page);
                s.scroll.set([0.0; 5]);
                s.refresh(window);
                capture(window, &folder.join(format!("{page:?}-{name}.bmp")));
                match page {
                    Page::Targets => {
                        forms::put_text(s, FIELD_BASE, "1");
                        forms::put_text(s, FIELD_BASE + 1, "-3");
                        activate(window, s, TARGETS_SAVE, false);
                        capture(window, &folder.join(format!("Targets-errors-{name}.bmp")));
                        forms::put_text(s, FIELD_BASE, "80");
                        forms::put_text(s, FIELD_BASE + 1, "2000");
                        s.form.borrow_mut().errors = Default::default();
                    }
                    Page::Settings => {
                        raw.lock().unwrap().theme_preference =
                            crate::appearance::ThemePreference::Dark;
                        SendMessageW(window, UPDATE, None, None);
                        capture(window, &folder.join(format!("Settings-dark-{name}.bmp")));
                        raw.lock().unwrap().theme_preference =
                            crate::appearance::ThemePreference::Light;
                        raw.lock().unwrap().startup_error =
                            Some("Cannot enable Windows startup: Access is denied.".into());
                        SendMessageW(window, UPDATE, None, None);
                        capture(
                            window,
                            &folder.join(format!("Settings-startup-error-{name}.bmp")),
                        );
                        {
                            let mut v = raw.lock().unwrap();
                            v.startup_error = None;
                            v.windows_startup = true;
                            v.window_behavior.start_minimized = false;
                            v.window_behavior.minimize_to_tray = true;
                            v.window_behavior.keep_running_on_close = false;
                        }
                        SendMessageW(window, UPDATE, None, None);
                        capture(window, &folder.join(format!("Settings-options-{name}.bmp")));
                        {
                            let mut v = raw.lock().unwrap();
                            v.windows_startup = false;
                            v.window_behavior = Default::default();
                        }
                        SendMessageW(window, UPDATE, None, None);
                        s.form.borrow_mut().shortcut_errors[1] =
                            Some("Shortcuts require at least two of Ctrl, Alt and Shift".into());
                        s.refresh(window);
                        let top = s
                            .scene
                            .borrow()
                            .blocks
                            .iter()
                            .find_map(|(r, b)| {
                                matches!(b, pages::Block::Section("Shortcuts")).then_some(r.y)
                            })
                            .unwrap();
                        scroll_by(window, s, top);
                        capture(window, &folder.join(format!("Settings-errors-{name}.bmp")));
                        s.form.borrow_mut().shortcut_errors = Default::default();
                    }
                    Page::Today => {
                        let mut v = raw.lock().unwrap();
                        let d = std::sync::Arc::make_mut(v.dashboard.as_mut().unwrap());
                        d.day.preferences.weight = None;
                        drop(v);
                        SendMessageW(window, UPDATE, None, None);
                        capture(window, &folder.join(format!("Today-pad-{name}.bmp")));
                        let mut v = raw.lock().unwrap();
                        std::sync::Arc::make_mut(v.dashboard.as_mut().unwrap())
                            .day
                            .preferences
                            .weight = Some(80.0);
                    }
                    _ => unreachable!(),
                }
            }
        }
        SendMessageW(window, WM_CLOSE, None, None);
        let _ = DestroyWindow(owner);
        SetThreadDpiAwarenessContext(previous_dpi);
    }
    println!("Native screenshots: {}", folder.display());
}
