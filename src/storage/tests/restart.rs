use super::*;
use crate::statistics::{Dashboard, Request};

fn at(second: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-23T09:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
        + chrono::Duration::seconds(second)
}
fn sample(steps: u32, speed: f64) -> Sample {
    Sample {
        steps: Some(steps),
        speed_kmh: Some(speed),
        elapsed_s: Some(100),
        distance_m: Some(50),
        ..Default::default()
    }
}
fn record(s: &mut Store, second: i64, connection: &str, kind: &str, sample: Option<&Sample>) {
    s.record(
        Event {
            key: &format!("{connection}:{second}:{kind}"),
            device: "pad",
            connection,
            utc: at(second),
            monotonic_ns: Some(second * 1_000_000_000),
            kind,
            uuid: "",
            raw: &[],
            detail: "synthetic",
        },
        sample,
    )
    .unwrap();
}
fn read(s: &Store) -> Dashboard {
    s.dashboard(
        &Request {
            day: "2026-10-23".into(),
            range: 1,
        },
        at(1000),
    )
    .unwrap()
}
fn begin(s: &mut Store, paused: bool) {
    record(s, 0, "a", "notification", Some(&sample(0, 3.0)));
    record(s, 1, "a", "notification", Some(&sample(2, 3.0)));
    if paused {
        record(s, 2, "a", "notification", Some(&sample(2, 0.0)));
    }
    record(s, 3, "a", "recorder_exited", None);
}
fn resume(s: &mut Store, second: i64, connection: &str, first: &Sample) {
    record(s, second, connection, "recorder_started", None);
    record(s, second + 1, connection, "connected", None);
    record(s, second + 2, connection, "notification", Some(first));
    let mut next = first.clone();
    next.steps = Some(first.steps.unwrap_or(2) + 2);
    next.distance_m = Some(first.distance_m.unwrap_or(50));
    next.elapsed_s = Some(first.elapsed_s.unwrap_or(100) + 1);
    record(s, second + 3, connection, "notification", Some(&next));
    if first.steps.is_none() {
        // Accounting needs another ordered sample after a missing baseline.
        next.steps = next.steps.map(|n| n + 2);
        record(s, second + 4, connection, "notification", Some(&next));
    }
    next.speed_kmh = Some(0.0);
    record(s, second + 5, connection, "notification", Some(&next));
}
fn fresh() -> Store {
    Store::initialize(Connection::open_in_memory().unwrap(), chrono_tz::UTC).unwrap()
}

