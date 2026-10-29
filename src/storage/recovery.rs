//! Recovery decisions and baselines share the source event's transaction.
use super::*;
use crate::statistics::recovery::{Recovery, estimate};
use rusqlite::Transaction;
type Baseline = (String, String, String, Option<i64>, bool);

pub(super) fn record(
    tx: &Transaction<'_>,
    event: &Event<'_>,
    sample: &Sample,
    quality: &str,
    excluded: bool,
    event_id: i64,
) -> rusqlite::Result<Recovery> {
    if quality == "out_of_order" {
        return Ok(Recovery::default());
    }
    // Existing databases can use their last committed accounting anchor on
    // upgrade. This seeds one baseline; it never backfills historical gaps.
    let read = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Baseline> {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    };
    let mut old: Option<Baseline> = tx
        .query_row(
            "SELECT sample,utc,connection,mono,excluded FROM recovery_baselines WHERE device=?1",
            [event.device],
            read,
        )
        .optional()?;
    if old.is_none() {
        old = tx.query_row("SELECT s.decoded,e.utc,e.connection_id,e.monotonic_ns,COALESCE(p.excluded,0)
         FROM anchors a JOIN events e ON e.device=a.device AND e.connection_id=a.connection_id AND e.utc=a.utc
         JOIN samples s ON s.event_id=e.id LEFT JOIN ui_processed p ON p.event_id=e.id
         WHERE a.device=?1 AND e.id<>?2 ORDER BY e.id DESC LIMIT 1",
        params![event.device,event_id],
        read,
    ).optional()?;
    }
    let mut recovery = Recovery::default();
    if let Some((json, utc, connection, mono, prior_excluded)) = old {
        let old_utc = crate::statistics::sessions::stamp(&utc)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let wall = (event.utc - old_utc).num_milliseconds();
        let reversed = wall < 0
            || connection == event.connection
                && mono.zip(event.monotonic_ns).is_some_and(|(a, b)| b < a);
        if reversed {
            return Ok(recovery);
        }
        let gap = connection != event.connection || wall > 5000 || quality == "day_boundary";
        let enabled: bool = tx.query_row(
            "SELECT COALESCE((SELECT value FROM settings WHERE key='recover_gaps'),'true')<>'false'",
            [], |r| r.get(0),
        )?;
        if gap
            && enabled
            && !excluded
            && !prior_excluded
            && !matches!(quality, "counter_reset" | "observed")
        {
            let prior: Sample = serde_json::from_str(&json).map_err(super::sessions::json_error)?;
            recovery = estimate(&prior, sample, wall);
        }
    }
    tx.execute("INSERT INTO recovery_baselines VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(device) DO UPDATE SET sample=excluded.sample,utc=excluded.utc,connection=excluded.connection,mono=excluded.mono,excluded=excluded.excluded",
        params![event.device,serde_json::to_string(sample).map_err(super::sessions::json_error)?,event.utc.to_rfc3339(),event.connection,event.monotonic_ns,excluded])?;
    if recovery.any() {
        tx.execute(
            "INSERT INTO recovery_credits VALUES(?1,?2,?3,?4)",
            params![
                event_id,
                recovery.steps,
                recovery.active_ms,
                recovery.factor
            ],
        )?;
    }
    Ok(recovery)
}
