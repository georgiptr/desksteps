//! Persisted session state and confirmed-pause timing.
use super::*;
mod restart;
pub(crate) const PAUSE_SECONDS: i64 = 300;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Anchor {
    pub(crate) session: Option<i64>,
    pub(crate) connection: String,
    pub(crate) utc: String,
    pub(crate) mono: Option<i64>,
    pub(crate) day: String,
    pub(crate) speed: Option<f64>,
    pub(crate) pause: Option<String>,
    pub(crate) gap: bool,
    /// Session ended by a lost link and when, so recovered steps or a prompt
    /// resume can extend it instead of starting another.
    #[serde(default)]
    pub(crate) ended: Option<(i64, String)>,
    /// Counter evidence for distinguishing a paused restart from missing activity.
    #[serde(default)]
    pub(crate) restart: restart::Restart,
}
pub(crate) fn stamp(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|t| t.with_timezone(&Utc))
}

use super::met;

/// Storage applies these projections in the raw event's transaction. The sink
/// returns the real session ID, avoiding predicted IDs or nested transactions.
pub(crate) trait Projection {
    fn finish(&mut self, session: i64, end: &str) -> Result<(), String>;
    fn pause(&mut self, session: i64, end: &str) -> Result<(), String>;
    fn start(&mut self, device: &str, start: &str, end: &str, gap: bool) -> Result<i64, String>;
    fn interval(
        &mut self,
        session: i64,
        day: &str,
        delta: i64,
        active_ms: i64,
        factor: f64,
    ) -> Result<(), String>;
    fn walking(&mut self, session: i64, end: &str, gap: bool) -> Result<(), String>;
    fn mark_gap(&mut self, session: i64) -> Result<(), String>;
}
pub(crate) fn advance(
    a: &mut Anchor,
    event: &crate::event::Event<'_>,
    sample: Option<(&crate::protocol::Sample, i64, &str)>,
    day: &str,
    excluded: bool,
    recovery: super::recovery::Recovery,
    writer: &mut impl Projection,
) -> Result<bool, String> {
    let now = event.utc;
    let old = if a.utc.is_empty() {
        None
    } else {
        Some(stamp(&a.utc).map_err(|e| e.to_string())?)
    };
    if old.is_some_and(|old| now < old) || sample.is_some_and(|(_, _, q)| q == "out_of_order") {
        return Ok(false);
    }
    let changed = !a.connection.is_empty() && a.connection != event.connection;
    if changed
        || matches!(
            event.kind,
            "disconnected"
                | "manual_disconnect"
                | "recorder_started"
                | "recorder_exited"
                | "queue_overflow"
                | "notification_error"
                | "telemetry_missing"
        )
    {
        a.restart.interrupt();
        // A lost link ends the session at its last evidence; walking that was
        // still in progress keeps a gap label because the end is unconfirmed.
        if let Some(session) = a.session.take() {
            a.restart.end(a.pause.is_some());
            let end = a.pause.clone().unwrap_or_else(|| a.utc.clone());
            if a.pause.is_none() {
                writer.mark_gap(session)?;
            }
            writer.finish(session, &end)?;
            a.ended = Some((session, end));
        }
        a.gap = false;
        a.pause = None;
        a.speed = None;
    }
    if let Some(pause) = &a.pause
        && (now - stamp(pause).map_err(|e| e.to_string())?).num_seconds() >= PAUSE_SECONDS
        && let Some(session) = a.session.take()
    {
        writer.finish(session, pause)?;
        a.pause = None;
    }
    if excluded {
        a.restart = Default::default();
        // A diagnostic launch cannot contribute personal exercise. Keep any
        // prior personal session with a gap, without turning the trial into rest.
        a.gap = a.session.is_some();
        a.pause = None;
        a.speed = None;
    } else if sample.is_none()
        && !changed
        && !a.gap
        && event.connection == a.connection
        && event.kind == "notification"
        && event.uuid == "00002ada-0000-1000-8000-00805f9b34fb"
    {
        // The verified pad emits 02 02 for Pause, 02 01 for Stop and 04 for
        // Resume. Record these as status evidence, never as steps or motion.
        if matches!(event.raw, [2, 1] | [2, 2])
            && a.pause.is_none()
            && let Some(session) = a.session
        {
            a.pause = Some(now.to_rfc3339());
            a.speed = None;
            writer.pause(session, &now.to_rfc3339())?;
        } else if event.raw == [4] {
            a.pause = None;
            a.speed = None;
        }
    } else if let Some((s, delta, quality)) = sample {
        a.restart.observe(s, quality);
        let recovered = recovery.any();
        let credited = quality == "observed" && delta > 0 || recovered;
        if credited {
            // Recovered steps prove walking continued through the lost link, and a
            // resume within the pause window is the same session as after a pause.
            if a.session.is_none()
                && let Some((session, end)) = a.ended.take()
            {
                let clean_pause = a.restart.resume() && !recovered;
                if recovered
                    || (now - stamp(&end).map_err(|e| e.to_string())?).num_seconds() < PAUSE_SECONDS
                {
                    a.session = Some(session);
                    if !clean_pause {
                        writer.mark_gap(session)?;
                    }
                }
            }
            if a.session.is_none() {
                let start = old
                    .filter(|t| {
                        !changed
                            && now.signed_duration_since(*t).num_milliseconds() <= 5000
                            && a.pause.is_none()
                    })
                    .unwrap_or(now);
                a.session = Some(writer.start(
                    event.device,
                    &start.to_rfc3339(),
                    &now.to_rfc3339(),
                    a.gap || recovered,
                )?);
            }
            let wall = old.map(|t| (now - t).num_milliseconds()).unwrap_or(0);
            let elapsed = event
                .monotonic_ns
                .zip(a.mono)
                .map(|(n, o)| (n - o) / 1_000_000)
                .unwrap_or(wall);
            let close = !changed
                && wall > 0
                && wall <= 5000
                && elapsed > 0
                && elapsed <= 5000
                && a.day == day;
            let speed = a.speed.filter(|s| *s > 0.0).and_then(met);
            let active_ms = if recovered {
                recovery.active_ms
            } else if close
                && a.pause.is_none()
                && speed.is_some()
                && s.speed_kmh.is_some_and(|v| v >= 0.0)
            {
                elapsed
            } else {
                0
            };
            let factor = if recovered {
                recovery.factor.unwrap_or(0.0)
            } else {
                (speed.unwrap_or(1.0) - 1.0) * 3.5 / 200.0 * active_ms as f64 / 60_000.0
            };
            let session = a.session.unwrap();
            a.gap = recovered || !close && old.is_some() || active_ms == 0;
            writer.interval(
                session,
                day,
                if recovered { recovery.steps } else { delta },
                active_ms,
                factor,
            )?;
            writer.walking(session, &now.to_rfc3339(), a.gap)?;
            a.pause = None;
        }
        if s.speed_kmh == Some(0.0)
            && a.pause.is_none()
            && let Some(session) = a.session
        {
            a.pause = Some(now.to_rfc3339());
            writer.pause(session, &now.to_rfc3339())?;
        } else if s.speed_kmh.is_some_and(|v| v > 0.0) && a.pause.is_some() {
            a.pause = None;
        }
        a.connection = event.connection.into();
        a.utc = now.to_rfc3339();
        a.mono = event.monotonic_ns;
        a.day = day.into();
        a.speed = if excluded { None } else { s.speed_kmh };
    }
    if a.gap
        && let Some(session) = a.session
    {
        writer.mark_gap(session)?;
    }
    Ok(true)
}
