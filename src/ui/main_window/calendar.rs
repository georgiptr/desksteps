//! Month calendar popup (SysMonthCal32) that reports the picked day to the main window.
use chrono::{Datelike, NaiveDate};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SYSTEMTIME, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{Controls::*, Input::KeyboardAndMouse::SetFocus, WindowsAndMessaging::*},
    },
    core::w,
};

/// Posted to the owner; `wparam` is `yyyymmdd`.
pub const PICKED: u32 = WM_APP + 41;

pub fn encode(date: NaiveDate) -> usize {
    date.year() as usize * 10000 + date.month() as usize * 100 + date.day() as usize
}
pub fn decode(value: usize) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(
        (value / 10000) as i32,
        (value / 100 % 100) as u32,
        (value % 100) as u32,
    )
}
/// Opens the popup with its top-left at `anchor` (screen pixels), selecting `date`.
pub unsafe fn open(owner: HWND, anchor: POINT, date: NaiveDate) -> windows::core::Result<HWND> {
    unsafe {
        let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_DATE_CLASSES,
        });
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: w!("DeskStepsCalendar"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        // Repeated registration in the same process is harmless.
        let _ = RegisterClassW(&class);
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("DeskStepsCalendar"),
            w!(""),
            WS_POPUP | WS_BORDER | WS_CLIPCHILDREN,
            anchor.x,
            anchor.y,
            300,
            300,
            Some(owner),
            None,
            Some(instance),
            None,
        )?;
        let calendar = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            MONTHCAL_CLASSW,
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            10,
            10,
            Some(window),
            Some(HMENU(std::ptr::dangling_mut())),
            Some(instance),
            None,
        )?;
        let st = SYSTEMTIME {
            wYear: date.year() as u16,
            wMonth: date.month() as u16,
            wDay: date.day() as u16,
            ..Default::default()
        };
        SendMessageW(
            calendar,
            MCM_SETCURSEL,
            None,
            Some(LPARAM(&st as *const _ as isize)),
        );
        let mut need = RECT::default();
        SendMessageW(
            calendar,
            MCM_GETMINREQRECT,
            None,
            Some(LPARAM(&mut need as *mut _ as isize)),
        );
        let (w, h) = (need.right - need.left, need.bottom - need.top);
        let _ = SetWindowPos(calendar, None, 0, 0, w, h, SWP_NOZORDER);
        let _ = SetWindowPos(window, None, anchor.x, anchor.y, w + 2, h + 2, SWP_NOZORDER);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetFocus(Some(calendar));
        Ok(window)
    }
}
unsafe extern "system" fn proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NOTIFY => {
                let header = &*(lparam.0 as *const NMHDR);
                if header.code == MCN_SELECT {
                    let mut st = SYSTEMTIME::default();
                    SendMessageW(
                        header.hwndFrom,
                        MCM_GETCURSEL,
                        None,
                        Some(LPARAM(&mut st as *mut _ as isize)),
                    );
                    if let Some(date) =
                        NaiveDate::from_ymd_opt(st.wYear as i32, st.wMonth as u32, st.wDay as u32)
                        && let Ok(owner) = GetWindow(window, GW_OWNER)
                    {
                        let _ = PostMessageW(Some(owner), PICKED, WPARAM(encode(date)), LPARAM(0));
                    }
                    let _ = DestroyWindow(window);
                }
                LRESULT(0)
            }
            WM_ACTIVATE if wparam.0 & 0xffff == 0 => {
                let _ = DestroyWindow(window);
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == 0x1b => {
                let _ = DestroyWindow(window);
                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picked_dates_round_trip_and_reject_garbage() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        assert_eq!(encode(d), 20261023);
        assert_eq!(decode(20261023), Some(d));
        assert_eq!(decode(20261323), None);
        assert_eq!(decode(0), None);
    }
}
