//! Derived session persistence; event writes participate in recording's transaction.
use super::*;
use crate::statistics::MODEL;
use crate::statistics::sessions::{Anchor, PAUSE_SECONDS, stamp};
use rusqlite::Transaction;
pub(super) fn json_error(e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}
pub(super) fn record(
    tx: &Transaction<'_>,
    event: &Event<'_>,
    sample: Option<(&Sample, i64, &str)>,
    day: &str,
) -> rusqlite::Result<()> {
    let id = tx.last_insert_rowid();
    let process = event.connection.split(':').next().unwrap_or("");
    if event.kind == "recorder_started" {
        tx.execute(
            "INSERT OR REPLACE INTO ui_runs VALUES(?1,?2)",
            params![process, event.detail.contains("empty-belt")],
        )?;
    }
    let excluded: bool = tx
        .query_row(
            "SELECT excluded FROM ui_runs WHERE process=?1",
            [process],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false);
    tx.execute(
        "INSERT INTO ui_processed VALUES(?1,?2)",
        params![id, excluded],
    )?;
    let stored: Option<String> = tx
        .query_row(
            "SELECT value FROM ui_anchors WHERE device=?1",
            [event.device],
            |r| r.get(0),
        )
        .optional()?;
    let mut a: Anchor = stored
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(json_error)?
        .unwrap_or_default();
    let mut projection = SqlProjection {
        tx,
        event_id: id,
        matched: None,
    };
    let recovery = if let Some((s, _, quality)) = sample {
        super::recovery::record(tx, event, s, quality, excluded, id)?
    } else {
        crate::statistics::recovery::Recovery::default()
    };
    if !crate::statistics::sessions::advance(
        &mut a,
        event,
        sample,
        day,
        excluded,
        recovery,
        &mut projection,
    )
    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e))))?
    {
        return Ok(());
    }
    tx.execute("INSERT INTO ui_anchors VALUES(?1,?2) ON CONFLICT(device) DO UPDATE SET value=excluded.value",params![event.device,serde_json::to_string(&a).map_err(json_error)?])?;
    Ok(())
}
impl Store {
    pub fn expire_sessions(&self, now: DateTime<Utc>) -> Result<(), String> {
        let mut expired = Vec::new();
        let mut statement = self
            .conn
            .prepare("SELECT device,value FROM ui_anchors")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (device, value) = row.map_err(|e| e.to_string())?;
            let mut a: Anchor = serde_json::from_str(&value).map_err(|e| e.to_string())?;
            if let Some(pause) = a.pause.clone()
                && (now - stamp(&pause).map_err(|e| e.to_string())?).num_seconds() >= PAUSE_SECONDS
                && let Some(session) = a.session.take()
            {
                a.pause = None;
                expired.push((device, a, session, pause));
            }
        }
        if expired.is_empty() {
            return Ok(());
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        for (device, a, session, pause) in expired {
            tx.execute(
                "UPDATE ui_sessions SET state='Finished',end=?1 WHERE id=?2",
                params![pause, session],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE ui_anchors SET value=?1 WHERE device=?2",
                params![
                    serde_json::to_string(&a).map_err(|e| e.to_string())?,
                    device
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
}
/// Session rules version; databases projected under older rules are rebuilt once.
const SESSION_RULES: &str = "confirmed-pause-restart-v2";
struct Replayed {
    id: i64,
    key: String,
    device: String,
    connection: String,
    mono: Option<i64>,
    utc: String,
    kind: String,
    uuid: String,
    raw: Vec<u8>,
    detail: String,
    excluded: bool,
    /// Decoded JSON, step delta, quality and local day.
    sample: Option<(String, i64, String, String)>,
    credit: Option<(i64, i64, Option<f64>)>,
}
impl Store {
    /// Replays processed events through the current session rules. Only session
    /// grouping/coverage may change: every stored interval must be reproduced exactly or
    /// the rebuild rolls back and the earlier projection stays in place.
    pub(crate) fn rebuild_sessions(&mut self) -> Result<(), String> {
        if self.setting("session_rules")?.as_deref().is_some_and(|v| {
            v == SESSION_RULES
                || v.starts_with(&format!("{SESSION_RULES}: kept earlier sessions: "))
        }) {
            return Ok(());
        }
        let result = self.replay_sessions();
        let outcome = match &result {
            Ok(()) => SESSION_RULES.to_string(),
            Err(e) => format!("{SESSION_RULES}: kept earlier sessions: {e}"),
        };
        self.set_setting("session_rules", &outcome)
    }
    fn replay_sessions(&mut self) -> Result<(), String> {
        let zone = self.timezone;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let rows: Vec<Replayed> = tx
            .prepare("SELECT e.id,e.source_key,e.device,e.connection_id,e.monotonic_ns,e.utc,e.kind,e.uuid,e.raw,e.detail,p.excluded,
                s.decoded,s.step_delta,s.quality,s.local_day,r.steps,r.active_ms,r.factor,s.event_id IS NOT NULL,r.event_id IS NOT NULL
                FROM events e JOIN ui_processed p ON p.event_id=e.id LEFT JOIN samples s ON s.event_id=e.id
                LEFT JOIN recovery_credits r ON r.event_id=e.id ORDER BY e.id")
            .map_err(|e| e.to_string())?
            .query_map([], |r| {
                let sample = r.get::<_, bool>(18)?;
                let credit = r.get::<_, bool>(19)?;
                Ok(Replayed {
                    id: r.get(0)?,
                    key: r.get(1)?,
                    device: r.get(2)?,
                    connection: r.get(3)?,
                    mono: r.get(4)?,
                    utc: r.get(5)?,
                    kind: r.get(6)?,
                    uuid: r.get(7)?,
                    raw: r.get(8)?,
                    detail: r.get(9)?,
                    excluded: r.get(10)?,
                    sample: if sample {
                        Some((r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?))
                    } else {
                        None
                    },
                    credit: if credit {
                        Some((r.get(15)?, r.get(16)?, r.get(17)?))
                    } else {
                        None
                    },
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;
        tx.execute_batch(
            "PRAGMA defer_foreign_keys=ON; DELETE FROM ui_sessions; DELETE FROM ui_anchors;",
        )
        .map_err(|e| e.to_string())?;
        let mut anchors = std::collections::BTreeMap::<String, Anchor>::new();
        let mut matched = 0;
        for row in rows {
            let Replayed {
                id,
                key,
                device,
                connection,
                mono,
                utc,
                kind,
                uuid,
                raw,
                detail,
                excluded,
                sample,
                credit,
            } = row;
            let time = stamp(&utc).map_err(|e| e.to_string())?;
            let event = Event {
                key: &key,
                device: &device,
                connection: &connection,
                utc: time,
                monotonic_ns: mono,
                kind: &kind,
                uuid: &uuid,
                raw: &raw,
                detail: &detail,
            };
            let decoded = sample
                .as_ref()
                .map(|(json, delta, quality, day)| {
                    serde_json::from_str::<Sample>(json)
                        .map(|s| (s, *delta, quality.as_str(), day.clone()))
                })
                .transpose()
                .map_err(|e| e.to_string())?;
            let day = decoded.as_ref().map_or_else(
                || time.with_timezone(&zone).format("%Y-%m-%d").to_string(),
                |d| d.3.clone(),
            );
            let recovery = credit.map_or_else(Default::default, |(steps, active_ms, factor)| {
                crate::statistics::recovery::Recovery {
                    steps,
                    active_ms,
                    factor,
                }
            });
            let mut a = anchors.get(&device).cloned().unwrap_or_default();
            let mut projection = SqlProjection {
                tx: &tx,
                event_id: id,
                matched: Some(0),
            };
            let advanced = crate::statistics::sessions::advance(
                &mut a,
                &event,
                decoded
                    .as_ref()
                    .map(|(s, delta, quality, _)| (s, *delta, *quality)),
                &day,
                excluded,
                recovery,
                &mut projection,
            )?;
            matched += projection.matched.unwrap_or(0);
            if advanced {
                anchors.insert(device, a);
            }
        }
        let total: usize = tx
            .query_row("SELECT COUNT(*) FROM ui_intervals", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if matched != total {
            return Err(format!("{matched} of {total} intervals reproduced"));
        }
        for (device, a) in anchors {
            tx.execute(
                "INSERT INTO ui_anchors VALUES(?1,?2)",
                params![
                    device,
                    serde_json::to_string(&a).map_err(|e| e.to_string())?
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
}

struct SqlProjection<'a, 'c> {
    tx: &'a Transaction<'c>,
    event_id: i64,
    /// While rebuilding, intervals already exist: they are matched and
    /// reassigned instead of inserted, and counted.
    matched: Option<usize>,
}
impl crate::statistics::sessions::Projection for SqlProjection<'_, '_> {
    fn finish(&mut self, session: i64, end: &str) -> Result<(), String> {
        self.tx
            .execute(
                "UPDATE ui_sessions SET state='Finished',end=?1 WHERE id=?2",
                params![end, session],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn pause(&mut self, session: i64, end: &str) -> Result<(), String> {
        self.tx
            .execute(
                "UPDATE ui_sessions SET end=?1,state='Paused' WHERE id=?2",
                params![end, session],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn start(&mut self, device: &str, start: &str, end: &str, gap: bool) -> Result<i64, String> {
        self.tx
            .execute(
                "INSERT INTO ui_sessions(device,start,end,state,gap) VALUES(?1,?2,?3,'Walking',?4)",
                params![device, start, end, gap],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.tx.last_insert_rowid())
    }
    fn interval(
        &mut self,
        session: i64,
        day: &str,
        delta: i64,
        active_ms: i64,
        factor: f64,
    ) -> Result<(), String> {
        if let Some(matched) = &mut self.matched {
            let updated = self
                .tx
                .execute(
                    "UPDATE ui_intervals SET session=?2 WHERE event_id=?1 AND day=?3 AND steps=?4 AND active_ms=?5 AND ABS(factor-?6)<1e-9",
                    params![self.event_id, session, day, delta, active_ms, factor],
                )
                .map_err(|e| e.to_string())?;
            if updated != 1 {
                return Err(format!("interval {} differs on rebuild", self.event_id));
            }
            *matched += 1;
            return Ok(());
        }
        self.tx
            .execute(
                "INSERT INTO ui_intervals VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![self.event_id, session, day, delta, active_ms, factor, MODEL],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn walking(&mut self, session: i64, end: &str, gap: bool) -> Result<(), String> {
        self.tx
            .execute(
                "UPDATE ui_sessions SET end=?1,state='Walking',gap=MAX(gap,?2) WHERE id=?3",
                params![end, gap, session],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn mark_gap(&mut self, session: i64) -> Result<(), String> {
        self.tx
            .execute("UPDATE ui_sessions SET gap=1 WHERE id=?1", [session])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
