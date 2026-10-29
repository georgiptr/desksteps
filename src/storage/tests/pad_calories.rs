use super::*;
use crate::statistics::{Dashboard, Preferences, Request};

fn memory() -> Store {
    Store::initialize(Connection::open_in_memory().unwrap(), chrono_tz::UTC).unwrap()
}
fn at(second: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-23T23:59:00Z")
        .unwrap()
        .with_timezone(&Utc)
        + chrono::Duration::seconds(second)
}
fn sample(steps: u32, calories: Option<u32>) -> Sample {
    Sample {
        steps: Some(steps),
        elapsed_s: Some(steps),
        distance_m: Some(steps),
        speed_kmh: Some(3.0),
        calories_kcal: calories,
        calories_per_hour: Some(100),
        ..Default::default()
    }
}
fn send(s: &mut Store, second: i64, connection: &str, sample: &Sample) -> bool {
    send_device(s, second, connection, "pad", sample)
}
fn send_device(
    s: &mut Store,
    second: i64,
    connection: &str,
    device: &str,
    sample: &Sample,
) -> bool {
    s.record(
        Event {
            key: &format!("{device}:{connection}:{second}"),
            device,
            connection,
            utc: at(second),
            monotonic_ns: Some(second * 1_000_000_000),
            kind: "notification",
            uuid: crate::protocol::TREADMILL,
            raw: &[0x80, 0],
            detail: "synthetic",
        },
        Some(sample),
    )
    .unwrap()
}
fn read(s: &Store, second: i64) -> Dashboard {
    s.dashboard(
        &Request {
            day: at(second).date_naive().to_string(),
            range: 2,
        },
        at(second),
    )
    .unwrap()
}
fn weight(s: &Store, day: &str, weight: Option<f64>) {
    s.save_preferences(&Preferences {
        effective: day.into(),
        weight,
        calories: Some(100.0),
        ..Default::default()
    })
    .unwrap();
}

#[test]
fn reported_readings_survive_weight_changes_while_displayed_sources_diverge() {
    let mut s = memory();
    send(&mut s, 0, "a", &sample(0, Some(0)));
    send(&mut s, 1, "a", &sample(2, Some(10)));
    send(&mut s, 2, "a", &sample(4, Some(12)));
    let d = read(&s, 2);
    assert_eq!(
        (d.day.displayed_kcal(), d.day.kcal, d.day.calorie_label()),
        (Some(12.0), None, "pad kcal")
    );
    assert_eq!(d.sessions[0].pad_calories.total, Some(12.0));
    assert!(!d.day.pad_calories.incomplete);
    weight(&s, "2026-10-23", Some(80.0));
    let d = read(&s, 2);
    assert!((d.day.displayed_kcal().unwrap() - 0.084).abs() < 1e-9);
    assert_eq!(d.day.calorie_label(), "active kcal");
    assert_eq!(d.day.pad_calories.total, Some(12.0));
    assert_eq!(d.day.kcal, d.sessions[0].kcal);
    let raw: String = s
        .conn
        .query_row(
            "SELECT decoded FROM samples ORDER BY event_id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let decoded: Sample = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        (decoded.calories_kcal, decoded.calories_per_hour),
        (Some(12), Some(100))
    );
    assert_eq!(
        s.conn
            .query_row("SELECT raw FROM events ORDER BY id DESC LIMIT 1", [], |r| r
                .get::<_, Vec<u8>>(0))
            .unwrap(),
        [0x80, 0]
    );
    weight(&s, "2026-10-23", None);
    assert_eq!(read(&s, 2).day.displayed_kcal(), Some(12.0));
    // Missing app inputs cannot silently select the pad when weight is present.
    s.conn
        .execute("UPDATE ui_intervals SET active_ms=0,factor=0", [])
        .unwrap();
    weight(&s, "2026-10-23", Some(80.0));
    assert_eq!(read(&s, 2).day.displayed_kcal(), None);
}

#[test]
fn cumulative_plateaus_and_calorie_ticks_without_step_ticks_are_counted_once() {
    let mut s = memory();
    for (t, steps, kcal) in [(0, 0, 0), (1, 2, 1), (2, 2, 2), (3, 4, 2), (4, 4, 2)] {
        send(&mut s, t, "a", &sample(steps, Some(kcal)));
    }
    let d = read(&s, 4);
    assert_eq!(d.day.displayed_kcal(), Some(2.0));
    assert_eq!(d.sessions[0].pad_calories.total, Some(2.0));
    assert!(!send(&mut s, 4, "a", &sample(4, Some(2))));
    assert_eq!(read(&s, 4).day.displayed_kcal(), Some(2.0));
    s.expire_sessions(at(400)).unwrap();
    assert_eq!(read(&s, 4).day.displayed_kcal(), Some(2.0));
}

