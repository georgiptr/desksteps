use super::*;
#[path = "tests/pad_calories.rs"]
mod calorie_tests;
#[test]
fn device_names_survive_reopen_and_stay_associated_with_their_address() {
    let folder = std::env::temp_dir().join(format!(
        "desksteps-device-names-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let path = folder.join("synthetic.sqlite");
    {
        let mut store = Store::open(&path, chrono_tz::UTC).unwrap();
        // An existing address-only setting needs no migration.
        store.set_setting("device", &crate::address(1)).unwrap();
        assert_eq!(store.device_name(1).unwrap(), None);
        store.save_device(1, Some("Synthetic Pad A")).unwrap();
        store.save_device(2, Some("Synthetic Pad B")).unwrap();
    }
    {
        let mut store = Store::open(&path, chrono_tz::UTC).unwrap();
        assert_eq!(store.setting("device").unwrap(), Some(crate::address(2)));
        assert_eq!(
            store.device_name(1).unwrap().as_deref(),
            Some("Synthetic Pad A")
        );
        assert_eq!(
            store.device_name(2).unwrap().as_deref(),
            Some("Synthetic Pad B")
        );
        for name in [None, Some(""), Some("  ")] {
            store.save_device(1, name).unwrap();
            assert_eq!(
                store.device_name(1).unwrap().as_deref(),
                Some("Synthetic Pad A")
            );
        }
        store.save_device(3, None).unwrap();
        assert_eq!(store.device_name(3).unwrap(), None);
    }
    std::fs::remove_dir_all(folder).unwrap();
}
#[test]
fn failed_device_name_write_rolls_back_the_selected_address() {
    let mut store = memory();
    store.save_device(1, Some("Synthetic A")).unwrap();
    store.conn.execute_batch("CREATE TRIGGER fail_name BEFORE INSERT ON settings WHEN NEW.key LIKE 'device_name:%' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(store.save_device(2, Some("Synthetic B")).is_err());
    assert_eq!(store.setting("device").unwrap(), Some(crate::address(1)));
    assert_eq!(store.device_name(2).unwrap(), None);
}
#[path = "tests/rebuild.rs"]
mod rebuild;
#[path = "tests/recovery.rs"]
mod recovery;
#[path = "tests/restart.rs"]
mod restart;
fn database_snapshot(store: &Store) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    [
        "settings",
        "events",
        "samples",
        "anchors",
        "ui_preferences",
        "ui_sessions",
        "ui_anchors",
        "ui_runs",
        "ui_processed",
        "ui_intervals",
        "recovery_baselines",
        "recovery_credits",
        "pad_calories",
    ]
    .iter()
    .map(|table| {
        let mut statement = store
            .conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| {
                (0..columns)
                    .map(|column| row.get(column))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    })
    .collect()
}

#[test]
fn every_projection_write_failure_rolls_back_the_complete_recording_transaction() {
    for (table, operation) in [
        ("ui_sessions", "INSERT"),
        ("ui_intervals", "INSERT"),
        ("ui_sessions", "UPDATE"),
        ("ui_anchors", "UPDATE"),
        ("ui_processed", "INSERT"),
        ("pad_calories", "INSERT"),
    ] {
        let mut store = memory();
        let baseline = Sample {
            steps: Some(0),
            speed_kmh: Some(3.0),
            elapsed_s: Some(0),
            ..Default::default()
        };
        let mut initial = event("baseline", Some(0));
        initial.utc -= chrono::Duration::seconds(1);
        store.record(initial, Some(&baseline)).unwrap();
        let before = database_snapshot(&store);
        store.conn.execute_batch(&format!("CREATE TRIGGER projection_failure BEFORE {operation} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;")).unwrap();
        let sample = Sample {
            steps: Some(2),
            speed_kmh: Some(3.0),
            elapsed_s: Some(1),
            ..Default::default()
        };
        assert!(
            store
                .record(event("next", Some(1_000_000_000)), Some(&sample))
                .is_err(),
            "{table}/{operation}"
        );
        assert_eq!(
            database_snapshot(&store),
            before,
            "{table}/{operation} leaked partial writes"
        );
        store
            .conn
            .execute_batch("DROP TRIGGER projection_failure")
            .unwrap();
        assert!(
            store
                .record(event("next", Some(1_000_000_000)), Some(&sample))
                .unwrap()
        );
        assert!(
            !store
                .record(event("next", Some(1_000_000_000)), Some(&sample))
                .unwrap()
        );
        assert_eq!(store.totals(event("", None).utc).unwrap().recorded_steps, 2);
    }
}

#[test]
fn invalid_session_anchor_rolls_back_raw_evidence_and_can_be_repaired_without_duplicates() {
    let mut store = memory();
    store
        .conn
        .execute(
            "INSERT INTO ui_anchors VALUES('test-pad','invalid json')",
            [],
        )
        .unwrap();
    let before = database_snapshot(&store);
    assert!(store.record(event("bad-anchor", Some(1)), None).is_err());
    assert_eq!(database_snapshot(&store), before);
    store.conn.execute("DELETE FROM ui_anchors", []).unwrap();
    assert!(store.record(event("bad-anchor", Some(1)), None).unwrap());
}

#[test]
fn session_expiration_failure_preserves_both_session_and_restart_anchor() {
    let mut store = memory();
    let base = event("", None).utc;
    for (second, steps, speed) in [(0, 0, 3.0), (1, 1, 3.0), (2, 1, 0.0)] {
        let mut input = event("", Some(second * 1_000_000_000));
        let key = format!("sample-{second}");
        input.key = &key;
        input.utc = base + chrono::Duration::seconds(second);
        store
            .record(
                input,
                Some(&Sample {
                    steps: Some(steps),
                    speed_kmh: Some(speed),
                    ..Default::default()
                }),
            )
            .unwrap();
    }
    let before = database_snapshot(&store);
    store.conn.execute_batch("CREATE TRIGGER expire_failure BEFORE UPDATE ON ui_sessions BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(
        store
            .expire_sessions(base + chrono::Duration::seconds(302))
            .is_err()
    );
    assert_eq!(database_snapshot(&store), before);
    store
        .conn
        .execute_batch("DROP TRIGGER expire_failure")
        .unwrap();
    store
        .expire_sessions(base + chrono::Duration::seconds(302))
        .unwrap();
    assert_eq!(
        store
            .conn
            .query_row("SELECT state FROM ui_sessions", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "Finished"
    );
}
fn event<'a>(key: &'a str, mono: Option<i64>) -> Event<'a> {
    Event {
        key,
        device: "test-pad",
        connection: "a",
        utc: DateTime::parse_from_rfc3339("2026-10-09T09:00:04Z")
            .unwrap()
            .with_timezone(&Utc),
        monotonic_ns: mono,
        kind: "notification",
        uuid: "test",
        raw: &[1, 2],
        detail: "fixture",
    }
}
#[test]
fn settings_upsert_and_database_version_guards() {
    let s = memory();
    assert_eq!(s.setting("missing").unwrap(), None);
    s.set_setting("device", "first").unwrap();
    s.set_setting("device", "second").unwrap();
    assert_eq!(s.setting("device").unwrap().as_deref(), Some("second"));
    let future = Connection::open_in_memory().unwrap();
    future.pragma_update(None, "user_version", 2).unwrap();
    assert!(
        Store::initialize(future, chrono_tz::UTC)
            .err()
            .unwrap()
            .contains("newer")
    );
    let readonly = Connection::open_in_memory().unwrap();
    readonly.pragma_update(None, "query_only", true).unwrap();
    assert!(Store::initialize(readonly, chrono_tz::UTC).is_err());
    s.conn.execute_batch("DROP TABLE settings").unwrap();
    assert!(s.setting("device").is_err());
    assert!(s.set_setting("device", "third").is_err());
}
#[test]
fn failed_sample_insert_rolls_back_raw_event_and_anchor_together() {
    let mut s = memory();
    s.conn.execute_batch("CREATE TRIGGER fail_sample BEFORE INSERT ON samples BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    let sample = Sample {
        steps: Some(9),
        ..Default::default()
    };
    assert!(
        s.record(event("failed", Some(4_000_000_000)), Some(&sample))
            .is_err()
    );
    for table in ["events", "samples", "anchors"] {
        assert_eq!(
            s.conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    s.conn.execute_batch("DROP TRIGGER fail_sample").unwrap();
    assert!(
        s.record(event("failed", Some(4_000_000_000)), Some(&sample))
            .unwrap()
    );
    assert!(
        !s.record(event("failed", Some(4_000_000_000)), Some(&sample))
            .unwrap()
    );
}
#[test]
fn database_faults_are_errors_and_never_partial_credit() {
    let mut s = memory();
    put(&mut s, 0, "a", "2026-10-09T09:00:00Z", Some(0), 0);
    s.conn
        .execute("UPDATE anchors SET utc='corrupt timestamp'", [])
        .unwrap();
    let sample = Sample {
        steps: Some(4),
        ..Default::default()
    };
    assert!(
        s.record(event("corrupt", Some(4_000_000_000)), Some(&sample))
            .is_err()
    );
    s.conn.execute_batch("DROP TABLE anchors").unwrap();
    assert!(
        s.record(event("missing-table", None), Some(&sample))
            .is_err()
    );
    s.conn.pragma_update(None, "query_only", true).unwrap();
    assert!(s.record(event("readonly", None), None).is_err());
    s.conn.pragma_update(None, "query_only", false).unwrap();
    s.conn.execute_batch("DROP TABLE samples").unwrap();
    assert!(s.totals(Utc::now()).is_err());
    assert_eq!(
        s.conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn monotonic_reversal_cannot_rewind_and_missing_monotonic_uses_wall_time() {
    let mut s = memory();
    put(&mut s, 2, "a", "2026-10-09T09:00:02Z", Some(2), 2);
    let sample = Sample {
        steps: Some(9),
        elapsed_s: Some(4),
        ..Default::default()
    };
    s.record(event("reversed", Some(1_000_000_000)), Some(&sample))
        .unwrap();
    assert_eq!(
        s.conn
            .query_row("SELECT steps FROM anchors", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    s.record(event("fallback", None), Some(&sample)).unwrap();
    assert_eq!(
        s.totals(
            DateTime::parse_from_rfc3339("2026-10-09T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        )
        .unwrap()
        .recorded_steps,
        7
    );
}
#[test]
fn elapsed_reset_long_gap_and_counter_wrap_are_withheld_without_rollover_assumptions() {
    let mut s = memory();
    put(&mut s, 0, "a", "2026-10-09T09:00:00Z", Some(100), 100);
    put(&mut s, 1, "a", "2026-10-09T09:00:01Z", Some(100), 1);
    put(&mut s, 2, "a", "2026-10-09T09:00:09Z", Some(105), 9);
    put(&mut s, 3, "a", "2026-10-09T09:00:10Z", Some(0xffffff), 10);
    put(&mut s, 4, "a", "2026-10-09T09:00:11Z", Some(3), 11);
    let qualities: Vec<String> = s
        .conn
        .prepare("SELECT quality FROM samples ORDER BY event_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        qualities,
        [
            "initial_baseline",
            "counter_reset",
            "collection_gap",
            "observed",
            "counter_reset"
        ]
    );
    assert!(s.record(event("raw", None), None).unwrap());
}
fn memory() -> Store {
    let store = Store::initialize(
        Connection::open_in_memory().unwrap(),
        chrono_tz::Europe::Helsinki,
    )
    .unwrap();
    store.set_setting("recover_gaps", "false").unwrap();
    store
}
fn put(
    store: &mut Store,
    id: u32,
    conn: &str,
    utc: &str,
    steps: Option<u32>,
    elapsed: u32,
) -> bool {
    let utc = DateTime::parse_from_rfc3339(utc)
        .unwrap()
        .with_timezone(&Utc);
    store
        .record(
            Event {
                key: &id.to_string(),
                device: "test-pad",
                connection: conn,
                utc,
                monotonic_ns: Some(id as i64 * 1_000_000_000),
                kind: "notification",
                uuid: "test",
                raw: &[],
                detail: "",
            },
            Some(&Sample {
                steps,
                elapsed_s: Some(elapsed),
                ..Default::default()
            }),
        )
        .unwrap()
}
#[test]
fn pause_reset_and_duplicates_preserve_progress() {
    let mut s = memory();
    let base = "2026-10-09T09:00:";
    for (i, steps) in [(0, 0), (1, 4), (2, 4), (3, 8), (4, 0), (5, 3)] {
        put(
            &mut s,
            i,
            "a",
            &format!("{base}{i:02}Z"),
            Some(steps),
            if i < 4 { i } else { i - 4 },
        );
    }
    assert!(!put(&mut s, 5, "a", "2026-10-09T09:00:05Z", Some(3), 1));
    assert_eq!(
        s.totals(
            DateTime::parse_from_rfc3339("2026-10-09T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        )
        .unwrap()
        .recorded_steps,
        11
    );
}
#[test]
fn reconnect_keeps_gap_uncertain_and_does_not_recredit_baseline() {
    let mut s = memory();
    put(&mut s, 0, "a", "2026-10-09T09:00:00Z", Some(100), 100);
    put(&mut s, 1, "a", "2026-10-09T09:00:01Z", Some(104), 101);
    put(&mut s, 2, "b", "2026-10-09T09:03:00Z", Some(120), 280);
    put(&mut s, 3, "b", "2026-10-09T09:03:01Z", Some(124), 281);
    put(&mut s, 4, "c", "2026-10-09T09:03:02Z", Some(124), 281);
    let total = s
        .totals(
            DateTime::parse_from_rfc3339("2026-10-09T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
    assert_eq!(total.recorded_steps, 8);
    assert_eq!(total.uncertain_events, 2);
}
#[test]
fn midnight_and_missing_fields_are_not_silently_allocated() {
    let mut s = memory();
    put(&mut s, 0, "a", "2026-10-09T20:59:59Z", Some(0), 0);
    put(&mut s, 1, "a", "2026-10-09T21:00:00Z", Some(2), 1);
    put(&mut s, 2, "a", "2026-10-09T21:00:01Z", None, 2);
    put(&mut s, 3, "a", "2026-10-09T21:00:02Z", Some(4), 3);
    let total = s
        .totals(
            DateTime::parse_from_rfc3339("2026-10-10T01:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
    assert_eq!(total.day, "2026-10-10");
    assert_eq!(total.recorded_steps, 2);
    assert_eq!(total.uncertain_events, 2);
}
#[test]
fn old_replay_event_cannot_rewind_anchor() {
    let mut s = memory();
    put(&mut s, 0, "a", "2026-10-09T09:00:00Z", Some(0), 0);
    put(&mut s, 1, "a", "2026-10-09T09:00:01Z", Some(2), 1);
    put(&mut s, 2, "a", "2026-10-08T09:00:00Z", Some(99), 99);
    put(&mut s, 3, "a", "2026-10-09T09:00:03Z", Some(4), 3);
    assert_eq!(
        s.totals(
            DateTime::parse_from_rfc3339("2026-10-09T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        )
        .unwrap()
        .recorded_steps,
        4
    );
}

#[test]
fn persisted_anchor_survives_reopening_database() {
    let folder = std::env::temp_dir().join(format!(
        "desksteps-store-test-{}",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let path = folder.join("test.sqlite");
    let zone = chrono_tz::Europe::Helsinki;
    {
        let mut s = Store::open(&path, zone).unwrap();
        put(
            &mut s,
            0,
            "first-process",
            "2026-10-09T09:00:00Z",
            Some(0),
            0,
        );
        put(
            &mut s,
            1,
            "first-process",
            "2026-10-09T09:00:01Z",
            Some(4),
            1,
        );
    }
    {
        let mut s = Store::open(&path, zone).unwrap();
        put(
            &mut s,
            2,
            "second-process",
            "2026-10-09T09:00:02Z",
            Some(4),
            1,
        );
        put(
            &mut s,
            3,
            "second-process",
            "2026-10-09T09:00:03Z",
            Some(7),
            2,
        );
        assert_eq!(
            s.totals(
                DateTime::parse_from_rfc3339("2026-10-09T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
            .unwrap()
            .recorded_steps,
            7
        );
    }
    assert!(Store::open(&path, chrono_tz::UTC).is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&folder).unwrap();
}
