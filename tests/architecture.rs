//! Source dependency guards. These checks never instantiate Windows/Bluetooth.
use std::{
    fs,
    path::{Path, PathBuf},
};
fn sources(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .is_some_and(|n| n == "tests" || n == "tests.rs")
        {
            continue;
        }
        if path.is_dir() {
            files.extend(sources(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}
fn production(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap()
        .split("#[cfg(test)]")
        .next()
        .unwrap()
        .to_string()
}
#[test]
fn domain_logic_cannot_depend_on_sql_windows_application_or_ui() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = sources(&root.join("statistics"));
    files.extend(
        [
            "accounting.rs",
            "protocol.rs",
            "control.rs",
            "device.rs",
            "event.rs",
        ]
        .map(|p| root.join(p)),
    );
    for file in files {
        let source = production(&file);
        for forbidden in [
            "rusqlite",
            "windows::",
            "windows_future",
            "crate::storage",
            "crate::application",
            "crate::ui",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} depends on {forbidden}",
                file.display()
            );
        }
    }
}
#[test]
fn application_and_storage_do_not_call_the_ui() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for area in ["application", "storage"] {
        for file in sources(&root.join(area)) {
            let source = production(&file);
            assert!(!source.contains("crate::ui"), "{} calls UI", file.display());
        }
    }
}
#[test]
fn ui_uses_application_requests_instead_of_opening_databases_or_bluetooth() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    for file in sources(&root) {
        let source = production(&file);
        for forbidden in [
            "rusqlite",
            "crate::storage",
            "Devices::Bluetooth",
            "Arc<Mutex<View>>",
            "view.lock()",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} bypasses application boundary: {forbidden}",
                file.display()
            );
        }
    }
}
