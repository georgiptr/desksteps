//! Native information, error and confirmation dialogs shared by startup and UI.
use super::native::wide;
use windows::{
    Win32::UI::WindowsAndMessaging::*,
    core::{PCWSTR, w},
};
pub fn error_dialog(text: &str) {
    let text = wide(text);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            w!("DeskSteps"),
            MB_OK | MB_ICONERROR,
        );
    }
}
pub fn information(text: &str) {
    let text = wide(text);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            w!("DeskSteps"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
pub fn confirm(text: &str) -> bool {
    let text = wide(text);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            w!("DeskSteps"),
            MB_OKCANCEL | MB_ICONINFORMATION,
        ) == IDOK
    }
}
