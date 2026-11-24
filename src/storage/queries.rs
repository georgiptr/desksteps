//! Read models. SQL remains here rather than in the presentation/domain modules.
use super::compaction::{DayFigures, SessionFigures};
use super::sessions::json_error;
use super::*;
use crate::statistics::sessions::{Anchor, PAUSE_SECONDS, stamp};
use crate::statistics::{Dashboard, Day, Request, Session, date};
use chrono::Days;
impl Store {
    pub fn totals(&self, now: DateTime<Utc>) -> Result<Totals, String> {
        let day = now
            .with_timezone(&self.timezone)
            .format("%Y-%m-%d")
            .to_string();
        let (recorded_steps, uncertain_events, samples) = self.conn.query_row("SELECT
            COALESCE(SUM(CASE WHEN quality='observed' THEN step_delta ELSE 0 END),0)+COALESCE(SUM(r.steps),0),
            COALESCE(SUM(CASE WHEN quality NOT IN ('observed','unchanged') AND (step_delta>0 OR quality IN ('missing_steps','out_of_order')) THEN 1 ELSE 0 END),0), COUNT(*)
            FROM samples s LEFT JOIN recovery_credits r ON r.event_id=s.event_id WHERE local_day=?1", [&day], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e| e.to_string())?;
        Ok(Totals {
            day,
            recorded_steps,
            uncertain_events,
            samples,
        })
    }
    /// Preference-independent figures from retained detail. Weight is applied later,
    /// so a dated weight edited after compaction still changes the estimate.
    pub(super) fn detailed_day(&self, day: &str) -> Result<DayFigures, String> {
        let mut d = Day {
            date: day.into(),
            ..Default::default()
        };
        (d.steps,d.samples,d.uncertain) = self.conn.query_row("SELECT COALESCE(SUM(CASE WHEN s.quality='observed' THEN s.step_delta ELSE 0 END),0),COUNT(*),COALESCE(SUM(CASE WHEN s.quality NOT IN ('observed','unchanged') AND (s.step_delta>0 OR s.quality IN ('missing_steps','out_of_order')) THEN 1 ELSE 0 END),0) FROM samples s LEFT JOIN ui_processed p ON p.event_id=s.event_id WHERE s.local_day=?1 AND COALESCE(p.excluded,0)=0",[day],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        let mapped: i64;
        (mapped,d.active_ms,d.factor,d.sessions) = self.conn.query_row("SELECT COALESCE(SUM(steps),0),COALESCE(SUM(active_ms),0),COALESCE(SUM(factor),0),COUNT(DISTINCT session) FROM ui_intervals WHERE day=?1",[day],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
        let recovered_factor: Option<f64>;
        (d.recovered_steps,d.recovered_ms,recovered_factor,d.recovery_kcal_missing) = self.conn.query_row("SELECT COALESCE(SUM(r.steps),0),COALESCE(SUM(r.active_ms),0),SUM(r.factor),COALESCE(MAX(r.factor IS NULL AND r.event_id IS NOT NULL),0) FROM recovery_credits r JOIN samples s ON s.event_id=r.event_id WHERE s.local_day=?1",[day],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
        d.steps += d.recovered_steps;
        d.unassigned = d.steps - mapped;
        let estimated_ms = self.conn.query_row("SELECT COALESCE(SUM(i.active_ms),0) FROM ui_intervals i LEFT JOIN recovery_credits r ON r.event_id=i.event_id WHERE i.day=?1 AND (r.event_id IS NULL OR r.factor IS NOT NULL)",[day],|r|r.get(0)).map_err(|e|e.to_string())?;
        d.partial_sessions = self.conn.query_row("SELECT COUNT(DISTINCT i.session) FROM ui_intervals i JOIN ui_sessions s ON s.id=i.session WHERE i.day=?1 AND s.gap=1",[day],|r|r.get(0)).map_err(|e|e.to_string())?;
        d.pad_calories = self.pad_calories(day, None)?;
        Ok(DayFigures {
            day: d,
            recovered_factor,
            estimated_ms,
        })
    }
    /// Sessions of one day from retained detail, before weight is applied.
    pub(super) fn detailed_sessions(&self, day: &str) -> Result<Vec<SessionFigures>, String> {
        let mut sessions: Vec<SessionFigures> = self.conn.prepare("SELECT s.id,s.start,s.end,s.state,SUM(i.steps),SUM(i.active_ms),SUM(i.factor),s.gap,COALESCE(SUM(r.steps),0),COALESCE(SUM(r.active_ms),0),SUM(r.factor),COALESCE(MAX(r.factor IS NULL AND r.event_id IS NOT NULL),0),SUM(CASE WHEN r.event_id IS NULL OR r.factor IS NOT NULL THEN i.active_ms ELSE 0 END) FROM ui_intervals i JOIN ui_sessions s ON s.id=i.session LEFT JOIN recovery_credits r ON r.event_id=i.event_id WHERE i.day=?1 GROUP BY s.id ORDER BY s.start DESC").map_err(|e|e.to_string())?.query_map([day],|r|Ok(SessionFigures{session:Session{id:r.get(0)?,start:r.get(1)?,end:r.get(2)?,state:r.get(3)?,steps:r.get(4)?,active_ms:r.get(5)?,gap:r.get(7)?,recovered_steps:r.get(8)?,recovered_ms:r.get(9)?,recovery_kcal_missing:r.get(11)?,..Default::default()},factor:r.get(6)?,recovered_factor:r.get(10)?,estimated_ms:r.get(12)?})).map_err(|e|e.to_string())?.collect::<rusqlite::Result<_>>().map_err(|e|e.to_string())?;
        for s in &mut sessions {
            s.session.pad_calories = self.pad_calories(day, Some(s.session.id))?;
        }
        Ok(sessions)
    }
    fn day_summary(&self, day: &str) -> Result<Day, String> {
        let figures = match self.frozen_day(day)? {
            Some(frozen) => frozen,
            None => self.detailed_day(day)?,
        };
        let mut d = figures.day;
        d.preferences = self.preferences(day)?;
        d.recovered_kcal = figures
            .recovered_factor
            .zip(d.preferences.weight)
            .map(|(factor, weight)| factor * weight);
        d.kcal = if d.samples > 0 && (d.steps == 0 && d.active_ms == 0 || figures.estimated_ms > 0)
        {
            d.preferences.weight.map(|w| w * d.factor)
        } else {
            None
        };
        Ok(d)
    }
    pub fn dashboard(&self, request: &Request, now: DateTime<Utc>) -> Result<Dashboard, String> {
        self.expire_sessions(now)?;
        let selected = date(&request.day)?;
        let today = now
            .with_timezone(&self.timezone)
            .format("%Y-%m-%d")
            .to_string();
        let mut days = Vec::new();
        for back in (0..request.range.clamp(1, crate::statistics::MAX_RANGE)).rev() {
            let day = selected
                .checked_sub_days(Days::new(back.into()))
                .ok_or("Date out of range")?;
            days.push(self.day_summary(&day.to_string())?);
        }
        let day = days.last().unwrap().clone();
        // A compacted day has no live anchors; its saved session states are final.
        let frozen = self.is_frozen(&request.day)?;
        let figures = if frozen {
            self.frozen_sessions(&request.day)?
        } else {
            self.detailed_sessions(&request.day)?
        };
        let weight = day.preferences.weight;
        let mut sessions: Vec<Session> = figures
            .into_iter()
            .map(|f| Session {
                kcal: (f.estimated_ms > 0)
                    .then_some(weight.map(|w| f.factor * w))
                    .flatten(),
                recovered_kcal: f.recovered_factor.zip(weight).map(|(r, w)| r * w),
                ..f.session
            })
            .collect();
        let anchors: Vec<Anchor> = self
            .conn
            .prepare("SELECT value FROM ui_anchors")
            .map_err(|e| e.to_string())?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .map(|r| serde_json::from_str(&r?).map_err(json_error))
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;
        let mut current = None;
        for s in sessions.iter_mut().filter(|_| !frozen) {
            if let Some(a) = anchors.iter().find(|a| a.session == Some(s.id)) {
                if let Some(p) = &a.pause {
                    if (now - stamp(p).map_err(|e| e.to_string())?).num_seconds() >= PAUSE_SECONDS {
                        s.state = "Finished".into();
                    }
                } else if a.gap {
                    s.state = "Continuity uncertain".into();
                } else if (now - stamp(&a.utc).map_err(|e| e.to_string())?).num_seconds() > 5 {
                    s.state = "Awaiting fresh data".into();
                }
                if s.state != "Finished" && request.day == today {
                    current = Some(s.clone());
                }
            }
        }
        Ok(Dashboard {
            day,
            days,
            sessions,
            current,
            today,
            timezone: self.timezone.name().into(),
        })
    }
}
