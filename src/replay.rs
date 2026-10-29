//! Offline JSONL replay. Only explicit test databases are opened.
use crate::{
    protocol,
    storage::{Event, Store},
};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::{
    fs::File,
    io::{BufRead, BufReader},
};
pub(crate) fn replay_file(
    path: &std::path::Path,
    db: &std::path::Path,
    timezone: Tz,
) -> Result<(), String> {
    let source = path.canonicalize().map_err(|e| e.to_string())?;
    let mut store = Store::open(db, timezone)?;
    let reader = BufReader::new(File::open(&source).map_err(|e| e.to_string())?);
    let mut connection = 0;
    let mut inserted = 0;
    let mut errors = 0;
    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(&line).map_err(|e| format!("Line {}: {e}", index + 1))?;
        let kind = value["kind"].as_str().unwrap_or("unknown");
        if kind == "connected" {
            connection += 1;
        }
        let Some(stamp) = value["utc"].as_str() else {
            continue;
        };
        let utc = DateTime::parse_from_rfc3339(stamp)
            .map_err(|e| e.to_string())?
            .with_timezone(&Utc);
        let uuid = value["uuid"].as_str().unwrap_or("");
        let bytes = protocol::unhex(value["hex"].as_str().unwrap_or(""))?;
        let decoded = if uuid == protocol::TREADMILL {
            Some(protocol::decode(&bytes))
        } else {
            None
        };
        if decoded.as_ref().is_some_and(|r| r.is_err()) {
            errors += 1;
        }
        let key = format!("{}:{}", source.display(), index + 1);
        let conn = format!("{}:connection:{connection}", source.display());
        inserted += store.record(
            Event {
                key: &key,
                device: "replay-pad",
                connection: &conn,
                utc,
                monotonic_ns: value["monotonic_ns"].as_i64(),
                kind,
                uuid,
                raw: &bytes,
                detail: &decoded
                    .as_ref()
                    .and_then(|r| r.as_ref().err())
                    .cloned()
                    .unwrap_or_default(),
            },
            decoded.as_ref().and_then(|r| r.as_ref().ok()),
        )? as u32;
    }
    // Deliberately report validation counts, not personal workout totals.
    std::fs::write(
        db.with_extension("replay-report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"inserted_events":inserted,"decode_errors":errors}),
        )
        .unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
