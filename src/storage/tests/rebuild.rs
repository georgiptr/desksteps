use super::*;
use crate::statistics::{Dashboard, Request};

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-23T09:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
        + chrono::Duration::seconds(seconds)
}
fn event<'a>(key: &'a str, connection: &'a str, second: i64, kind: &'a str) -> Event<'a> {
    Event {
        key,
        device: "pad",
        connection,
        utc: at(second),
        monotonic_ns: Some(second * 1_000_000_000),
        kind,
        uuid: "",
        raw: &[],
        detail: "synthetic",
    }
}
fn walk(s: &mut Store, connection: &str, second: i64, steps: u32) {
    let sample = Sample {
        steps: Some(steps),
        speed_kmh: Some(3.0),
        ..Default::default()
    };
    let key = format!("{connection}:{second}");
    s.record(
        event(&key, connection, second, "notification"),
        Some(&sample),
    )
    .unwrap();
}
fn lost(s: &mut Store, connection: &str, second: i64) {
    let key = format!("{connection}:lost");
    s.record(event(&key, connection, second, "disconnected"), None)
        .unwrap();
}
fn dashboard(s: &Store, second: i64) -> Dashboard {
    s.dashboard(
        &Request {
            day: "2026-10-23".into(),
            range: 1,
        },
        at(second),
    )
    .unwrap()
}
/// Two walks split by a disconnect, then the projection rewritten the way the
/// earlier rules left it: one session still open across both walks.
fn merged() -> Store {
    let mut s = Store::initialize(Connection::open_in_memory().unwrap(), chrono_tz::UTC).unwrap();
    s.set_setting("recover_gaps", "false").unwrap();
    for (second, steps) in [(0, 0), (1, 2), (2, 4)] {
        walk(&mut s, "a", second, steps);
    }
    lost(&mut s, "a", 3);
    for (second, steps) in [(1800, 4), (1801, 6), (1802, 8)] {
        walk(&mut s, "b", second, steps);
    }
    lost(&mut s, "b", 1803);
    s.conn
        .execute_batch(
            "UPDATE ui_intervals SET session=(SELECT MIN(id) FROM ui_sessions);
             DELETE FROM ui_sessions WHERE id<>(SELECT MIN(id) FROM ui_sessions);
             UPDATE ui_sessions SET state='Walking',end='2026-10-23T09:30:02+00:00';
             UPDATE ui_anchors SET value=json_set(value,'$.session',(SELECT id FROM ui_sessions),'$.gap',json('true'),'$.ended',NULL);
             DELETE FROM settings WHERE key='session_rules';",
        )
        .unwrap();
    s
}
#[test]
fn rebuild_splits_sessions_merged_across_a_disconnect_without_changing_totals() {
    let mut s = merged();
    let before = dashboard(&s, 7200);
    assert_eq!(before.sessions.len(), 1);
    assert!(before.current.is_some());
    s.rebuild_sessions().unwrap();
    let after = dashboard(&s, 7200);
    assert_eq!(
        s.setting("session_rules").unwrap().as_deref(),
        Some("confirmed-pause-restart-v2")
    );
    assert!(after.current.is_none());
    assert_eq!(after.sessions.len(), 2);
    assert_eq!(after.sessions[1].end, at(2).to_rfc3339());
    assert_eq!(after.sessions[0].end, at(1802).to_rfc3339());
    assert!(after.sessions.iter().all(|c| c.state == "Finished"));
    assert_eq!(
        (after.day.steps, after.day.active_ms, after.day.factor),
        (before.day.steps, before.day.active_ms, before.day.factor)
    );
    // The rebuild runs once; later opens leave the projection alone.
    let snapshot = super::database_snapshot(&s);
    s.rebuild_sessions().unwrap();
    assert_eq!(super::database_snapshot(&s), snapshot);
}
#[test]
fn rebuild_keeps_the_earlier_projection_when_an_interval_would_change() {
    let mut s = merged();
    s.conn
        .execute_batch("UPDATE ui_intervals SET steps=steps+1 WHERE event_id=(SELECT MAX(event_id) FROM ui_intervals)")
        .unwrap();
    let snapshot = super::database_snapshot(&s);
    s.rebuild_sessions().unwrap();
    let outcome = s.setting("session_rules").unwrap().unwrap();
    assert!(
        outcome.starts_with("confirmed-pause-restart-v2: kept earlier sessions: interval"),
        "{outcome}"
    );
    s.conn
        .execute("DELETE FROM settings WHERE key='session_rules'", [])
        .unwrap();
    assert_eq!(super::database_snapshot(&s), snapshot);
}
