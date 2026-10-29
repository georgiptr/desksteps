use super::*;
#[path = "tests/sessions.rs"]
mod sessions;
use crate::storage::schema::initialize_statistics as initialize;
use crate::{
    protocol::Sample,
    storage::{Event, Store},
};
use rusqlite::Connection;
fn store() -> Store {
    let store = Store::initialize(
        Connection::open_in_memory().unwrap(),
        chrono_tz::Europe::Helsinki,
    )
    .unwrap();
    store.set_setting("recover_gaps", "false").unwrap();
    store
}
fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-13T09:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
        + chrono::Duration::seconds(seconds)
}
fn put(s: &mut Store, second: i64, steps: u32, speed: f64, connection: &str) {
    let sample = Sample {
        steps: Some(steps),
        speed_kmh: Some(speed),
        ..Default::default()
    };
    s.record(
        Event {
            key: &format!("{connection}:{second}"),
            device: "pad",
            connection,
            utc: at(second),
            monotonic_ns: Some(second * 1_000_000_000),
            kind: "notification",
            uuid: "",
            raw: &[],
            detail: "",
        },
        Some(&sample),
    )
    .unwrap();
}
fn read(s: &Store, seconds: i64) -> Dashboard {
    s.dashboard(
        &Request {
            day: "2026-10-13".into(),
            range: 7,
        },
        at(seconds),
    )
    .unwrap()
}
#[test]
fn estimates_match_independent_reference_and_progress_is_not_capped() {
    assert_eq!(met(3.0), Some(2.8));
    assert_eq!(met(1.0), Some(2.1));
    for speed in [f64::NAN, -1.0, 0.0, 9.0] {
        assert_eq!(met(speed), None);
    }
    for speed in [1.7, 2.0, 3.3, 4.1, 4.9, 5.7, 6.5, 7.3, 8.1] {
        assert!(met(speed).unwrap() > 1.0);
    }
    assert_eq!(progress(120.0, Some(100.0)), Some(120.0));
    assert_eq!(progress(10.0, None), None);
    assert_eq!(progress(10.0, Some(0.0)), None);
    // 80 kg, 30 min, 2.8 MET: gross 117.6 kcal less 42 kcal rest = 75.6.
    assert!((((2.8 - 1.0) * 3.5 / 200.0 * 30.0 * 80.0) - 75.6_f64).abs() < 1e-9);
}
#[test]
fn dated_preferences_validate_and_do_not_rewrite_earlier_days() {
    let s = store();
    let mut p = Preferences {
        effective: "2026-10-13".into(),
        weight: Some(80.0),
        steps: Some(5000),
        minutes: Some(60.0),
        calories: Some(100.0),
    };
    s.save_preferences(&p).unwrap();
    assert_eq!(s.preferences("2026-10-12").unwrap().weight, None);
    p.effective = "2026-10-14".into();
    p.weight = Some(81.0);
    s.save_preferences(&p).unwrap();
    assert_eq!(s.preferences("2026-10-13").unwrap().weight, Some(80.0));
    p.steps = None;
    s.save_preferences(&p).unwrap();
    assert_eq!(s.preferences("2026-10-14").unwrap().steps, None);
    for w in [f64::NAN, 10.0, 150.001, 151.0, 400.0] {
        p.weight = Some(w);
        assert!(s.save_preferences(&p).is_err());
    }
    for w in [20.0, 150.0] {
        p.weight = Some(w);
        s.save_preferences(&p).unwrap();
        assert_eq!(s.preferences(&p.effective).unwrap().weight, Some(w));
    }
    p.weight = None;
    p.steps = Some(0);
    assert!(p.validate().is_err());
    p.steps = None;
    p.minutes = Some(-1.0);
    assert!(p.validate().is_err());
    assert!(date("2026-1-1").is_err());
    assert!(date("bogus").is_err());
    assert!(s.preferences("bogus").is_err());
}
#[test]
fn daily_totals_calories_pause_and_reconnect_are_conservative() {
    let mut s = store();
    s.save_preferences(&Preferences {
        effective: "2026-10-13".into(),
        weight: Some(80.0),
        ..Default::default()
    })
    .unwrap();
    put(&mut s, 0, 0, 3.0, "a");
    put(&mut s, 1, 2, 3.0, "a");
    put(&mut s, 2, 4, 0.0, "a");
    let d = read(&s, 2);
    assert_eq!(d.day.steps, 4);
    assert_eq!(d.day.active_ms, 2000);
    assert_eq!(d.day.sessions, 1);
    assert!((d.day.kcal.unwrap() - 0.084).abs() < 1e-9);
    assert_eq!(d.sessions[0].state, "Paused");
    put(&mut s, 10, 4, 3.0, "a");
    put(&mut s, 11, 6, 3.0, "a");
    assert_eq!(read(&s, 11).day.sessions, 1);
    put(&mut s, 12, 6, 0.0, "a");
    assert_eq!(read(&s, 312).sessions[0].state, "Finished");
    put(&mut s, 312, 6, 3.0, "a");
    put(&mut s, 313, 8, 3.0, "a");
    assert_eq!(read(&s, 313).day.sessions, 2);
    put(&mut s, 320, 10, 3.0, "b");
    put(&mut s, 321, 12, 3.0, "b");
    let d = read(&s, 321);
    assert_eq!(d.day.steps, 10);
    assert_eq!(d.day.sessions, 2);
    assert!(d.sessions[0].gap);
    assert_eq!(d.day.steps, d.sessions.iter().map(|s| s.steps).sum::<i64>());
    assert_eq!(read(&s, 321).days.len(), 7);
}
#[test]
fn silence_empty_belt_and_excluded_trial_do_not_create_exercise() {
    let mut s = store();
    put(&mut s, 0, 0, 3.0, "a");
    put(&mut s, 1, 0, 3.0, "a");
    assert_eq!(read(&s, 3600).day.sessions, 0);
    put(&mut s, 2, 2, 3.0, "a");
    assert_eq!(read(&s, 3600).day.sessions, 1);
    assert_ne!(read(&s, 3600).sessions[0].state, "Finished");
    s.record(
        Event {
            key: "x:0",
            device: "pad",
            connection: "x:0",
            utc: at(3),
            monotonic_ns: Some(0),
            kind: "recorder_started",
            uuid: "",
            raw: &[],
            detail: "Explicit empty-belt control test",
        },
        None,
    )
    .unwrap();
    put(&mut s, 4, 2, 3.0, "x:1");
    put(&mut s, 5, 8, 3.0, "x:1");
    let d = read(&s, 5);
    assert_eq!(d.day.steps, 2);
    assert_eq!(d.day.sessions, 1);
}
#[test]
fn duplicate_out_of_order_reset_and_midnight_do_not_duplicate_or_invent_time() {
    let mut s = store();
    put(&mut s, 0, 0, 3.0, "a");
    put(&mut s, 1, 2, 3.0, "a");
    put(&mut s, 1, 2, 3.0, "a");
    put(&mut s, -1, 100, 3.0, "a");
    put(&mut s, 2, 4, 3.0, "a");
    put(&mut s, 3, 0, 3.0, "a");
    put(&mut s, 4, 2, 3.0, "a");
    assert_eq!(read(&s, 4).day.steps, 6);
    let utc = DateTime::parse_from_rfc3339("2026-10-13T20:59:59Z")
        .unwrap()
        .with_timezone(&Utc);
    for (i, steps) in [(0, 2), (1, 4), (2, 6)] {
        s.record(
            Event {
                key: &format!("mid:{i}"),
                device: "pad",
                connection: "a",
                utc: utc + chrono::Duration::seconds(i),
                monotonic_ns: None,
                kind: "notification",
                uuid: "",
                raw: &[],
                detail: "",
            },
            Some(&Sample {
                steps: Some(steps),
                speed_kmh: Some(3.0),
                ..Default::default()
            }),
        )
        .unwrap();
    }
    let d = s
        .dashboard(
            &Request {
                day: "2026-10-14".into(),
                range: 1,
            },
            utc + chrono::Duration::seconds(2),
        )
        .unwrap();
    assert_eq!(d.day.steps, 2);
    assert_eq!(d.day.active_ms, 1000);
    assert_eq!(d.day.sessions, 1);
}
#[test]
fn legacy_is_unassigned_and_additive_schema_is_idempotent() {
    let s = store();
    initialize(&s.conn).unwrap();
    s.conn.execute("INSERT INTO events VALUES(99,'legacy','pad','a','2026-10-13T09:00:00Z',0,'notification','','','', 'Europe/Helsinki')",[]).unwrap();
    s.conn
        .execute(
            "INSERT INTO samples VALUES(99,'{}',12,'observed','2026-10-13')",
            [],
        )
        .unwrap();
    let d = read(&s, 0);
    assert_eq!(d.day.steps, 12);
    assert_eq!(d.day.unassigned, 12);
    assert_eq!(d.day.kcal, None);
    assert_eq!(d.day.active_ms, 0);
    assert!(d.day.quality().contains("Partial"));
    let empty = s
        .dashboard(
            &Request {
                day: "2026-10-12".into(),
                range: 0,
            },
            at(0),
        )
        .unwrap();
    assert_eq!(empty.days.len(), 1);
    assert_eq!(empty.day.quality(), "No recorded data");
}
#[test]
fn verified_pause_deadline_is_persisted_and_disconnect_ends_the_session() {
    let mut s = store();
    put(&mut s, 0, 0, 3.0, "a");
    put(&mut s, 1, 2, 3.0, "a");
    let status = |s: &mut Store, second: i64, raw: &[u8], connection: &str, kind: &str| {
        s.record(
            Event {
                key: &format!("status:{second}"),
                device: "pad",
                connection,
                utc: at(second),
                monotonic_ns: Some(second * 1_000_000_000),
                kind,
                uuid: "00002ada-0000-1000-8000-00805f9b34fb",
                raw,
                detail: "",
            },
            None,
        )
        .unwrap();
    };
    status(&mut s, 2, &[2, 2], "a", "notification");
    s.expire_sessions(at(301)).unwrap();
    assert_eq!(read(&s, 301).sessions[0].state, "Paused");
    s.expire_sessions(at(302)).unwrap();
    assert_eq!(read(&s, 302).sessions[0].state, "Finished");
    put(&mut s, 303, 2, 3.0, "b");
    put(&mut s, 304, 4, 3.0, "b");
    assert_eq!(read(&s, 304).day.sessions, 2);
    status(&mut s, 305, &[2, 1], "b", "notification");
    status(&mut s, 306, &[], "b", "manual_disconnect");
    // The disconnect ends the stopped session at the stop, not at "now".
    let d = read(&s, 306);
    assert_eq!(d.sessions[0].state, "Finished");
    assert_eq!(d.sessions[0].end, at(305).to_rfc3339());
    assert!(d.current.is_none());
    status(&mut s, 307, &[2, 2], "b", "notification");
    status(&mut s, 1001, &[4], "b", "notification");
    put(&mut s, 1002, 4, 4.1, "b");
    put(&mut s, 1003, 6, 4.1, "b");
    let d = read(&s, 1003);
    assert_eq!(d.day.sessions, 3);
    // Every session ended at a confirmed stop, so none is partial.
    assert_eq!(d.day.partial_sessions, 0);
}
#[test]
fn speed_changes_missing_profile_and_timezone_days_have_independent_reference_totals() {
    let mut s = store();
    put(&mut s, 0, 0, 3.0, "a");
    put(&mut s, 1, 2, 4.1, "a");
    put(&mut s, 2, 4, 4.1, "a");
    assert_eq!(read(&s, 2).day.kcal, None);
    s.save_preferences(&Preferences {
        effective: "2026-10-13".into(),
        weight: Some(80.0),
        ..Default::default()
    })
    .unwrap();
    // 1 second at 2.8 MET plus 1 second at 3.5 MET, above 1 MET rest.
    assert!((read(&s, 2).day.kcal.unwrap() - 0.10033333333333333).abs() < 1e-9);
    put(&mut s, 3, 6, 10.0, "a");
    put(&mut s, 4, 8, 10.0, "a");
    assert_eq!(read(&s, 4).day.active_ms, 3000);
    let dst = s
        .dashboard(
            &Request {
                day: "2026-10-25".into(),
                range: 30,
            },
            at(0),
        )
        .unwrap();
    assert_eq!(dst.days.len(), 30);
    assert_eq!(dst.days.last().unwrap().date, "2026-10-25");
    for (range, expected) in [(0, 1), (90, 90), (1000, 90)] {
        let long = s
            .dashboard(
                &Request {
                    day: "2026-10-25".into(),
                    range,
                },
                at(0),
            )
            .unwrap();
        assert_eq!(long.days.len(), expected);
        assert_eq!(
            long.days.first().unwrap().date == "2026-10-25",
            expected == 1
        );
    }
    assert!(
        s.dashboard(
            &Request {
                day: "invalid".into(),
                range: 30
            },
            at(0)
        )
        .is_err()
    );
}
#[test]
fn derived_write_failure_rolls_back_original_progress_and_reopen_preserves_identity() {
    let folder = std::env::temp_dir().join(format!(
        "desksteps-session-reopen-{}",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let path = folder.join("test.sqlite");
    let mut s = Store::open(&path, chrono_tz::Europe::Helsinki).unwrap();
    put(&mut s, 0, 0, 3.0, "a");
    put(&mut s, 1, 2, 3.0, "a");
    let session = read(&s, 1).sessions[0].id;
    drop(s);
    let mut s = Store::open(&path, chrono_tz::Europe::Helsinki).unwrap();
    put(&mut s, 2, 2, 3.0, "b");
    put(&mut s, 3, 4, 3.0, "b");
    assert_eq!(read(&s, 3).sessions[0].id, session);
    s.conn.execute_batch("CREATE TRIGGER fail_ui BEFORE INSERT ON ui_intervals BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    let sample = Sample {
        steps: Some(6),
        speed_kmh: Some(3.0),
        ..Default::default()
    };
    assert!(
        s.record(
            Event {
                key: "failed",
                device: "pad",
                connection: "b",
                utc: at(4),
                monotonic_ns: Some(4_000_000_000),
                kind: "notification",
                uuid: "",
                raw: &[],
                detail: ""
            },
            Some(&sample)
        )
        .is_err()
    );
    assert_eq!(read(&s, 4).day.steps, 4);
    assert_eq!(
        s.conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE source_key='failed'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(s);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(folder).unwrap();
}
#[test]
fn disconnect_while_walking_ends_at_the_last_sample_and_a_prompt_reconnect_extends_it() {
    let mut s = store();
    let lost = |s: &mut Store, second: i64, connection: &str| {
        s.record(
            Event {
                key: &format!("lost:{second}"),
                device: "pad",
                connection,
                utc: at(second),
                monotonic_ns: Some(second * 1_000_000_000),
                kind: "disconnected",
                uuid: "",
                raw: &[],
                detail: "",
            },
            None,
        )
        .unwrap();
    };
    put(&mut s, 0, 0, 3.0, "a");
    put(&mut s, 1, 2, 3.0, "a");
    put(&mut s, 2, 4, 3.0, "a");
    lost(&mut s, 3, "a");
    let d = read(&s, 3600);
    assert!(d.current.is_none());
    assert_eq!(
        (d.sessions[0].state.as_str(), d.sessions[0].end.clone()),
        ("Finished", at(2).to_rfc3339())
    );
    assert!(d.sessions[0].gap);
    // Within the pause window the same session continues.
    put(&mut s, 200, 4, 3.0, "b");
    put(&mut s, 201, 6, 3.0, "b");
    let d = read(&s, 201);
    assert_eq!(d.day.sessions, 1);
    assert_eq!(d.sessions[0].end, at(201).to_rfc3339());
    assert_eq!(d.current.as_ref().map(|c| c.id), Some(d.sessions[0].id));
    // Later, without recovered steps, a new session starts.
    lost(&mut s, 202, "b");
    put(&mut s, 900, 6, 3.0, "c");
    put(&mut s, 901, 8, 3.0, "c");
    let d = read(&s, 901);
    assert_eq!(d.day.sessions, 2);
    assert_eq!(d.sessions[1].end, at(201).to_rfc3339());
    assert_eq!(d.day.steps, d.sessions.iter().map(|s| s.steps).sum::<i64>());
}
