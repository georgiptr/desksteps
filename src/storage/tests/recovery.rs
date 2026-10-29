use super::*;
use crate::statistics::{Dashboard, Preferences, Request};

fn fresh() -> Store {
    let s = Store::initialize(Connection::open_in_memory().unwrap(), chrono_tz::UTC).unwrap();
    s.save_preferences(&Preferences {
        effective: "2026-10-23".into(),
        weight: Some(80.0),
        ..Default::default()
    })
    .unwrap();
    s
}
fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-23T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
        + chrono::Duration::seconds(seconds)
}
fn sample(steps: u32, elapsed: u32, distance: u32) -> Sample {
    Sample {
        steps: Some(steps),
        elapsed_s: Some(elapsed),
        distance_m: Some(distance),
        speed_kmh: Some(3.0),
        ..Default::default()
    }
}
fn send(s: &mut Store, second: i64, connection: &str, sample: &Sample) -> bool {
    s.record(
        Event {
            key: &format!("{connection}:{second}"),
            device: "pad",
            connection,
            utc: at(second),
            monotonic_ns: Some(second * 1_000_000_000),
            kind: "notification",
            uuid: "",
            raw: &[1, 2, 3],
            detail: "synthetic",
        },
        Some(sample),
    )
    .unwrap()
}
fn dashboard(s: &Store, second: i64) -> Dashboard {
    s.dashboard(
        &Request {
            day: at(second).date_naive().to_string(),
            range: 1,
        },
        at(second),
    )
    .unwrap()
}

