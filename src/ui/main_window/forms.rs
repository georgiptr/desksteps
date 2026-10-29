//! Native EDIT fields for the Targets and Shortcuts forms, themed through
//! WM_CTLCOLOREDIT, plus their save handlers. The window draws each field's frame.
use super::*;
use crate::ui::{
    model::{shortcuts_form, targets},
    native::{read_text, set_text},
};
use windows::Win32::{
    Graphics::Gdi::{
        CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateRectRgn, CreateSolidBrush,
        DEFAULT_CHARSET, DeleteObject, HBRUSH, HDC, OUT_DEFAULT_PRECIS, RDW_ALLCHILDREN,
        RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow, SetBkColor, SetTextColor, SetWindowRgn,
    },
    UI::{
        Controls::{EM_SETLIMITTEXT, EM_SETSEL, SetWindowTheme},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
    },
};

/// Posted by an EDIT when Tab is pressed in it; `wparam` is 1 for Shift+Tab.
pub const FIELD_TAB: u32 = WM_APP + 42;
const EN_SETFOCUS: usize = 0x0100;
const EN_KILLFOCUS: usize = 0x0200;
const WM_CTLCOLOREDIT: u32 = 0x0133;

pub const TARGET_FIELDS: [Id; 4] = [FIELD_BASE, FIELD_BASE + 1, FIELD_BASE + 2, FIELD_BASE + 3];
pub const SHORTCUT_FIELDS: [Id; 4] = [
    FIELD_BASE + 10,
    FIELD_BASE + 11,
    FIELD_BASE + 12,
    FIELD_BASE + 13,
];
pub fn is_field(id: Id) -> bool {
    TARGET_FIELDS.contains(&id) || SHORTCUT_FIELDS.contains(&id)
}
fn bgr(c: theme::Color) -> COLORREF {
    COLORREF(c.r as u32 | (c.g as u32) << 8 | (c.b as u32) << 16)
}
pub fn edit(s: &State, id: Id) -> Option<HWND> {
    s.edits
        .borrow()
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, h)| *h)
}
pub fn text(s: &State, id: Id) -> String {
    edit(s, id)
        .map(|h| unsafe { read_text(h) })
        .unwrap_or_default()
}
pub fn put_text(s: &State, id: Id, value: &str) {
    if let Some(h) = edit(s, id) {
        unsafe { set_text(h, value) };
    }
}
unsafe extern "system" fn edit_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    main: usize,
) -> LRESULT {
    unsafe {
        match message {
            WM_KEYDOWN if wparam.0 as u16 == VK_TAB.0 => {
                let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                let _ = PostMessageW(
                    Some(HWND(main as *mut _)),
                    FIELD_TAB,
                    WPARAM(shift as usize),
                    LPARAM(0),
                );
                LRESULT(0)
            }
            // Tab and Enter do nothing in a field: no default button exists.
            WM_CHAR if wparam.0 == 9 || wparam.0 == 13 => LRESULT(0),
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(window, Some(edit_proc), 1);
                DefSubclassProc(window, message, wparam, lparam)
            }
            _ => DefSubclassProc(window, message, wparam, lparam),
        }
    }
}
pub unsafe fn create_edits(window: HWND, s: &State) -> windows::core::Result<()> {
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        for id in TARGET_FIELDS.into_iter().chain(SHORTCUT_FIELDS) {
            let h = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                0,
                0,
                10,
                10,
                Some(window),
                Some(HMENU(id as usize as *mut _)),
                Some(instance),
                None,
            )?;
            let _ = SetWindowSubclass(h, Some(edit_proc), 1, window.0 as usize);
            let limit = if SHORTCUT_FIELDS.contains(&id) {
                48
            } else {
                16
            };
            SendMessageW(h, EM_SETLIMITTEXT, Some(WPARAM(limit)), None);
            s.edits.borrow_mut().push((id, h));
        }
    }
    unsafe { apply_font(window, s) };
    unsafe { apply_theme_to_edits(s) };
    Ok(())
}
/// Body font at the window's DPI for every field; the old font outlives the switch.
pub unsafe fn apply_font(window: HWND, s: &State) {
    unsafe {
        let scale = dpi(window) as i32;
        let font = CreateFontW(
            -14 * scale / 96,
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
        );
        let old = s.font.replace(font);
        for (_, h) in s.edits.borrow().iter() {
            SendMessageW(
                *h,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        drop(old);
    }
}
/// Rebuilds the field background brush and asks Windows for dark scrollbars/menus.
pub unsafe fn apply_theme_to_edits(s: &State) {
    let th = s.theme.get();
    unsafe {
        let old = s.brush.replace(CreateSolidBrush(bgr(th.control)));
        if !old.is_invalid() {
            let _ = DeleteObject(old.into());
        }
        for (_, h) in s.edits.borrow().iter() {
            let name = if th.is_dark() {
                w!("DarkMode_Explorer")
            } else {
                w!("Explorer")
            };
            let _ = SetWindowTheme(*h, name, None);
            let _ = InvalidateRect(Some(*h), None, true);
        }
    }
}
pub unsafe fn free_brush(s: &State) {
    let old = s.brush.replace(HBRUSH::default());
    if !old.is_invalid() {
        unsafe {
            let _ = DeleteObject(old.into());
        }
    }
}
/// Colours for WM_CTLCOLOREDIT: returns the brush to paint with.
pub unsafe fn colors(s: &State, dc: HDC) -> LRESULT {
    let th = s.theme.get();
    unsafe {
        SetTextColor(dc, bgr(th.text));
        SetBkColor(dc, bgr(th.control));
    }
    LRESULT(s.brush.get().0 as isize)
}
pub const CTLCOLOREDIT: u32 = WM_CTLCOLOREDIT;
/// Moves each field to its place in the scene, clipped to the page body; fields
/// that are not on this screen or are scrolled out of view are hidden.
pub unsafe fn place(window: HWND, s: &State) {
    let scene = s.scene.borrow();
    let scale = unsafe { dpi(window) } as f32 / 96.0;
    let body = scene.frame.body;
    let px = |v: f32| (v * scale).round() as i32;
    let mut flipped = false;
    for (id, h) in s.edits.borrow().iter() {
        let at = scene.fields.iter().find(|(i, _)| i == id).map(|(_, r)| *r);
        let visible = at.and_then(|r| {
            let x = r.x.max(body.x);
            let y = r.y.max(body.y);
            let right = r.right().min(body.right());
            let bottom = r.bottom().min(body.bottom());
            (px(right - r.x) > px(x - r.x) && px(bottom - r.y) > px(y - r.y))
                .then_some((r, x, y, right, bottom))
        });
        unsafe {
            flipped |= IsWindowVisible(*h).as_bool() != visible.is_some();
            match visible {
                None => {
                    let _ = ShowWindow(*h, SW_HIDE);
                }
                Some((r, x, y, right, bottom)) => {
                    let _ = SetWindowPos(
                        *h,
                        None,
                        px(r.x),
                        px(r.y),
                        px(r.w),
                        px(r.h),
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    let clip =
                        CreateRectRgn(px(x - r.x), px(y - r.y), px(right - r.x), px(bottom - r.y));
                    SetWindowRgn(*h, Some(clip), true);
                }
            }
        }
    }
    if flipped {
        // A field that appears or disappears leaves stale pixels unless the whole
        // window, children included, is repainted at once.
        unsafe {
            let _ = RedrawWindow(
                Some(window),
                None,
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW,
            );
        }
    }
}

/// Moves the real keyboard focus to match the toolkit focus: into a field, or back to
/// the window when the focus moved to something else.
pub unsafe fn sync_focus(window: HWND, s: &State) {
    let focus = s.pointer.borrow().focus;
    unsafe {
        match focus.and_then(|id| edit(s, id).map(|h| (id, h))) {
            Some((_, h)) => {
                if GetFocus() != h {
                    let _ = SetFocus(Some(h));
                    SendMessageW(h, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                }
            }
            None => {
                let current = GetFocus();
                if s.edits.borrow().iter().any(|(_, h)| *h == current) {
                    let _ = SetFocus(Some(window));
                }
            }
        }
    }
}
/// WM_COMMAND notifications from the fields keep the toolkit focus in step.
pub fn notification(s: &State, id: usize, code: usize) -> bool {
    let id = id as Id;
    if !is_field(id) {
        return false;
    }
    let mut pointer = s.pointer.borrow_mut();
    match code {
        EN_SETFOCUS => {
            pointer.focus = Some(id);
            pointer.focus_visible = false;
        }
        EN_KILLFOCUS if pointer.focus == Some(id) => pointer.focus = None,
        _ => {}
    }
    true
}
/// Fills the fields once from saved state: targets from today's dashboard day,
/// shortcuts from the current bindings.
pub fn fill(s: &State, view: &crate::application::View, screen: Screen) {
    let mut done = s.filled.get();
    if screen == Screen::Targets
        && !done[0]
        && let Some(day) = s.today.borrow().as_ref()
    {
        let p = &day.preferences;
        for (id, value) in [
            (TARGET_FIELDS[0], p.weight),
            (TARGET_FIELDS[1], p.steps.map(|v| v as f64)),
            (TARGET_FIELDS[2], p.minutes),
            (TARGET_FIELDS[3], p.calories),
        ] {
            put_text(s, id, &value.map(|v| format!("{v}")).unwrap_or_default());
        }
        done[0] = true;
    }
    if screen == Screen::Settings && !done[1] {
        for (id, binding) in SHORTCUT_FIELDS.iter().zip(&view.shortcuts.bindings) {
            put_text(s, *id, binding);
        }
        done[1] = true;
    }
    s.filled.set(done);
}
/// Validates the Targets form; on success asks for confirmation before the write, and
/// sends nothing if the window was closed while the dialog was open.
pub unsafe fn save_targets(window: HWND, s: &State) {
    let effective = s.form.borrow().effective.clone();
    let effective = if effective.is_empty() {
        today_of(s)
    } else {
        effective
    };
    let result = targets::check(
        &effective,
        &text(s, TARGET_FIELDS[0]),
        &text(s, TARGET_FIELDS[1]),
        &text(s, TARGET_FIELDS[2]),
        &text(s, TARGET_FIELDS[3]),
    );
    match result {
        Err(errors) => {
            let mut form = s.form.borrow_mut();
            form.errors = Default::default();
            for e in errors {
                form.errors[e.field.index()] = Some(e.message);
            }
            drop(form);
            s.refresh(window);
        }
        Ok(p) => {
            s.form.borrow_mut().errors = Default::default();
            s.send(Input::ClearUiMessage);
            let message = crate::ui::native::wide(&targets::confirmation(&p.effective));
            // Nested modal loop: the window may be closed while it runs.
            let confirmed = unsafe {
                MessageBoxW(
                    Some(window),
                    PCWSTR(message.as_ptr()),
                    w!("Save dated settings"),
                    MB_OKCANCEL | MB_ICONINFORMATION,
                ) == IDOK
            };
            if s.closing.get() {
                return;
            }
            if confirmed {
                s.send(Input::SavePreferences(p));
            }
            s.refresh(window);
        }
    }
}
pub unsafe fn save_shortcuts(window: HWND, s: &State) {
    let bindings = SHORTCUT_FIELDS.map(|id| text(s, id));
    let outcome = shortcuts_form::check(bindings);
    {
        let mut form = s.form.borrow_mut();
        form.shortcut_errors = Default::default();
        form.shortcut_general = None;
        form.shortcuts_saved = outcome.is_ok();
        if let Err(e) = &outcome {
            form.shortcut_errors = e.fields.clone();
            form.shortcut_general = e.general.clone();
        }
    }
    if let Ok(config) = outcome {
        s.send(Input::Shortcuts(config));
    }
    s.refresh(window);
}