#[test]
fn paused_restart_survives_real_database_reopen_and_repeated_restarts() {
    let path = std::env::temp_dir().join(format!(
        "desksteps-paused-restart-{}.sqlite",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        begin(&mut s, true);
        assert!(!read(&s).sessions[0].gap);
    }
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        // The device clock advances after Resume, before any new steps.
        let mut first = sample(2, 1.0);
        first.elapsed_s = Some(101);
        resume(&mut s, 120, "b", &first);
        record(&mut s, 125, "b", "recorder_exited", None);
    }
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        let mut first = sample(4, 1.0);
        first.elapsed_s = Some(103);
        resume(&mut s, 180, "c", &first);
        let d = read(&s);
        assert_eq!(d.sessions.len(), 1);
        assert!(!d.sessions[0].gap);
        assert_eq!(
            (d.day.steps, d.day.recovered_steps, d.day.uncertain),
            (6, 0, 0)
        );
        assert_eq!(d.day.quality(), "Observed intervals only");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn changed_reset_or_missing_restart_counters_never_become_a_clean_pause() {
    let original = sample(2, 3.0);
    let mut cases = Vec::new();
    for field in 0..7 {
        let mut s = original.clone();
        match field {
            0 => s.steps = Some(4),
            1 => s.distance_m = Some(60),
            2 => s.elapsed_s = Some(1),
            3 => s.steps = None,
            4 => s.distance_m = None,
            5 => s.elapsed_s = None,
            _ => s.distance_m = Some(0),
        }
        cases.push(s);
    }
    let mut reset = original.clone();
    reset.steps = Some(0);
    cases.push(reset);
    for enabled in [true, false] {
        for first in &cases {
            let mut s = fresh();
            s.set_setting("recover_gaps", if enabled { "true" } else { "false" })
                .unwrap();
            begin(&mut s, true);
            resume(&mut s, 120, "b", first);
            let d = read(&s);
            assert_eq!(d.sessions.len(), 1);
            assert!(d.sessions[0].gap, "enabled={enabled}, first={first:?}");
            if !enabled {
                assert_eq!(d.day.recovered_steps, 0);
            }
        }
    }
}

#[test]
fn unconfirmed_exit_stays_a_gap_and_five_minutes_starts_another_session() {
    for (paused, second, sessions, gap) in [(false, 120, 1, true), (true, 299, 2, false)] {
        let mut s = fresh();
        begin(&mut s, paused);
        // First credited step is at second + 3, measured from pause at 2.
        resume(&mut s, second, "b", &sample(2, 3.0));
        let d = read(&s);
        assert_eq!(d.sessions.len(), sessions);
        assert_eq!(d.sessions[0].gap, gap);
    }
}

#[test]
fn a_clean_restart_does_not_erase_an_earlier_real_gap() {
    let mut s = fresh();
    begin(&mut s, false);
    resume(&mut s, 60, "b", &sample(2, 3.0));
    record(&mut s, 65, "b", "recorder_exited", None);
    let mut first = sample(4, 3.0);
    first.elapsed_s = Some(101);
    resume(&mut s, 120, "c", &first);
    assert!(read(&s).sessions[0].gap);
}

#[test]
fn another_disconnect_requires_fresh_reconciliation_and_later_loss_stays_uncertain() {
    for changed in [false, true] {
        let mut s = fresh();
        s.set_setting("recover_gaps", "false").unwrap();
        begin(&mut s, true);
        record(&mut s, 60, "b", "notification", Some(&sample(2, 3.0)));
        record(&mut s, 61, "b", "disconnected", None);
        resume(&mut s, 120, "c", &sample(if changed { 3 } else { 2 }, 3.0));
        assert_eq!(read(&s).sessions[0].gap, changed);
    }
    let mut s = fresh();
    s.set_setting("recover_gaps", "false").unwrap();
    begin(&mut s, true);
    record(&mut s, 60, "b", "notification", Some(&sample(2, 3.0)));
    // Missing activity after initial reconciliation must not be hidden either.
    record(&mut s, 70, "b", "notification", Some(&sample(4, 3.0)));
    record(&mut s, 71, "b", "notification", Some(&sample(6, 3.0)));
    assert!(read(&s).sessions[0].gap);
}

#[test]
fn upgrade_repairs_only_derived_session_coverage_and_runs_once() {
    let mut s = fresh();
    begin(&mut s, true);
    resume(&mut s, 120, "b", &sample(2, 3.0));
    record(&mut s, 125, "b", "recorder_exited", None);
    // Reproduce the old version's persisted warning and version marker.
    s.conn.execute("UPDATE ui_sessions SET gap=1", []).unwrap();
    s.set_setting("session_rules", "disconnect-ends-session")
        .unwrap();
    let before = super::database_snapshot(&s);
    let totals = read(&s).day;
    s.rebuild_sessions().unwrap();
    let d = read(&s);
    assert!(!d.sessions[0].gap);
    assert_eq!(
        (d.day.steps, d.day.active_ms, d.day.factor),
        (totals.steps, totals.active_ms, totals.factor)
    );
    let after = super::database_snapshot(&s);
    // Settings, sessions and UI anchors may change; original evidence,
    // accounting/recovery anchors and every stored interval stay identical.
    for table in [1, 2, 3, 4, 7, 8, 9, 10, 11] {
        assert_eq!(before[table], after[table], "table index {table}");
    }
    s.rebuild_sessions().unwrap();
    assert_eq!(super::database_snapshot(&s), after);
}
