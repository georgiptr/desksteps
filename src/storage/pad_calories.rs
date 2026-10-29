//! Independent pad-counter projection. Raw decoded samples remain authoritative.
use super::*;
use crate::statistics::PadCalories;
use rusqlite::Transaction;

struct Reading {
    sample: Sample,
    device: String,
    connection: String,
    utc: String,
    mono: Option<i64>,
    day: String,
    excluded: bool,
    recovered: bool,
    quality: String,
}
fn reading(row: &rusqlite::Row<'_>) -> rusqlite::Result<Reading> {
    let json: String = row.get(0)?;
    Ok(Reading {
        sample: serde_json::from_str(&json).map_err(super::sessions::json_error)?,
        device: row.get(1)?,
        connection: row.get(2)?,
        utc: row.get(3)?,
        mono: row.get(4)?,
        day: row.get(5)?,
        excluded: row.get(6)?,
        recovered: row.get(7)?,
        quality: row.get(8)?,
    })
}
const READ: &str = "SELECT s.decoded,e.device,e.connection_id,e.utc,e.monotonic_ns,s.local_day,
    COALESCE(p.excluded,0),r.event_id IS NOT NULL,s.quality
    FROM samples s JOIN events e ON e.id=s.event_id
    LEFT JOIN ui_processed p ON p.event_id=e.id LEFT JOIN recovery_credits r ON r.event_id=e.id
    LEFT JOIN pad_calories c ON c.event_id=e.id";

pub(super) fn record(tx: &Transaction<'_>, id: i64) -> rusqlite::Result<()> {
    let new = tx.query_row(&format!("{READ} WHERE e.id=?1"), [id], reading)?;
    let old = tx.query_row(&format!("{READ} WHERE e.device=?1 AND e.id<?2 AND s.quality<>'out_of_order' AND COALESCE(c.baseline,1)=1 ORDER BY e.id DESC LIMIT 1"),
        params![new.device,id], reading).optional()?;
    let mut kcal = None;
    let mut baseline = new.quality != "out_of_order";
    if !new.excluded && new.quality != "out_of_order" {
        if let Some(old) = old.filter(|old| !old.excluded) {
            let wall = (crate::statistics::sessions::stamp(&new.utc).map_err(time_error)?
                - crate::statistics::sessions::stamp(&old.utc).map_err(time_error)?)
            .num_milliseconds();
            let ordered = wall >= 0
                && (old.connection != new.connection
                    || old.mono.zip(new.mono).is_none_or(|(a, b)| b >= a));
            baseline = ordered;
            let reset = old
                .sample
                .steps
                .zip(new.sample.steps)
                .is_some_and(|(a, b)| b < a)
                || old
                    .sample
                    .elapsed_s
                    .zip(new.sample.elapsed_s)
                    .is_some_and(|(a, b)| b < a)
                || old
                    .sample
                    .distance_m
                    .zip(new.sample.distance_m)
                    .is_some_and(|(a, b)| b < a);
            let close = old.connection == new.connection
                && old.day == new.day
                && wall <= 5000
                && old
                    .mono
                    .zip(new.mono)
                    .is_none_or(|(a, b)| b - a <= 5_000_000_000);
            let unchanged = old.sample.calories_kcal.is_some()
                && old.sample.calories_kcal == new.sample.calories_kcal;
            if ordered && !reset && (close || new.recovered || unchanged) {
                kcal = old
                    .sample
                    .calories_kcal
                    .zip(new.sample.calories_kcal)
                    .and_then(|(a, b)| b.checked_sub(a));
            }
        } else if new.sample.calories_kcal == Some(0) {
            // A known zero is useful; a nonzero initial workout has unknown attribution.
            kcal = Some(0);
        }
    }
    let incomplete = !new.excluded && baseline && kcal.is_none();
    tx.execute(
        "INSERT INTO pad_calories VALUES(?1,?2,?3,?4,?5)",
        params![id, kcal, new.recovered, incomplete, baseline],
    )?;
    Ok(())
}
fn time_error(e: chrono::ParseError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}
impl Store {
    /// Adds only missing projections, using saved recovery decisions, never today's toggle.
    pub(super) fn backfill_pad_calories(&mut self) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let ids = tx.prepare("SELECT s.event_id FROM samples s WHERE NOT EXISTS(SELECT 1 FROM pad_calories c WHERE c.event_id=s.event_id) ORDER BY s.event_id")
            .map_err(|e|e.to_string())?.query_map([], |r| r.get::<_,i64>(0))
            .map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;
        for id in ids {
            record(&tx, id).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub(super) fn pad_calories(
        &self,
        day: &str,
        session: Option<i64>,
    ) -> Result<PadCalories, String> {
        // Exact interval attribution includes recovery at its receipt date. Calorie ticks
        // between step increments belong to the containing session (or its live anchor).
        let sql = "SELECT SUM(c.kcal),COALESCE(MAX(c.incomplete),0),
            SUM(CASE WHEN c.recovered THEN c.kcal END),COALESCE(MAX(c.recovered AND c.kcal IS NULL),0)
            FROM pad_calories c JOIN samples s ON s.event_id=c.event_id JOIN events e ON e.id=c.event_id
            LEFT JOIN ui_processed p ON p.event_id=e.id LEFT JOIN ui_intervals i ON i.event_id=e.id
            WHERE s.local_day=?1 AND COALESCE(p.excluded,0)=0 AND (?2 IS NULL OR ?2=COALESCE(i.session,
                (SELECT u.id FROM ui_sessions u WHERE u.device=e.device
                    AND julianday(e.utc)>=julianday(u.start) AND julianday(e.utc)<=julianday(u.end)
                    ORDER BY u.start DESC,u.id DESC LIMIT 1),
                (SELECT u.id FROM ui_anchors a JOIN ui_sessions u ON u.id=json_extract(a.value,'$.session')
                    WHERE a.device=e.device AND json_extract(a.value,'$.connection')=e.connection_id
                    AND julianday(e.utc)>=julianday(u.start)) ))";
        self.conn
            .query_row(sql, params![day, session], |r| {
                Ok(PadCalories {
                    total: r.get(0)?,
                    incomplete: r.get(1)?,
                    recovered: r.get(2)?,
                    recovery_missing: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())
    }
}
