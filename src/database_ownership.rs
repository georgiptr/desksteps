//! Process-lifetime ownership of the actual database file, including path aliases.
//! This byte is far beyond SQLite's lock range (0x40000000..0x400001ff).
//! A byte-range lock beyond EOF does not change file length or database contents.
use std::{
    fs::{File, OpenOptions},
    os::windows::io::AsRawHandle,
    path::Path,
};
use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx},
    System::IO::OVERLAPPED,
};

#[derive(Debug)]
pub(crate) struct DatabaseOwnership {
    _file: File,
}
impl DatabaseOwnership {
    pub(crate) fn acquire(path: &Path) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| format!("Cannot open database {}: {e}", path.display()))?;
        let mut overlapped = OVERLAPPED::default();
        // Offset 1 TiB. SQLite never locks here; independent handles in this
        // process must still conflict, just as separate processes do.
        overlapped.Anonymous.Anonymous.OffsetHigh = 256;
        unsafe {
            LockFileEx(HANDLE(file.as_raw_handle()), LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                None, 1, 0, &mut overlapped)
        }.map_err(|e| format!("Cannot acquire database ownership for {}. Another DeskSteps app or replay may already be using it. ({e})", path.display()))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_file_aliases_conflict_and_close_releases_without_growing_file() {
        let folder =
            std::env::temp_dir().join(format!("desksteps-ownership-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let db = folder.join("test.sqlite");
        std::fs::write(&db, b"sentinel").unwrap();
        let alias = folder.join("alias.sqlite");
        let _ = std::fs::remove_file(&alias);
        std::fs::hard_link(&db, &alias).unwrap();
        let owner = DatabaseOwnership::acquire(&db).unwrap();
        assert!(DatabaseOwnership::acquire(&db).is_err());
        assert!(DatabaseOwnership::acquire(&alias).is_err());
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "database_ownership::tests::ownership_contender_helper",
            ])
            .env("DESKSTEPS_TEST_OWNED_DATABASE", &alias)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stdout)
        );
        assert_eq!(std::fs::read(&db).unwrap(), b"sentinel");
        let other = folder.join("other.sqlite");
        let independent = DatabaseOwnership::acquire(&other).unwrap();
        drop(owner);
        drop(DatabaseOwnership::acquire(&alias).unwrap());
        drop(independent);
        for file in [db, alias, other] {
            std::fs::remove_file(file).unwrap();
        }
        std::fs::remove_dir(folder).unwrap();
    }
    #[test]
    fn ownership_contender_helper() {
        if let Some(path) = std::env::var_os("DESKSTEPS_TEST_OWNED_DATABASE") {
            let result = crate::storage::Store::open(Path::new(&path), chrono_tz::UTC);
            assert!(matches!(result, Err(e) if e.contains("database ownership")));
        }
    }
}
