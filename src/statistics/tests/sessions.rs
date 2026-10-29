use super::super::sessions::*;
use crate::{event::Event, protocol::Sample};
use chrono::{DateTime, Utc};
#[derive(Default)]
struct Sink {
    calls: Vec<&'static str>,
    fail: Option<&'static str>,
}
impl Sink {
    fn call(&mut self, name: &'static str) -> Result<(), String> {
        self.calls.push(name);
        if self.fail == Some(name) {
            Err("synthetic projection failure".into())
        } else {
            Ok(())
        }
    }
}
impl Projection for Sink {
    fn finish(&mut self, _: i64, _: &str) -> Result<(), String> {
        self.call("finish")
    }
    fn pause(&mut self, _: i64, _: &str) -> Result<(), String> {
        self.call("pause")
    }
    fn start(&mut self, _: &str, _: &str, _: &str, _: bool) -> Result<i64, String> {
        self.call("start")?;
        Ok(42)
    }
    fn interval(&mut self, _: i64, _: &str, _: i64, _: i64, _: f64) -> Result<(), String> {
        self.call("interval")
    }
    fn walking(&mut self, _: i64, _: &str, _: bool) -> Result<(), String> {
        self.call("walking")
    }
    fn mark_gap(&mut self, _: i64) -> Result<(), String> {
        self.call("gap")
    }
}
fn event() -> Event<'static> {
    Event {
        key: "synthetic",
        device: "pad",
        connection: "a",
        utc: DateTime::parse_from_rfc3339("2026-10-17T12:00:01Z")
            .unwrap()
            .with_timezone(&Utc),
        monotonic_ns: Some(1_000_000_000),
        kind: "notification",
        uuid: "",
        raw: &[],
        detail: "",
    }
}
fn anchor() -> Anchor {
    Anchor {
        connection: "a".into(),
        utc: "2026-10-17T12:00:00Z".into(),
        day: "2026-10-17".into(),
        mono: Some(0),
        speed: Some(3.0),
        ..Default::default()
    }
}
#[test]
fn out_of_order_evidence_never_changes_the_projection() {
    let mut a = anchor();
    let mut e = event();
    e.utc -= chrono::Duration::seconds(2);
    let mut sink = Sink::default();
    assert!(
        !advance(
            &mut a,
            &e,
            None,
            "2026-10-17",
            false,
            Default::default(),
            &mut sink
        )
        .unwrap()
    );
    assert!(sink.calls.is_empty());
}
#[test]
fn malformed_persisted_timestamps_are_errors() {
    let mut a = anchor();
    a.utc = "invalid".into();
    assert!(
        advance(
            &mut a,
            &event(),
            None,
            "2026-10-17",
            false,
            Default::default(),
            &mut Sink::default()
        )
        .is_err()
    );
    let mut a = anchor();
    a.pause = Some("invalid".into());
    assert!(
        advance(
            &mut a,
            &event(),
            None,
            "2026-10-17",
            false,
            Default::default(),
            &mut Sink::default()
        )
        .is_err()
    );
}
#[test]
fn projection_failures_propagate_instead_of_reporting_success() {
    let sample = Sample {
        steps: Some(1),
        speed_kmh: Some(3.0),
        ..Default::default()
    };
    for failure in ["start", "interval", "walking"] {
        let mut sink = Sink {
            fail: Some(failure),
            ..Default::default()
        };
        assert!(
            advance(
                &mut anchor(),
                &event(),
                Some((&sample, 1, "observed")),
                "2026-10-17",
                false,
                Default::default(),
                &mut sink
            )
            .is_err()
        );
        assert_eq!(sink.calls.last(), Some(&failure));
    }
}
#[test]
fn confirmed_pause_ends_at_its_boundary_and_disconnect_ends_at_last_evidence() {
    let mut a = anchor();
    a.session = Some(1);
    a.pause = Some("2026-10-17T11:55:01Z".into());
    let mut sink = Sink::default();
    advance(
        &mut a,
        &event(),
        None,
        "2026-10-17",
        false,
        Default::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(sink.calls, vec!["finish"]);
    assert_eq!(a.session, None);
    // A stop followed by a disconnect ends cleanly at the stop.
    let mut a = anchor();
    a.session = Some(1);
    a.pause = Some("2026-10-17T11:59:01Z".into());
    let mut e = event();
    e.kind = "disconnected";
    let mut sink = Sink::default();
    advance(
        &mut a,
        &e,
        None,
        "2026-10-17",
        false,
        Default::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(sink.calls, vec!["finish"]);
    assert_eq!(a.session, None);
    assert_eq!(a.ended, Some((1, "2026-10-17T11:59:01Z".into())));
    assert!(a.pause.is_none() && !a.gap);
    // Walking cut off by a disconnect ends at the last sample with a gap label.
    let mut a = anchor();
    a.session = Some(1);
    let mut sink = Sink::default();
    advance(
        &mut a,
        &e,
        None,
        "2026-10-17",
        false,
        Default::default(),
        &mut sink,
    )
    .unwrap();
    assert_eq!(sink.calls, vec!["gap", "finish"]);
    assert_eq!(a.ended, Some((1, "2026-10-17T12:00:00Z".into())));
}
#[test]
fn a_session_ended_by_a_lost_link_is_extended_only_by_recovery_or_a_prompt_resume() {
    let sample = Sample {
        steps: Some(1),
        speed_kmh: Some(3.0),
        ..Default::default()
    };
    let run = |ended: &str, recovery: crate::statistics::recovery::Recovery| {
        let mut a = anchor();
        a.connection = "old".into();
        a.ended = Some((7, ended.into()));
        let mut sink = Sink::default();
        advance(
            &mut a,
            &event(),
            Some((&sample, 1, "observed")),
            "2026-10-17",
            false,
            recovery,
            &mut sink,
        )
        .unwrap();
        (a, sink.calls)
    };
    let (a, calls) = run("2026-10-17T11:55:02Z", Default::default());
    assert_eq!(a.session, Some(7));
    assert_eq!(calls[0], "gap");
    assert!(!calls.contains(&"start") && a.ended.is_none());
    let (a, calls) = run("2026-10-17T11:55:01Z", Default::default());
    assert_eq!(a.session, Some(42));
    assert_eq!(calls[0], "start");
    assert!(a.ended.is_none());
    let recovered = crate::statistics::recovery::Recovery {
        steps: 500,
        active_ms: 300_000,
        factor: None,
    };
    let (a, calls) = run("2026-10-17T09:00:00Z", recovered);
    assert_eq!(a.session, Some(7));
    assert!(!calls.contains(&"start"));
}