#[test]
fn initial_unknowns_missing_fields_and_resets_are_partial_without_invented_credit() {
    let mut s = memory();
    send(&mut s, 0, "a", &sample(100, Some(50)));
    assert_eq!(read(&s, 0).day.displayed_kcal(), None);
    send(&mut s, 1, "a", &sample(102, Some(52)));
    send(&mut s, 2, "a", &sample(104, None));
    send(&mut s, 3, "a", &sample(106, Some(60)));
    send(&mut s, 4, "a", &sample(108, Some(61)));
    // Calorie-only reset, followed by a full workout-counter reset.
    send(&mut s, 5, "a", &sample(110, Some(1)));
    send(&mut s, 6, "a", &sample(0, Some(0)));
    send(&mut s, 7, "a", &sample(2, Some(2)));
    let d = read(&s, 7);
    assert_eq!(d.day.pad_calories.total, Some(5.0));
    assert!(d.day.pad_calories.incomplete);
    assert_eq!(d.sessions[0].pad_calories.total, Some(5.0));
    assert!(d.sessions[0].pad_calories.incomplete);
    // A missing-only history has no calorie value, rather than a fabricated zero.
    let mut empty = memory();
    send(&mut empty, 0, "a", &sample(0, None));
    send(&mut empty, 1, "a", &sample(2, None));
    assert_eq!(read(&empty, 1).day.displayed_kcal(), None);
}

#[test]
fn gap_recovery_uses_saved_decisions_and_midnight_credits_the_receipt_date() {
    let mut s = memory();
    send(&mut s, 0, "a", &sample(0, Some(0)));
    send(&mut s, 1, "a", &sample(2, Some(1)));
    send(&mut s, 61, "b", &sample(62, Some(11)));
    let d = read(&s, 61);
    assert_eq!(d.days[0].pad_calories.total, Some(1.0));
    assert_eq!(d.day.displayed_kcal(), Some(10.0));
    assert_eq!(d.day.pad_calories.recovered, Some(10.0));
    assert_eq!(d.sessions[0].pad_calories.total, Some(10.0));
    weight(&s, "2026-10-24", Some(80.0));
    let d = read(&s, 61);
    assert_eq!(d.days[0].displayed_kcal(), Some(1.0));
    assert!(d.day.displayed_kcal().unwrap() > 0.0);
    assert_ne!(d.day.displayed_kcal(), Some(10.0));
    s.set_setting("recover_gaps", "false").unwrap();
    send(&mut s, 81, "c", &sample(82, Some(20)));
    send(&mut s, 82, "c", &sample(84, Some(21)));
    assert_eq!(read(&s, 82).day.pad_calories.total, Some(11.0));
    assert!(read(&s, 82).day.pad_calories.incomplete);
    // Backfill is driven by the original recovery rows, even with recovery now off.
    let before = database_snapshot(&s);
    s.conn.execute("DELETE FROM pad_calories", []).unwrap();
    s.backfill_pad_calories().unwrap();
    assert_eq!(database_snapshot(&s), before);
    s.backfill_pad_calories().unwrap();
    assert_eq!(database_snapshot(&s), before);
}

#[test]
fn recovery_with_missing_pad_calories_is_marked_unavailable() {
    let mut s = memory();
    send(&mut s, 0, "a", &sample(0, None));
    send(&mut s, 10, "b", &sample(10, Some(10)));
    let d = read(&s, 10);
    assert_eq!(d.day.pad_calories.total, None);
    assert!(d.day.pad_calories.recovery_missing);
    assert!(d.sessions[0].pad_calories.recovery_missing);
}

