//! Black-box executable checks. Every launch is explicit offline replay into
//! synthetic data; no Bluetooth, tray or normal database is opened.
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "desksteps-replay-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, source: &str) -> Output {
        let source_path = self.0.join("synthetic.jsonl");
        fs::write(&source_path, source).unwrap();
        Command::new(env!("CARGO_BIN_EXE_desksteps"))
            .args(["--no-bluetooth", "--replay"])
            .arg(source_path)
            .arg("--database")
            .arg(self.0.join("test.sqlite"))
            .args(["--timezone", "Europe/Helsinki"])
            .output()
            .unwrap()
    }
    fn report(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.join("test.replay-report.json")).unwrap()).unwrap()
    }
    fn connect(&self) -> Connection {
        Connection::open(self.0.join("test.sqlite")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn notification(second: i64, steps: u32, speed: u16) -> Value {
    let mut bytes = vec![0x00, 0x24];
    bytes.extend(speed.to_le_bytes());
    bytes.extend((second as u16).to_le_bytes());
    bytes.extend(&steps.to_le_bytes()[..3]);
    let base = DateTime::parse_from_rfc3339("2026-10-17T20:59:55Z")
        .unwrap()
        .with_timezone(&Utc);
    json!({"kind":"notification","utc":(base+Duration::seconds(second)).to_rfc3339(),"monotonic_ns":second*1_000_000_000,"uuid":"00002acd-0000-1000-8000-00805f9b34fb","hex":bytes.iter().map(|b|format!("{b:02x}")).collect::<String>()})
}
#[test]
fn executable_replay_preserves_raw_packets_day_boundaries_resets_and_idempotence() {
    let f = Fixture::new();
    let mut rows = vec![json!({"kind":"connected","utc":"2026-10-17T20:59:55Z"})];
    for second in 0..20 {
        rows.push(notification(second, 100 + second as u32 * 2, 300));
    }
    rows.push(notification(20, 2, 300));
    rows.push(notification(21, 4, 300));
    rows.push(json!({"kind":"notification","utc":"2026-10-17T21:00:17Z","uuid":"00002acd-0000-1000-8000-00805f9b34fb","hex":"00"}));
    let source = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(f.run(&source).status.success());
    assert_eq!(f.report(), json!({"inserted_events":24,"decode_errors":1}));
    {
        let c = f.connect();
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            24
        );
        assert_eq!(
            c.query_row(
                "SELECT SUM(step_delta) FROM samples WHERE quality='observed'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            38
        );
        assert_eq!(
            c.query_row(
                "SELECT COUNT(*) FROM samples WHERE quality='day_boundary'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            c.query_row(
                "SELECT COUNT(*) FROM samples WHERE quality='counter_reset'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            c.query_row(
                "SELECT raw FROM events WHERE detail LIKE 'Truncated%'",
                [],
                |r| r.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
            vec![0]
        );
    }
    assert!(f.run(&source).status.success());
    assert_eq!(f.report(), json!({"inserted_events":0,"decode_errors":1}));
    assert_eq!(
        f.connect()
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        24
    );
}
#[test]
fn malformed_json_reports_failure_while_preserving_preceding_committed_evidence() {
    let f = Fixture::new();
    let row = json!({"kind":"connected","utc":"2026-10-17T00:00:00Z"});
    let result = f.run(&format!("{row}\ninvalid json"));
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Line 2"));
    assert_eq!(
        f.connect()
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn invalid_hex_is_rejected_and_rows_without_timestamps_are_skipped() {
    let f = Fixture::new();
    assert!(
        !f.run(r#"{"kind":"notification","utc":"2026-10-17T00:00:00Z","hex":"xyz"}"#)
            .status
            .success()
    );
    assert_eq!(
        f.connect()
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(f.run("\n{\"kind\":\"unknown\"}\n").status.success());
    assert_eq!(f.report(), json!({"inserted_events":0,"decode_errors":0}));
}
