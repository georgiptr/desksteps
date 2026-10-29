//! Small native helpers, not a UI framework. Only owned GDI fonts enter FontSlot.
use std::cell::Cell;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        Graphics::Gdi::{DeleteObject, HFONT},
        UI::WindowsAndMessaging::{
            GetWindowTextLengthW, GetWindowTextW, HICON, ICON_BIG, ICON_SMALL, SendMessageW,
            SetWindowTextW, WM_GETICON, WM_SETICON,
        },
    },
    core::PCWSTR,
};
pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub(super) unsafe fn set_text(window: HWND, value: &str) {
    let value = wide(value);
    unsafe {
        let _ = SetWindowTextW(window, PCWSTR(value.as_ptr()));
    }
}
pub(super) unsafe fn read_text(window: HWND) -> String {
    unsafe {
        let mut value = vec![0u16; (GetWindowTextLengthW(window) + 1) as usize];
        let count = GetWindowTextW(window, &mut value);
        String::from_utf16_lossy(&value[..count as usize])
    }
}
/// The icon owner must outlive the window; WM_SETICON borrows the handle.
pub(super) unsafe fn set_window_icon(window: HWND, icon: HICON) {
    for size in [ICON_SMALL, ICON_BIG] {
        unsafe {
            SendMessageW(
                window,
                WM_SETICON,
                Some(WPARAM(size as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
    }
}
/// Owned windows borrow both icons from their longer-lived tray owner.
pub(super) unsafe fn inherit_window_icons(window: HWND, owner: HWND) {
    for size in [ICON_SMALL, ICON_BIG] {
        unsafe {
            let icon = SendMessageW(owner, WM_GETICON, Some(WPARAM(size as usize)), None);
            if icon.0 != 0 {
                SendMessageW(
                    window,
                    WM_SETICON,
                    Some(WPARAM(size as usize)),
                    Some(LPARAM(icon.0)),
                );
            }
        }
    }
}
pub(super) struct OwnedFont(HFONT);
impl Drop for OwnedFont {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.0.into());
            }
        }
    }
}
pub(super) struct FontSlot(Cell<HFONT>);
impl FontSlot {
    pub fn new() -> Self {
        Self(Cell::new(HFONT::default()))
    }
    pub fn get(&self) -> HFONT {
        self.0.get()
    }
    /// Retain the returned owner until native children have received the new font.
    pub fn replace(&self, font: HFONT) -> OwnedFont {
        OwnedFont(self.0.replace(font))
    }
}
impl Drop for FontSlot {
    fn drop(&mut self) {
        drop(OwnedFont(self.0.replace(HFONT::default())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf16_text_is_terminated_and_preserves_unicode() {
        for text in ["", "DeskSteps", "Пауза 🚶"] {
            let wide = wide(text);
            assert_eq!(wide.last(), Some(&0));
            assert_eq!(String::from_utf16(&wide[..wide.len() - 1]).unwrap(), text);
        }
    }
}