#[test]
fn out_of_order_and_monotonic_reversals_cannot_rewind_the_pad_baseline() {
    let mut s = memory();
    send(&mut s, 0, "a", &sample(0, Some(0)));
    send(&mut s, 2, "a", &sample(4, Some(2)));
    send(&mut s, 1, "a", &sample(2, Some(99)));
    send(&mut s, 3, "a", &sample(6, Some(3)));
    assert_eq!(read(&s, 3).day.pad_calories.total, Some(3.0));
    // Missing steps must not hide an out-of-order calorie-only notification.
    let mut reversed = sample(0, Some(99));
    reversed.steps = None;
    send(&mut s, 1, "a-missing", &reversed);
    send(&mut s, 4, "a", &sample(8, Some(4)));
    assert_eq!(read(&s, 4).day.pad_calories.total, Some(4.0));
    // Ordered wall time with an impossible monotonic gap must not be observed.
    s.record(
        Event {
            key: "mono",
            device: "pad",
            connection: "a",
            utc: at(5),
            monotonic_ns: Some(20_000_000_000),
            kind: "notification",
            uuid: "",
            raw: &[],
            detail: "synthetic",
        },
        Some(&sample(10, Some(20))),
    )
    .unwrap();
    assert_eq!(read(&s, 5).day.pad_calories.total, Some(4.0));
}

#[test]
fn device_baselines_are_independent_and_empty_belt_diagnostics_are_excluded() {
    let mut s = memory();
    send_device(&mut s, 0, "a", "one", &sample(0, Some(0)));
    send_device(&mut s, 0, "b", "two", &sample(0, Some(0)));
    send_device(&mut s, 1, "a", "one", &sample(2, Some(2)));
    send_device(&mut s, 1, "b", "two", &sample(2, Some(5)));
    assert_eq!(read(&s, 1).day.pad_calories.total, Some(7.0));
    s.record(
        Event {
            key: "diagnostic",
            device: "one",
            connection: "diag:1",
            utc: at(2),
            monotonic_ns: None,
            kind: "recorder_started",
            uuid: "",
            raw: &[],
            detail: "empty-belt",
        },
        None,
    )
    .unwrap();
    send_device(&mut s, 3, "diag:1", "one", &sample(100, Some(100)));
    send_device(&mut s, 4, "diag:1", "one", &sample(102, Some(110)));
    assert_eq!(read(&s, 4).day.pad_calories.total, Some(7.0));
    send_device(&mut s, 5, "fresh:1", "one", &sample(104, Some(112)));
    assert_eq!(read(&s, 5).day.pad_calories.total, Some(7.0));
    send_device(&mut s, 6, "fresh:1", "one", &sample(106, Some(114)));
    assert_eq!(read(&s, 6).day.pad_calories.total, Some(9.0));
}

#[test]
fn real_reopen_backfills_older_samples_preserves_evidence_and_survives_restart() {
    let folder = std::env::temp_dir().join(format!(
        "desksteps-calories-{}",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let path = folder.join("synthetic.sqlite");
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        send(&mut s, 0, "a", &sample(0, Some(0)));
        send(&mut s, 1, "a", &sample(2, Some(3)));
        s.conn.execute("DELETE FROM pad_calories", []).unwrap();
    }
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        assert_eq!(read(&s, 1).day.displayed_kcal(), Some(3.0));
        send(&mut s, 11, "b", &sample(12, Some(8)));
        assert_eq!(read(&s, 11).day.displayed_kcal(), Some(8.0));
        assert_eq!(read(&s, 11).day.pad_calories.recovered, Some(5.0));
        assert_eq!(
            s.conn
                .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert!(
            !s.conn
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
    }
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn failed_backfill_and_calorie_write_leave_all_raw_and_derived_data_unchanged() {
    let mut s = memory();
    send(&mut s, 0, "a", &sample(0, Some(0)));
    s.conn.execute("DELETE FROM pad_calories", []).unwrap();
    s.conn.execute_batch("CREATE TRIGGER fail_calorie BEFORE INSERT ON pad_calories BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    let before = database_snapshot(&s);
    assert!(s.backfill_pad_calories().is_err());
    assert_eq!(database_snapshot(&s), before);
    s.conn
        .execute_batch("DROP TRIGGER fail_calorie; UPDATE samples SET decoded='invalid json'")
        .unwrap();
    assert!(s.backfill_pad_calories().is_err());
    s.conn
        .execute(
            "UPDATE samples SET decoded=?1",
            [serde_json::to_string(&sample(0, Some(0))).unwrap()],
        )
        .unwrap();
    s.conn
        .execute("UPDATE events SET utc='invalid time'", [])
        .unwrap();
    // A later sample exposes the corrupt baseline and rolls back its entire recording.
    let before = database_snapshot(&s);
    let result = s.record(
        Event {
            key: "bad-time",
            device: "pad",
            connection: "a",
            utc: at(1),
            monotonic_ns: None,
            kind: "notification",
            uuid: "",
            raw: &[],
            detail: "synthetic",
        },
        Some(&sample(2, Some(2))),
    );
    assert!(result.is_err());
    assert_eq!(database_snapshot(&s), before);
}
