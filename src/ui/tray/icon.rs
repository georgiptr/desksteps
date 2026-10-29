//! Tray icon implementation.
use super::*;
pub(super) fn icon(development: bool) -> windows::core::Result<HICON> {
    // A small blue tile with a white belt and footsteps, drawn directly into
    // the native icon bitmap. No browser/UI runtime or external image assets.
    let mut pixels = vec![0u8; 32 * 32 * 4];
    let mask = [0u8; 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let index = (y * 32 + x) * 4;
            pixels[index..index + 4].copy_from_slice(&crate::identity::pixel(
                x as u32,
                y as u32,
                development,
            ));
        }
    }
    unsafe { CreateIcon(None, 32, 32, 1, 32, mask.as_ptr(), pixels.as_ptr()) }
}
pub(super) fn icon_data(window: HWND, context: &Context, tip: &str) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: ICON_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: CALLBACK,
        hIcon: context.icon,
        ..Default::default()
    };
    let units: Vec<u16> = tip.encode_utf16().take(127).collect();
    data.szTip[..units.len()].copy_from_slice(&units);
    data
}
pub(super) unsafe fn add_icon(window: HWND, context: &Context) -> Result<(), String> {
    let tip = tooltip(context);
    let mut data = icon_data(window, context, &tip);
    if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
        return Err("Windows could not add the tray icon".into());
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    if !unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) }.as_bool() {
        return Err("Windows could not set the tray callback version".into());
    }
    *context.tip.borrow_mut() = tip;
    Ok(())
}
fn tooltip(context: &Context) -> String {
    if let Some(worker) = &context.worker {
        let view = worker.view.snapshot();
        format!(
            "{}: {}\n{} recorded steps today{}",
            crate::identity::title(context.development),
            view.status,
            view.totals.recorded_steps,
            if view.error.is_some() {
                ": check status"
            } else {
                ""
            }
        )
    } else {
        format!("{}: starting", crate::identity::title(context.development))
    }
}
pub(super) unsafe fn update_icon(window: HWND, context: &Context) {
    let tip = tooltip(context);
    if *context.tip.borrow() != tip
        && unsafe { Shell_NotifyIconW(NIM_MODIFY, &icon_data(window, context, &tip)) }.as_bool()
    {
        *context.tip.borrow_mut() = tip;
    }
}