#[test]
fn defaults_on_recovers_reference_example_and_never_recredits_a_snapshot() {
    let mut s = fresh();
    assert_eq!(s.setting("recover_gaps").unwrap(), None);
    send(&mut s, 0, "first", &sample(1000, 600, 500));
    assert_eq!(dashboard(&s, 0).day.steps, 0);
    send(&mut s, 180, "second", &sample(1300, 780, 650));
    let d = dashboard(&s, 180);
    assert_eq!(
        (
            d.day.steps,
            d.day.recovered_steps,
            d.day.active_ms,
            d.day.recovered_ms,
            d.day.unassigned
        ),
        (300, 300, 180000, 180000, 0)
    );
    assert!((d.day.kcal.unwrap() - 7.56).abs() < 1e-9);
    assert_eq!(d.day.kcal, d.day.recovered_kcal);
    assert_eq!(d.sessions[0].steps, 300);
    assert_eq!(d.sessions[0].recovered_ms, 180000);
    assert!(d.sessions[0].gap && d.day.quality().starts_with("Partial"));
    assert_eq!(s.totals(at(180)).unwrap().recorded_steps, 300);
    assert_eq!(
        s.conn
            .query_row(
                "SELECT quality FROM samples ORDER BY event_id DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "collection_gap"
    );
    assert!(!send(&mut s, 180, "second", &sample(1300, 780, 650)));
    send(&mut s, 181, "third", &sample(1300, 780, 650));
    send(&mut s, 182, "third", &sample(1304, 781, 651));
    assert_eq!(dashboard(&s, 182).day.steps, 304);
    assert_eq!(dashboard(&s, 182).day.recovered_steps, 300);
}

#[test]
fn setting_applies_to_future_gaps_without_rewriting_history() {
    let mut s = fresh();
    send(&mut s, 0, "a", &sample(100, 100, 100));
    s.set_setting("recover_gaps", "false").unwrap();
    send(&mut s, 10, "b", &sample(110, 110, 110));
    assert_eq!(dashboard(&s, 10).day.steps, 0);
    s.set_setting("recover_gaps", "true").unwrap();
    assert_eq!(dashboard(&s, 10).day.steps, 0);
    send(&mut s, 20, "c", &sample(120, 120, 120));
    s.set_setting("recover_gaps", "false").unwrap();
    assert_eq!(dashboard(&s, 20).day.steps, 10);
    send(&mut s, 30, "d", &sample(130, 130, 130));
    send(&mut s, 31, "d", &sample(132, 131, 131));
    assert_eq!(dashboard(&s, 31).day.steps, 12);
}

#[test]
fn recoverable_metrics_are_independent_and_missing_calories_are_not_zero() {
    for missing in ["distance", "elapsed", "steps", "weight"] {
        let mut s = fresh();
        let mut old = sample(1000, 600, 500);
        let mut new = sample(1300, 780, 650);
        match missing {
            "distance" => {
                old.distance_m = None;
                new.distance_m = None;
            }
            "elapsed" => {
                old.elapsed_s = None;
                new.elapsed_s = None;
            }
            "steps" => {
                old.steps = None;
                new.steps = None;
            }
            _ => s
                .save_preferences(&Preferences {
                    effective: "2026-10-23".into(),
                    ..Default::default()
                })
                .unwrap(),
        }
        send(&mut s, 0, "a", &old);
        send(&mut s, 180, "b", &new);
        let d = dashboard(&s, 180);
        assert_eq!(d.day.steps, if missing == "steps" { 0 } else { 300 });
        assert_eq!(
            d.day.active_ms,
            if missing == "elapsed" { 0 } else { 180000 }
        );
        if missing == "steps" {
            assert!(d.day.kcal.unwrap() > 0.0);
        } else {
            assert_eq!(d.day.kcal, None);
        }
        assert_eq!(
            d.day.recovery_kcal_missing,
            matches!(missing, "distance" | "elapsed")
        );
    }
}

#[test]
fn resets_empty_belts_and_impossible_timers_never_invent_activity() {
    for (new, expected_steps, expected_ms) in [
        (sample(2, 780, 650), 0, 0),
        (sample(1300, 2, 650), 0, 0),
        (sample(1300, 780, 2), 0, 0),
        (sample(1000, 780, 500), 0, 0),
        (sample(1300, 900, 650), 300, 0),
    ] {
        let mut s = fresh();
        send(&mut s, 0, "a", &sample(1000, 600, 500));
        send(&mut s, 180, "b", &new);
        let d = dashboard(&s, 180);
        assert_eq!(
            (d.day.steps, d.day.active_ms),
            (expected_steps, expected_ms)
        );
        assert_eq!(
            d.day.kcal,
            if expected_steps == 0 { Some(0.0) } else { None }
        );
    }
}

#[test]
fn midnight_and_same_connection_silence_use_the_receipt_day() {
    let mut s = fresh();
    send(&mut s, 43199, "a", &sample(1000, 600, 500));
    send(&mut s, 43200, "a", &sample(1002, 601, 501));
    let d = dashboard(&s, 43200);
    assert_eq!(
        (d.day.steps, d.day.recovered_steps, d.day.recovered_ms),
        (2, 2, 1000)
    );
    send(&mut s, 43210, "a", &sample(1012, 611, 511));
    assert_eq!(dashboard(&s, 43210).day.recovered_steps, 12);
    assert_eq!(s.totals(at(43199)).unwrap().recorded_steps, 0);
}

#[test]
fn excluded_trials_and_other_devices_cannot_supply_personal_recovery() {
    let mut s = fresh();
    send(&mut s, 0, "a:1", &sample(1000, 600, 500));
    s.record(
        Event {
            key: "trial",
            device: "pad",
            connection: "trial:0",
            utc: at(1),
            monotonic_ns: Some(0),
            kind: "recorder_started",
            uuid: "",
            raw: &[],
            detail: "empty-belt trial",
        },
        None,
    )
    .unwrap();
    send(&mut s, 180, "trial:1", &sample(1300, 780, 650));
    send(&mut s, 360, "personal:1", &sample(1600, 960, 800));
    assert_eq!(dashboard(&s, 360).day.recovered_steps, 0);
    send(&mut s, 540, "later:1", &sample(1900, 1140, 950));
    assert_eq!(dashboard(&s, 540).day.recovered_steps, 300);
    let mut e = event("other", None);
    e.device = "other";
    e.connection = "other";
    e.utc = at(720);
    s.record(e, Some(&sample(5000, 5000, 5000))).unwrap();
    assert_eq!(dashboard(&s, 720).day.recovered_steps, 300);
}

#[test]
fn recovery_writes_roll_back_together_and_retry_is_idempotent() {
    for (table, operation) in [
        ("recovery_credits", "INSERT"),
        ("recovery_baselines", "UPDATE"),
        ("ui_intervals", "INSERT"),
        ("anchors", "UPDATE"),
    ] {
        let mut s = fresh();
        send(&mut s, 0, "a", &sample(1000, 600, 500));
        let before = database_snapshot(&s);
        s.conn.execute_batch(&format!("CREATE TRIGGER fail_recovery BEFORE {operation} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;")).unwrap();
        let mut e = event("reconnect", Some(180_000_000_000));
        e.device = "pad";
        e.connection = "b";
        e.utc = at(180);
        assert!(s.record(e, Some(&sample(1300, 780, 650))).is_err());
        assert_eq!(database_snapshot(&s), before);
        s.conn.execute_batch("DROP TRIGGER fail_recovery").unwrap();
        send(&mut s, 180, "b", &sample(1300, 780, 650));
        assert_eq!(dashboard(&s, 180).day.recovered_steps, 300);
    }
}

#[test]
fn upgrade_uses_last_original_anchor_without_backfilling_old_gaps() {
    let mut s = fresh();
    s.set_setting("recover_gaps", "false").unwrap();
    send(&mut s, 0, "a", &sample(1000, 600, 500));
    send(&mut s, 180, "b", &sample(1300, 780, 650));
    s.conn.execute_batch("DROP TABLE recovery_baselines; DROP TABLE recovery_credits; DELETE FROM settings WHERE key='recover_gaps'").unwrap();
    super::super::schema::initialize_statistics(&s.conn).unwrap();
    send(&mut s, 360, "c", &sample(1600, 960, 800));
    assert_eq!(dashboard(&s, 360).day.recovered_steps, 300);
    assert_eq!(
        s.conn
            .query_row("SELECT COUNT(*) FROM recovery_credits", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn malformed_baselines_fail_atomically_and_out_of_order_cannot_rewind_them() {
    let mut s = fresh();
    send(&mut s, 0, "a", &sample(1000, 600, 500));
    send(&mut s, 180, "b", &sample(1300, 780, 650));
    let before: String = s
        .conn
        .query_row("SELECT sample FROM recovery_baselines", [], |r| r.get(0))
        .unwrap();
    send(&mut s, 100, "b", &sample(1200, 700, 600));
    assert_eq!(
        s.conn
            .query_row("SELECT sample FROM recovery_baselines", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        before
    );
    for column in ["utc", "sample"] {
        s.conn
            .execute(
                &format!("UPDATE recovery_baselines SET {column}='invalid'"),
                [],
            )
            .unwrap();
        let before = database_snapshot(&s);
        let mut e = event(column, None);
        e.device = "pad";
        e.connection = "c";
        e.utc = at(360);
        assert!(s.record(e, Some(&sample(1600, 960, 800))).is_err());
        assert_eq!(database_snapshot(&s), before);
        s.conn
            .execute(
                "UPDATE recovery_baselines SET utc=?1,sample=?2",
                params![
                    at(180).to_rfc3339(),
                    serde_json::to_string(&sample(1300, 780, 650)).unwrap()
                ],
            )
            .unwrap();
    }
}

#[test]
fn real_database_reopen_keeps_the_recovery_baseline_and_disabled_setting() {
    let path = std::env::temp_dir().join(format!(
        "desksteps-recovery-{}.sqlite",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        send(&mut s, 0, "a", &sample(1000, 600, 500));
    }
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        send(&mut s, 180, "b", &sample(1300, 780, 650));
        assert_eq!(dashboard(&s, 180).day.recovered_steps, 300);
        s.set_setting("recover_gaps", "false").unwrap();
    }
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        send(&mut s, 360, "c", &sample(1600, 960, 800));
        assert_eq!(dashboard(&s, 360).day.recovered_steps, 300);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn missing_steps_do_not_allow_monotonic_reversal_to_rewind_recovery() {
    let mut s = fresh();
    let mut old = sample(1000, 600, 500);
    old.steps = None;
    send(&mut s, 0, "a", &old);
    let mut e = event("reverse", Some(-1));
    e.device = "pad";
    e.connection = "a";
    e.utc = at(10);
    s.record(
        e,
        Some(&Sample {
            elapsed_s: Some(610),
            distance_m: Some(510),
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(
        s.conn
            .query_row("SELECT utc FROM recovery_baselines", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        at(0).to_rfc3339()
    );
}
#[test]
fn recovered_steps_extend_a_session_ended_by_a_lost_link() {
    let mut s = fresh();
    send(&mut s, 0, "first", &sample(1000, 600, 500));
    send(&mut s, 1, "first", &sample(1002, 601, 501));
    s.record(
        Event {
            key: "first:lost",
            device: "pad",
            connection: "first",
            utc: at(2),
            monotonic_ns: Some(2_000_000_000),
            kind: "disconnected",
            uuid: "",
            raw: &[],
            detail: "synthetic",
        },
        None,
    )
    .unwrap();
    assert!(dashboard(&s, 1200).current.is_none());
    // Twenty minutes later the counters show the walk went on.
    send(&mut s, 1200, "second", &sample(3000, 1800, 1500));
    let d = dashboard(&s, 1200);
    assert_eq!(d.sessions.len(), 1);
    assert_eq!(d.sessions[0].recovered_steps, 1998);
    assert_eq!(d.sessions[0].end, at(1200).to_rfc3339());
    assert!(d.sessions[0].gap);
    assert_eq!(d.current.as_ref().map(|c| c.id), Some(d.sessions[0].id));
}
