//! Open the selected database's folder, independent of installed/portable mode.
use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::HWND,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    core::{PCWSTR, w},
};

fn folder(database: &Path) -> Result<PathBuf, String> {
    std::path::absolute(database)
        .map_err(|e| format!("Cannot locate data folder: {e}"))?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Cannot locate data folder".into())
}

pub(super) fn open(owner: HWND, database: &Path) -> Result<(), String> {
    let folder = folder(database)?;
    let path: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
    // Pass the folder as the Shell target, without command-line parsing/quoting.
    let result = unsafe {
        ShellExecuteW(
            Some(owner),
            w!("open"),
            PCWSTR(path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    let code = result.0 as isize;
    if code <= 32 {
        Err(format!(
            "Cannot open data folder {} (Windows Shell error {code})",
            folder.display()
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_database_folder_follows_portable_installed_and_explicit_paths() {
        let root = std::env::temp_dir().join(format!("desksteps-folder-{}", std::process::id()));
        let app = root.join("Portable приложение");
        let exe = app.join("desksteps.exe");
        let local = root.join("Local AppData");
        let selected =
            |portable| crate::runtime::database_path(&exe, Some(local.clone()), portable).unwrap();
        assert_eq!(folder(&selected(false)).unwrap(), local.join("DeskSteps"));
        assert_eq!(folder(&selected(true)).unwrap(), app.join("data"));
        // An empty adjacent data directory selects portable mode automatically.
        std::fs::create_dir_all(app.join("data")).unwrap();
        assert_eq!(folder(&selected(false)).unwrap(), app.join("data"));
        assert_eq!(
            folder(&root.join("Custom history/records.sqlite")).unwrap(),
            root.join("Custom history")
        );
        assert_eq!(
            folder(Path::new("synthetic.sqlite")).unwrap(),
            std::env::current_dir().unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
