//! Installed/portable storage and persisted timezone selection. No legacy import.
use std::path::{Path, PathBuf};

/// Executable-relative storage, shared by Cargo debug/release development launches.
/// It cannot be redirected to personal history by a command-line override.
pub(crate) fn development_database_path(executable: &Path) -> Result<PathBuf, String> {
    let folder = executable
        .parent()
        .ok_or("Cannot locate executable directory")?;
    // Cargo profiles share synthetic data even in a relocated/custom target tree.
    // Packaged binaries instead keep it separate beside their executable.
    let base = if folder
        .file_name()
        .is_some_and(|name| name == "debug" || name == "release")
    {
        folder
            .parent()
            .ok_or("Cannot locate Cargo output directory")?
    } else {
        folder
    };
    Ok(base.join("development/desksteps.sqlite"))
}

pub fn database_path(
    executable: &Path,
    local: Option<PathBuf>,
    portable: bool,
) -> Result<PathBuf, String> {
    let folder = executable
        .parent()
        .ok_or("Cannot locate executable directory")?;
    if portable || folder.join("data").is_dir() {
        Ok(folder.join("data").join("desksteps.sqlite"))
    } else {
        Ok(local
            .ok_or("Windows LocalAppData is unavailable")?
            .join("DeskSteps")
            .join("desksteps.sqlite"))
    }
}

pub fn saved_timezone(database: &Path) -> Result<Option<chrono_tz::Tz>, String> {
    saved_setting(database, "timezone")?
        .map(|name| {
            name.parse()
                .map_err(|_| "Database timezone is invalid".into())
        })
        .transpose()
}

pub(crate) fn window_behavior(
    database: &Path,
) -> Result<crate::window_behavior::WindowBehavior, String> {
    Ok(crate::window_behavior::WindowBehavior {
        start_minimized: saved_setting(database, "start_minimized")?.as_deref() != Some("false"),
        minimize_to_tray: saved_setting(database, "minimize_to_tray")?.as_deref() == Some("true"),
        keep_running_on_close: saved_setting(database, "keep_running_on_close")?.as_deref()
            != Some("false"),
    })
}

fn saved_setting(database: &Path, key: &str) -> Result<Option<String>, String> {
    if !database.is_file() {
        return Ok(None);
    }
    let conn =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
    use rusqlite::OptionalExtension;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='settings')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Ok(None);
    }
    conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .optional()
    .map_err(|e| e.to_string())
}

pub fn detected_timezone() -> Result<chrono_tz::Tz, String> {
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED).map_err(|e| e.to_string())?;
    }
    let result = windows::Globalization::Calendar::new()
        .and_then(|c| c.GetTimeZone())
        .map_err(|e| e.to_string())
        .and_then(|name| {
            name.to_string().parse().map_err(|_| {
                "Cannot map Windows timezone; specify --timezone with an IANA name".into()
            })
        });
    unsafe {
        RoUninitialize();
    }
    result
}

pub fn check_writable(database: &Path) -> Result<(), String> {
    let folder = database
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("Cannot create data folder {}: {e}", folder.display()))?;
    let probe = folder.join(format!(
        ".desksteps-write-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e| format!("Data folder is not writable: {} ({e}). Move the portable folder to a writable location.",folder.display()))?;
    drop(file);
    std::fs::remove_file(probe).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn development_storage_follows_relocated_binaries_and_keeps_profiles_together() {
        for profile in ["debug", "release"] {
            let exe = PathBuf::from("relocated-output")
                .join(profile)
                .join("desksteps.exe");
            assert_eq!(
                development_database_path(&exe).unwrap(),
                Path::new("relocated-output/development/desksteps.sqlite")
            );
        }
        assert_eq!(
            development_database_path(Path::new("portable/desksteps.exe")).unwrap(),
            Path::new("portable/development/desksteps.sqlite")
        );
        assert!(development_database_path(Path::new("/")).is_err());
    }
    #[test]
    fn storage_is_relative_to_executable_and_never_uses_legacy_data() {
        let exe = Path::new("C:/apps/DeskSteps/desksteps.exe");
        assert_eq!(
            database_path(exe, None, true).unwrap(),
            Path::new("C:/apps/DeskSteps/data/desksteps.sqlite")
        );
        assert_eq!(
            database_path(exe, Some(PathBuf::from("C:/user/local")), false).unwrap(),
            Path::new("C:/user/local/DeskSteps/desksteps.sqlite")
        );
        assert!(database_path(exe, None, false).is_err());
        assert!(database_path(Path::new("/"), None, true).is_err());
    }
    #[test]
    fn adjacent_data_directory_timezone_and_writable_checks_use_temporary_data() {
        let root = std::env::temp_dir().join(format!("desksteps-paths-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("desksteps.exe");
        std::fs::create_dir_all(root.join("data")).unwrap();
        let db = database_path(&exe, None, false).unwrap();
        assert_eq!(db, root.join("data/desksteps.sqlite"));
        assert_eq!(saved_timezone(&db).unwrap(), None);
        check_writable(&db).unwrap();
        let store = crate::storage::Store::open(&db, chrono_tz::Europe::Helsinki).unwrap();
        drop(store);
        assert_eq!(
            saved_timezone(&db).unwrap(),
            Some(chrono_tz::Europe::Helsinki)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn only_a_data_directory_automatically_selects_portable_mode() {
        let root = std::env::temp_dir().join(format!("desksteps-detection-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("desksteps.exe");
        let local = root.join("local");
        let installed = local.join("DeskSteps/desksteps.sqlite");
        let selected = || database_path(&exe, Some(local.clone()), false).unwrap();
        assert_eq!(selected(), installed);
        std::fs::write(root.join("data"), b"not a directory").unwrap();
        assert_eq!(selected(), installed);
        std::fs::remove_file(root.join("data")).unwrap();
        assert_eq!(
            database_path(&exe, None, true).unwrap(),
            root.join("data/desksteps.sqlite")
        );
        std::fs::create_dir(root.join("data")).unwrap();
        assert_eq!(selected(), root.join("data/desksteps.sqlite"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
