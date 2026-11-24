//! History compaction. Today and yesterday keep full raw detail for diagnosis;
//! each older day keeps only its preference-independent summary.
//!
//! A day is summarized, read back and compared exactly before its detail is
//! deleted, all in one transaction. Weight stays separate, so estimates still
//! follow dated weight edits. The newest calorie baseline and accounting-anchor
//! sample per device are retained so the next recording decides exactly as it
//! would have with full history.
use super::*;
use crate::statistics::{Day, PadCalories, Session};
use chrono::{Days, NaiveDate};

/// Local days, counting today, that keep complete raw evidence.
pub const DETAIL_DAYS: u64 = 2;
/// Older DeskSteps versions reject this version instead of showing empty history.
pub(crate) const COMPACTED_VERSION: i64 = 2;
const MARKER: &str = "history_compaction";

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DayFigures {
    pub(crate) day: Day,
    pub(crate) recovered_factor: Option<f64>,
    pub(crate) estimated_ms: i64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SessionFigures {
    pub(crate) session: Session,
    pub(crate) factor: f64,
    pub(crate) recovered_factor: Option<f64>,
    pub(crate) estimated_ms: i64,
}

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS day_summaries(day TEXT PRIMARY KEY,
        steps INTEGER NOT NULL, active_ms INTEGER NOT NULL, factor REAL NOT NULL, estimated_ms INTEGER NOT NULL,
        sessions INTEGER NOT NULL, samples INTEGER NOT NULL, uncertain INTEGER NOT NULL, unassigned INTEGER NOT NULL,
        partial_sessions INTEGER NOT NULL, recovered_steps INTEGER NOT NULL, recovered_ms INTEGER NOT NULL,
        recovered_factor REAL, recovery_kcal_missing INTEGER NOT NULL, pad_total REAL, pad_incomplete INTEGER NOT NULL,
        pad_recovered REAL, pad_recovery_missing INTEGER NOT NULL) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS session_summaries(day TEXT NOT NULL, session INTEGER NOT NULL,
        start TEXT NOT NULL, end TEXT NOT NULL, state TEXT NOT NULL, gap INTEGER NOT NULL,
        steps INTEGER NOT NULL, active_ms INTEGER NOT NULL, factor REAL NOT NULL, estimated_ms INTEGER NOT NULL,
        recovered_steps INTEGER NOT NULL, recovered_ms INTEGER NOT NULL, recovered_factor REAL,
        recovery_kcal_missing INTEGER NOT NULL, pad_total REAL, pad_incomplete INTEGER NOT NULL,
        pad_recovered REAL, pad_recovery_missing INTEGER NOT NULL, PRIMARY KEY(day,session)) WITHOUT ROWID;";

fn pad(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<PadCalories> {
    Ok(PadCalories {
        total: r.get(at)?,
        incomplete: r.get(at + 1)?,
        recovered: r.get(at + 2)?,
        recovery_missing: r.get(at + 3)?,
    })
}
fn sql(e: rusqlite::Error) -> String {
    e.to_string()
}

impl Store {
    pub(super) fn is_frozen(&self, day: &str) -> Result<bool, String> {
        self.conn
            .query_row(
                "SELECT 1 FROM day_summaries WHERE day=?1",
                [day],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
            .map_err(sql)
    }
    pub(super) fn frozen_day(&self, day: &str) -> Result<Option<DayFigures>, String> {
        self.conn
            .query_row(
                "SELECT steps,active_ms,factor,estimated_ms,sessions,samples,uncertain,unassigned,
                partial_sessions,recovered_steps,recovered_ms,recovered_factor,recovery_kcal_missing,
                pad_total,pad_incomplete,pad_recovered,pad_recovery_missing FROM day_summaries WHERE day=?1",
                [day],
                |r| {
                    Ok(DayFigures {
                        day: Day {
                            date: day.into(),
                            steps: r.get(0)?,
                            active_ms: r.get(1)?,
                            factor: r.get(2)?,
                            sessions: r.get(4)?,
                            samples: r.get(5)?,
                            uncertain: r.get(6)?,
                            unassigned: r.get(7)?,
                            partial_sessions: r.get(8)?,
                            recovered_steps: r.get(9)?,
                            recovered_ms: r.get(10)?,
                            recovery_kcal_missing: r.get(12)?,
                            pad_calories: pad(r, 13)?,
                            ..Default::default()
                        },
                        recovered_factor: r.get(11)?,
                        estimated_ms: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(sql)
    }
    pub(super) fn frozen_sessions(&self, day: &str) -> Result<Vec<SessionFigures>, String> {
        self.conn
            .prepare(
                "SELECT session,start,end,state,gap,steps,active_ms,factor,estimated_ms,recovered_steps,
                recovered_ms,recovered_factor,recovery_kcal_missing,pad_total,pad_incomplete,pad_recovered,
                pad_recovery_missing FROM session_summaries WHERE day=?1 ORDER BY start DESC,session DESC",
            )
            .map_err(sql)?
            .query_map([day], |r| {
                Ok(SessionFigures {
                    session: Session {
                        id: r.get(0)?,
                        start: r.get(1)?,
                        end: r.get(2)?,
                        state: r.get(3)?,
                        gap: r.get(4)?,
                        steps: r.get(5)?,
                        active_ms: r.get(6)?,
                        recovered_steps: r.get(9)?,
                        recovered_ms: r.get(10)?,
                        recovery_kcal_missing: r.get(12)?,
                        pad_calories: pad(r, 13)?,
                        ..Default::default()
                    },
                    factor: r.get(7)?,
                    recovered_factor: r.get(11)?,
                    estimated_ms: r.get(8)?,
                })
            })
            .map_err(sql)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql)
    }
    fn freeze(&self, day: &str) -> Result<(), String> {
        let f = self.detailed_day(day)?;
        let mut sessions = self.detailed_sessions(day)?;
        let (d, p) = (&f.day, &f.day.pad_calories);
        self.conn
            .execute(
                "INSERT INTO day_summaries VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    day, d.steps, d.active_ms, d.factor, f.estimated_ms, d.sessions, d.samples,
                    d.uncertain, d.unassigned, d.partial_sessions, d.recovered_steps, d.recovered_ms,
                    f.recovered_factor, d.recovery_kcal_missing, p.total, p.incomplete, p.recovered,
                    p.recovery_missing
                ],
            )
            .map_err(sql)?;
        for s in &sessions {
            let (x, p) = (&s.session, &s.session.pad_calories);
            self.conn
                .execute(
                    "INSERT INTO session_summaries VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                    params![
                        day, x.id, x.start, x.end, x.state, x.gap, x.steps, x.active_ms, s.factor,
                        s.estimated_ms, x.recovered_steps, x.recovered_ms, s.recovered_factor,
                        x.recovery_kcal_missing, p.total, p.incomplete, p.recovered, p.recovery_missing
                    ],
                )
                .map_err(sql)?;
        }
        // Detail order between equal start times is unspecified; the summary's is fixed.
        sessions.sort_by(|a, b| {
            (&b.session.start, b.session.id).cmp(&(&a.session.start, a.session.id))
        });
        if self.frozen_day(day)?.as_ref() != Some(&f) || self.frozen_sessions(day)? != sessions {
            return Err(format!("{day} summary differs from its detail"));
        }
        Ok(())
    }
    /// Compacts closed days once per local day. A failure keeps all detail,
    /// is remembered in settings and is retried on the next day or launch.
    pub fn compact(&mut self, now: DateTime<Utc>) -> Result<usize, String> {
        let today = now.with_timezone(&self.timezone).date_naive();
        if self.compacted_for == Some(today) {
            return Ok(0);
        }
        self.compacted_for = Some(today);
        let result = self.compact_before(today);
        if let Err(e) = &result {
            let _ = self.set_setting("compaction_error", &format!("{today}: {e}"));
        }
        result
    }
    fn compact_before(&mut self, today: NaiveDate) -> Result<usize, String> {
        let first = today
            .checked_sub_days(Days::new(DETAIL_DAYS - 1))
            .ok_or("Date out of range")?
            .to_string();
        let live = "SELECT json_extract(value,'$.session') FROM ui_anchors WHERE json_extract(value,'$.session') IS NOT NULL";
        // A session still running from an older day keeps that day detailed. A
        // session ended by a lost link keeps its row, because recovered steps can
        // extend it; its compacted days keep the summary saved here.
        let held: Vec<String> = self.strings(
            &format!("SELECT DISTINCT day FROM ui_intervals WHERE day<?1 AND session IN ({live})"),
            &first,
        )?;
        let days: Vec<String> = self
            .strings("SELECT DISTINCT local_day FROM samples WHERE local_day<?1 AND local_day NOT IN (SELECT day FROM day_summaries) ORDER BY local_day", &first)?
            .into_iter()
            .filter(|d| !held.contains(d))
            .collect();
        let zone = self.timezone;
        let loose: Vec<i64> = self
            .conn
            .prepare("SELECT e.id,e.utc FROM events e WHERE NOT EXISTS(SELECT 1 FROM samples s WHERE s.event_id=e.id)")
            .map_err(sql)?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql)?
            .into_iter()
            .filter_map(|(id, utc)| {
                let day = crate::statistics::sessions::stamp(&utc)
                    .ok()?
                    .with_timezone(&zone)
                    .date_naive()
                    .to_string();
                (day < first && !held.contains(&day)).then_some(id)
            })
            .collect();
        let residual: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM samples s JOIN day_summaries d ON d.day=s.local_day)",
                [],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if days.is_empty() && loose.is_empty() && !residual {
            return Ok(0);
        }
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        for day in &days {
            self.freeze(day)?;
        }
        // The newest calorie baseline and accounting-anchor sample per device seed
        // the next decisions; their counts already live in the summary.
        let pinned = "SELECT MAX(s.event_id) FROM samples s JOIN events e ON e.id=s.event_id
                LEFT JOIN pad_calories c ON c.event_id=s.event_id
                WHERE s.quality<>'out_of_order' AND COALESCE(c.baseline,1)=1 GROUP BY e.device
            UNION SELECT MAX(e.id) FROM anchors a JOIN events e ON e.device=a.device
                AND e.connection_id=a.connection_id AND e.utc=a.utc JOIN samples s ON s.event_id=e.id GROUP BY a.device";
        tx.execute_batch(
            "DROP TABLE IF EXISTS temp.doomed; CREATE TEMP TABLE doomed(id INTEGER PRIMARY KEY);",
        )
        .map_err(sql)?;
        tx.execute(&format!("INSERT INTO temp.doomed SELECT s.event_id FROM samples s JOIN day_summaries d ON d.day=s.local_day WHERE s.event_id NOT IN ({pinned})"), [])
            .map_err(sql)?;
        for id in &loose {
            tx.execute("INSERT OR IGNORE INTO temp.doomed VALUES(?1)", [id])
                .map_err(sql)?;
        }
        tx.execute_batch(&format!(
            "DELETE FROM pad_calories WHERE event_id IN (SELECT id FROM temp.doomed);
            DELETE FROM recovery_credits WHERE event_id IN (SELECT s.event_id FROM samples s JOIN day_summaries d ON d.day=s.local_day);
            DELETE FROM ui_intervals WHERE day IN (SELECT day FROM day_summaries);
            DELETE FROM ui_processed WHERE event_id IN (SELECT id FROM temp.doomed);
            DELETE FROM samples WHERE event_id IN (SELECT id FROM temp.doomed);
            DELETE FROM events WHERE id IN (SELECT id FROM temp.doomed);
            DELETE FROM ui_sessions WHERE id IN (SELECT session FROM session_summaries)
                AND id NOT IN (SELECT session FROM ui_intervals) AND id NOT IN ({live})
                AND id NOT IN (SELECT json_extract(value,'$.ended[0]') FROM ui_anchors
                    WHERE json_extract(value,'$.ended[0]') IS NOT NULL);
            DROP TABLE temp.doomed;
            PRAGMA user_version={COMPACTED_VERSION};"
        ))
        .map_err(sql)?;
        tx.execute(
            "INSERT INTO settings VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![MARKER, format!("v1; detail days {DETAIL_DAYS}")],
        )
        .map_err(sql)?;
        tx.execute("DELETE FROM settings WHERE key='compaction_error'", [])
            .map_err(sql)?;
        tx.commit().map_err(sql)?;
        self.reclaim()?;
        Ok(days.len())
    }
    fn strings(&self, query: &str, first: &str) -> Result<Vec<String>, String> {
        self.conn
            .prepare(query)
            .map_err(sql)?
            .query_map([first], |r| r.get(0))
            .map_err(sql)?
            .collect::<rusqlite::Result<_>>()
            .map_err(sql)
    }
    /// Deleted pages are returned to the filesystem; the first run converts the file.
    fn reclaim(&self) -> Result<(), String> {
        let mode: i64 = self
            .conn
            .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
            .map_err(sql)?;
        if mode == 2 {
            self.conn.execute_batch("PRAGMA incremental_vacuum;")
        } else {
            self.conn
                .execute_batch("PRAGMA auto_vacuum=INCREMENTAL; VACUUM;")
        }
        .map_err(sql)?;
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(sql)
    }
}
