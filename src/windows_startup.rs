//! Opt-in, current-user Windows sign-in registration. Offline modes never call this.
use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
            RegSetKeyValueW,
        },
    },
    core::{PCWSTR, w},
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("DeskSteps");

fn command(executable: &Path) -> Result<Vec<u16>, String> {
    let path: Vec<u16> = executable.as_os_str().encode_wide().collect();
    if !executable.is_absolute() || path.iter().any(|c| *c == 0 || *c == b'"' as u16) {
        return Err("Windows startup requires an absolute executable path without quotes".into());
    }
    if path.len() + 2 > 260 {
        return Err(
            "The executable path is too long for Windows startup (260 characters maximum)".into(),
        );
    }
    Ok(std::iter::once(b'"' as u16)
        .chain(path)
        .chain([b'"' as u16, 0])
        .collect())
}
fn read(key: PCWSTR) -> Result<Option<Vec<u16>>, String> {
    // Windows limits Run commands to 260 characters; leave ample room to read
    // registrations written by another copy without truncating/comparing prefixes.
    let mut buffer = vec![0u16; 32_768];
    let mut bytes = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    result
        .ok()
        .map_err(|e| format!("Cannot read Windows startup: {e}"))?;
    buffer.truncate(bytes as usize / 2);
    Ok(Some(buffer))
}
fn registered(key: PCWSTR, executable: &Path) -> Result<bool, String> {
    Ok(read(key)?.as_ref() == Some(&command(executable)?))
}
fn set(key: PCWSTR, executable: &Path, enabled: bool) -> Result<bool, String> {
    let command = command(executable)?;
    if enabled {
        // Create/update only this current-user value, never a service or
        // machine-wide registration.
        let result = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key,
                VALUE,
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * 2) as u32,
            )
        };
        result
            .ok()
            .map_err(|e| format!("Cannot enable Windows startup: {e}"))?;
    } else if registered(key, executable)? {
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, VALUE) };
        if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
            return Err(format!(
                "Cannot disable Windows startup: {}",
                result.to_hresult()
            ));
        }
    }
    registered(key, executable)
}
pub(crate) fn enabled() -> Result<bool, String> {
    registered(
        RUN_KEY,
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )
}
pub(crate) fn configure(enabled: bool) -> Result<bool, String> {
    set(
        RUN_KEY,
        &std::env::current_exe().map_err(|e| e.to_string())?,
        enabled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_command_quotes_the_exact_executable_without_launch_flags() {
        let path = Path::new("C:\\Apps with spaces\\DeskSteps\\desksteps.exe");
        assert_eq!(
            String::from_utf16_lossy(&command(path).unwrap()),
            "\"C:\\Apps with spaces\\DeskSteps\\desksteps.exe\"\0"
        );
        assert!(command(Path::new("desksteps.exe")).is_err());
        assert!(command(Path::new("C:\\invalid\"path.exe")).is_err());
        assert!(command(Path::new("C:\\invalid\0path.exe")).is_err());
        assert!(command(Path::new(&format!("C:\\{}.exe", "a".repeat(260)))).is_err());
    }
    #[test]
    fn native_ui_windows_startup_roundtrip_uses_only_a_disposable_registry_key() {
        use windows::Win32::System::Registry::{
            HKEY, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, RegCloseKey, RegCreateKeyExW,
            RegDeleteTreeW,
        };
        let name: Vec<u16> = format!(
            "Software\\DeskStepsStartupTest-{}-{}\0",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        )
        .encode_utf16()
        .collect();
        let key = PCWSTR(name.as_ptr());
        struct Cleanup(Vec<u16>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                unsafe {
                    let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.0.as_ptr()));
                }
            }
        }
        let _cleanup = Cleanup(name.clone());
        let mut opened = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key,
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut opened,
                None,
            )
            .ok()
            .unwrap();
            RegCloseKey(opened).ok().unwrap();
        }
        let first = Path::new("C:\\Synthetic Apps\\First\\desksteps.exe");
        let other = Path::new("C:\\Synthetic Apps\\Other\\desksteps.exe");
        assert!(!registered(key, first).unwrap());
        assert!(set(key, first, true).unwrap());
        assert!(registered(key, first).unwrap());
        assert!(!registered(key, other).unwrap());
        assert!(!set(key, other, false).unwrap());
        assert!(registered(key, first).unwrap());
        assert!(!set(key, first, false).unwrap());
        assert!(read(key).unwrap().is_none());
        // An invalid subkey fails explicitly rather than reporting success.
        let invalid: Vec<u16> = format!(
            "{}\\{}\0",
            String::from_utf16_lossy(&name[..name.len() - 1]),
            "x".repeat(300)
        )
        .encode_utf16()
        .collect();
        assert!(set(PCWSTR(invalid.as_ptr()), first, true).is_err());
    }
}
