//! Native startup checks use only a fresh synthetic database, with Bluetooth off.
#[test]
fn native_ui_first_launch_opens_main_and_saved_startup_options_choose_taskbar_or_tray() {
    let folder = std::env::temp_dir().join(format!(
        "desksteps-first-launch-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    let database = folder.join("synthetic.sqlite");
    for (first, settings, opened, minimized, taskbar) in [
        (true, None, true, false, true),
        (false, None, false, true, true),
        (false, Some((false, false)), true, false, true),
        (false, Some((true, true)), false, false, false),
        (false, Some((true, false)), false, true, true),
    ] {
        if let Some((start_minimized, minimize_to_tray)) = settings {
            let conn = rusqlite::Connection::open(&database).unwrap();
            for (key, value) in [
                ("start_minimized", start_minimized),
                ("minimize_to_tray", minimize_to_tray),
            ] {
                conn.execute("INSERT INTO settings VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [key, if value { "true" } else { "false" }]).unwrap();
            }
        }
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_desksteps"))
            .args([
                "--no-bluetooth",
                "--no-controls",
                "--timezone",
                "UTC",
                "--smoke-seconds",
                "1",
                "--database",
            ])
            .arg(&database)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(database.with_extension("smoke-report.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(report["first_run"], first);
        assert_eq!(report["opened_on_launch"], opened);
        assert_eq!(report["minimized_on_launch"], minimized);
        assert_eq!(report["taskbar_window_on_launch"], taskbar);
        assert_eq!(report["taskbar_windows_checked"], true);
        assert_eq!(report["clean_exit"], true);
        assert_eq!(report["bluetooth_disabled"], true);
    }
    std::fs::remove_dir_all(folder).unwrap();
}
