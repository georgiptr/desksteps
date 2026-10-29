//! Owners unwind native setup failures while Context is still alive.
use super::*;
use windows::Win32::Foundation::HANDLE;
pub(super) struct MutexOwner(pub HANDLE);
impl Drop for MutexOwner {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub(super) struct IconOwner(pub HICON);
impl Drop for IconOwner {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}
pub(super) struct WindowOwner(pub Option<HWND>);
impl Drop for WindowOwner {
    fn drop(&mut self) {
        if let Some(window) = self.0
            && unsafe { IsWindow(Some(window)) }.as_bool()
        {
            // Context outlives this guard. WM_CLOSE unregisters callbacks and
            // closes child windows before any of their state can be dropped.
            unsafe {
                SendMessageW(window, WM_CLOSE, None, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fail_after_window(window: HWND) -> Result<(), String> {
        let _owner = WindowOwner(Some(window));
        Err("synthetic setup failure".into())
    }
    #[test]
    fn native_ui_window_owner_closes_its_window_when_setup_unwinds() {
        unsafe {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Synthetic cleanup test"),
                WINDOW_STYLE(0),
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let result = fail_after_window(window);
            assert!(result.is_err());
            assert!(!IsWindow(Some(window)).as_bool());
        }
    }
}
