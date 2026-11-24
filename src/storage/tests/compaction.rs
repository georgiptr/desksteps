use super::*;
use crate::statistics::{Dashboard, Preferences, Request};

fn at(stamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(stamp)
        .unwrap()
        .with_timezone(&Utc)
}
fn store() -> Store {
    let s = Store::initialize(Connection::open_in_memory().unwrap(), chrono_tz::UTC).unwrap();
    s.save_preferences(&Preferences {
        effective: "2026-11-01".into(),
        weight: Some(75.0),
        ..Default::default()
    })
    .unwrap();
    s
}
/// One second per sample; counters continue from `base` so a new connection
/// after a gap exercises recovery credits.
fn walk(s: &mut Store, device: &str, connection: &str, start: &str, seconds: u32, base: u32) {
    let start = at(start);
    for second in 0..=seconds {
        let n = base + second;
        s.record(
            Event {
                key: &format!("{device}:{connection}:{second}"),
                device,
                connection,
                utc: start + chrono::Duration::seconds(second.into()),
                monotonic_ns: Some(i64::from(second) * 1_000_000_000),
                kind: "notification",
                uuid: crate::protocol::TREADMILL,
                raw: &[0x80, 0],
                detail: "synthetic",
            },
            Some(&Sample {
                steps: Some(n * 2),
                elapsed_s: Some(n),
                distance_m: Some(n),
                speed_kmh: Some(3.0),
                calories_kcal: Some(n / 10),
                calories_per_hour: Some(100),
                ..Default::default()
            }),
        )
        .unwrap();
    }
    // The pad is switched off: the lost link ends the session.
    let end = start + chrono::Duration::seconds(i64::from(seconds) + 30);
    s.record(
        Event {
            key: &format!("{device}:{connection}:off"),
            device,
            connection,
            utc: end,
            monotonic_ns: Some((i64::from(seconds) + 30) * 1_000_000_000),
            kind: "disconnected",
            uuid: "",
            raw: &[],
            detail: "synthetic",
        },
        None,
    )
    .unwrap();
}
fn note(s: &mut Store, key: &str, utc: &str) {
    s.record(
        Event {
            key,
            device: "pad",
            connection: "note",
            utc: at(utc),
            monotonic_ns: None,
            kind: "connected",
            uuid: "",
            raw: &[],
            detail: "synthetic",
        },
        None,
    )
    .unwrap();
}
fn history(s: &mut Store) {
    walk(s, "pad", "p:1", "2026-11-01T10:00:00Z", 120, 0);
    note(s, "n1", "2026-11-01T09:59:00Z");
    walk(s, "pad", "p:2", "2026-11-02T23:58:00Z", 240, 200);
    walk(s, "other", "o:1", "2026-11-03T08:00:00Z", 60, 0);
    walk(s, "pad", "p:3", "2026-11-04T12:00:00Z", 90, 600);
    walk(s, "pad", "p:4", "2026-11-05T12:00:00Z", 30, 800);
}
const NOW: &str = "2026-11-06T12:00:00Z";
fn read(s: &Store, day: &str) -> Dashboard {
    s.dashboard(
        &Request {
            day: day.into(),
            range: 7,
        },
        at(NOW),
    )
    .unwrap()
}
fn count(s: &Store, table: &str) -> i64 {
    s.conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
const DAYS: [&str; 6] = [
    "2026-11-01",
    "2026-11-02",
    "2026-11-03",
    "2026-11-04",
    "2026-11-05",
    "2026-11-06",
];

#[test]
fn compacted_history_reads_exactly_as_before_and_keeps_two_detailed_days() {
    let mut s = store();
    history(&mut s);
    let before: Vec<_> = DAYS.iter().map(|d| read(&s, d)).collect();
    assert!(before[1].day.recovered_steps > 0 || before[1].days[1].recovered_steps > 0);
    assert!(before.iter().any(|d| d.day.pad_calories.total.is_some()));
    let events = count(&s, "events");
    assert_eq!(s.compact(at(NOW)).unwrap(), 4);
    let after: Vec<_> = DAYS.iter().map(|d| read(&s, d)).collect();
    assert_eq!(before, after);
    // Today and yesterday keep every row; older days keep one pinned sample per device.
    let kept = count(&s, "events");
    assert!(kept < events / 3, "{kept} of {events}");
    let old: i64 = s
        .conn
        .query_row(
            "SELECT COUNT(*) FROM samples WHERE local_day<'2026-11-05'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old, 1, "the other device's last sample is its baseline");
    assert_eq!(count(&s, "day_summaries"), 4);
    assert_eq!(count(&s, "session_summaries"), 5);
    let version: i64 = s
        .conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let vacuum: i64 = s
        .conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(vacuum, 2);
    assert!(s.setting("history_compaction").unwrap().is_some());
    // Once per local day; the next day compacts the previous yesterday.
    assert_eq!(s.compact(at(NOW)).unwrap(), 0);
    assert_eq!(s.compact(at("2026-11-07T00:00:01Z")).unwrap(), 1);
    assert_eq!(read(&s, "2026-11-05").sessions, after[4].sessions);
    // A session-rule rebuild replays only retained days and keeps summaries.
    s.set_setting("session_rules", "older").unwrap();
    s.rebuild_sessions().unwrap();
    assert_eq!(
        s.setting("session_rules").unwrap().as_deref(),
        Some("confirmed-pause-restart-v2")
    );
    assert_eq!(read(&s, "2026-11-02"), after[1]);
}

#[test]
fn recording_after_compaction_decides_exactly_as_with_full_history() {
    let (mut full, mut compacted) = (store(), store());
    history(&mut full);
    history(&mut compacted);
    compacted.compact(at(NOW)).unwrap();
    compacted.compact(at("2026-11-08T00:00:00Z")).unwrap();
    for s in [&mut full, &mut compacted] {
        walk(s, "pad", "p:5", "2026-11-08T09:00:00Z", 60, 1000);
        walk(s, "other", "o:2", "2026-11-08T10:00:00Z", 60, 100);
    }
    let day = |s: &Store| {
        s.dashboard(
            &Request {
                day: "2026-11-08".into(),
                range: 1,
            },
            at("2026-11-08T12:00:00Z"),
        )
        .unwrap()
    };
    let (a, b) = (day(&full), day(&compacted));
    assert!(a.day.recovered_steps > 0);
    assert!(a.day.pad_calories.total.is_some());
    assert_eq!(a, b);
}

#[test]
fn weight_still_applies_and_compacted_days_reject_late_events() {
    let mut s = store();
    history(&mut s);
    s.compact(at(NOW)).unwrap();
    let first = read(&s, "2026-11-01").day;
    let kcal = first.kcal.unwrap();
    s.save_preferences(&Preferences {
        effective: "2026-11-01".into(),
        weight: Some(150.0),
        ..Default::default()
    })
    .unwrap();
    let heavier = read(&s, "2026-11-01");
    assert!((heavier.day.kcal.unwrap() - kcal * 2.0).abs() < 1e-9);
    assert!(heavier.sessions.iter().all(|x| x.kcal.is_some()));
    // A replayed capture of a compacted day is not counted twice.
    let events = count(&s, "events");
    walk(&mut s, "pad", "replayed", "2026-11-01T10:00:00Z", 30, 0);
    note(&mut s, "late", "2026-11-02T08:00:00Z");
    assert_eq!(count(&s, "events"), events);
    assert_eq!(read(&s, "2026-11-01").day.steps, first.steps);
}

#[test]
fn live_old_session_and_failures_keep_detail() {
    let mut s = store();
    // No confirmed pause: the session is still live and holds its day.
    let start = at("2026-11-01T10:00:00Z");
    for second in 0..20u32 {
        s.record(
            Event {
                key: &second.to_string(),
                device: "pad",
                connection: "live",
                utc: start + chrono::Duration::seconds(second.into()),
                monotonic_ns: Some(i64::from(second) * 1_000_000_000),
                kind: "notification",
                uuid: "",
                raw: &[],
                detail: "",
            },
            Some(&Sample {
                steps: Some(second * 2),
                elapsed_s: Some(second),
                ..Default::default()
            }),
        )
        .unwrap();
    }
    note(&mut s, "n", "2026-11-01T09:00:00Z");
    assert_eq!(s.compact(at(NOW)).unwrap(), 0);
    assert_eq!(count(&s, "samples"), 20);
    assert_eq!(count(&s, "events"), 21);
    assert_eq!(s.setting("history_compaction").unwrap(), None);

    let mut s = store();
    history(&mut s);
    let before = read(&s, "2026-11-01");
    let events = count(&s, "events");
    s.conn
        .execute_batch("CREATE TRIGGER fail BEFORE INSERT ON session_summaries BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;")
        .unwrap();
    assert!(s.compact(at(NOW)).unwrap_err().contains("synthetic"));
    assert!(
        s.setting("compaction_error")
            .unwrap()
            .unwrap()
            .contains("synthetic")
    );
    assert_eq!(count(&s, "events"), events);
    assert_eq!(count(&s, "day_summaries"), 0);
    assert_eq!(read(&s, "2026-11-01"), before);
    // Not retried until the next local day; success clears the saved error.
    s.conn.execute_batch("DROP TRIGGER fail").unwrap();
    assert_eq!(s.compact(at(NOW)).unwrap(), 0);
    assert_eq!(s.compact(at("2026-11-07T08:00:00Z")).unwrap(), 5);
    assert_eq!(s.setting("compaction_error").unwrap(), None);
    assert_eq!(read(&s, "2026-11-01"), before);
}

#[test]
fn summary_mismatch_is_rejected_before_detail_is_deleted() {
    let mut s = store();
    history(&mut s);
    let events = count(&s, "events");
    // Corrupt the stored value on the way in; read-back verification must notice.
    s.conn
        .execute_batch("CREATE TRIGGER skew AFTER INSERT ON day_summaries BEGIN UPDATE day_summaries SET steps=steps+1 WHERE day=NEW.day; END;")
        .unwrap();
    assert!(s.compact(at(NOW)).unwrap_err().contains("differs"));
    assert_eq!(count(&s, "events"), events);
    assert_eq!(count(&s, "day_summaries"), 0);
}

#[test]
fn file_compaction_shrinks_in_place_leaves_no_other_files_and_reopens() {
    let folder = std::env::temp_dir().join(format!(
        "desksteps-compaction-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let path = folder.join("synthetic.sqlite");
    let names = || {
        let mut names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        history(&mut s);
        // Nothing old enough: no version change.
        assert_eq!(s.compact(at("2026-11-02T12:00:00Z")).unwrap(), 0);
    }
    let large = std::fs::metadata(&path).unwrap().len();
    {
        let mut s = Store::open(&path, chrono_tz::UTC).unwrap();
        assert_eq!(s.compact(at(NOW)).unwrap(), 4);
    }
    assert!(std::fs::metadata(&path).unwrap().len() < large);
    assert_eq!(names(), ["synthetic.sqlite"]);
    let s = Store::open(&path, chrono_tz::UTC).unwrap();
    assert_eq!(count(&s, "day_summaries"), 4);
    drop(s);
    // A connection that cannot write fails before anything is deleted.
    let mut s = Store::open(&folder.join("second.sqlite"), chrono_tz::UTC).unwrap();
    history(&mut s);
    let events = count(&s, "events");
    s.conn.execute_batch("PRAGMA query_only=ON").unwrap();
    assert!(s.compact(at(NOW)).is_err());
    assert_eq!(count(&s, "events"), events);
    assert_eq!(count(&s, "day_summaries"), 0);
    drop(s);
    std::fs::remove_dir_all(folder).unwrap();
}
