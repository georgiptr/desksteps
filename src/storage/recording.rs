//! Raw event, sample, accounting anchor and session projection commit together.
use super::*;
impl Store {
    /// Raw event, decoded sample, increment, and restart anchor commit together.
    /// Uncertain increments stay stored but are excluded from recorded daily steps.
    pub fn record(&mut self, event: Event<'_>, sample: Option<&Sample>) -> Result<bool, String> {
        let day = event
            .utc
            .with_timezone(&self.timezone)
            .format("%Y-%m-%d")
            .to_string();
        let timestamp = event.utc.to_rfc3339();
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let inserted = tx.execute("INSERT OR IGNORE INTO events(source_key,device,connection_id,utc,monotonic_ns,kind,uuid,raw,detail,timezone)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![event.key,event.device,event.connection,timestamp,event.monotonic_ns,event.kind,event.uuid,event.raw,event.detail,self.timezone.name()]).map_err(|e| e.to_string())?;
        if inserted == 0 {
            return Ok(false);
        }
        let id = tx.last_insert_rowid();
        if let Some(sample) = sample {
            let prior = if sample.steps.is_some() {
                tx.query_row("SELECT connection_id,utc,monotonic_ns,steps,elapsed_s,local_day FROM anchors WHERE device=?1", [event.device], |r| {
                    let stamp: String = r.get(1)?;
                    let utc = DateTime::parse_from_rfc3339(&stamp)
                        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1,rusqlite::types::Type::Text,Box::new(e)))?.with_timezone(&Utc);
                    Ok(crate::accounting::Anchor {connection:r.get(0)?,utc,monotonic_ns:r.get(2)?,steps:r.get(3)?,elapsed_s:r.get(4)?,day:r.get(5)?})
                }).optional().map_err(|e| e.to_string())?
            } else {
                None
            };
            let decision = crate::accounting::decide(
                sample,
                event.connection,
                event.utc,
                event.monotonic_ns,
                &day,
                prior.as_ref(),
            );
            let (delta, quality) = (decision.delta, decision.quality);
            tx.execute(
                "INSERT INTO samples VALUES(?1,?2,?3,?4,?5)",
                params![
                    id,
                    serde_json::to_string(sample).map_err(|e| e.to_string())?,
                    delta,
                    quality,
                    day
                ],
            )
            .map_err(|e| e.to_string())?;
            super::sessions::record(&tx, &event, Some((sample, delta, quality)), &day)
                .map_err(|e| e.to_string())?;
            super::pad_calories::record(&tx, id).map_err(|e| e.to_string())?;
            if decision.update_anchor {
                tx.execute("INSERT INTO anchors VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(device) DO UPDATE SET connection_id=excluded.connection_id,utc=excluded.utc,monotonic_ns=excluded.monotonic_ns,steps=excluded.steps,elapsed_s=excluded.elapsed_s,local_day=excluded.local_day",
                    params![event.device,event.connection,timestamp,event.monotonic_ns,sample.steps,sample.elapsed_s,day]).map_err(|e| e.to_string())?;
            }
        } else {
            super::sessions::record(&tx, &event, None, &day).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }
}
