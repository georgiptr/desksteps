//! Runtime recording operations. State and serialization belong to Runtime.
use super::*;
impl Runtime<'_> {
    pub(super) fn check_recording(&mut self) -> Result<(), String> {
        if Instant::now() >= self.next_session_check {
            self.store.expire_sessions(self.utc_now())?;
            self.next_session_check = Instant::now() + Duration::from_secs(60);
        }
        if self
            .telemetry_watch
            .missing(self.link.as_ref().map(|l| l.generation), Instant::now())
        {
            self.sequence += 1;
            self.store.record(
                Event {
                    key: &format!("{}:{}", self.process, self.sequence),
                    device: &self.connection.target.map(address).unwrap_or_default(),
                    connection: &format!("{}:{}", self.process, self.connection.generation),
                    utc: self.utc_now(),
                    monotonic_ns: Some(self.mono_now()),
                    kind: "telemetry_missing",
                    uuid: protocol::TREADMILL,
                    raw: &[],
                    detail: MISSING_TELEMETRY,
                },
                None,
            )?;
            self.status = "Connected: treadmill data missing".into();
            self.view.lock().unwrap().error = Some(MISSING_TELEMETRY.into());
        }
        if self.overflow.swap(false, Ordering::Relaxed) {
            self.connection.invalidate_scan();
            self.sequence += 1;
            self.store.record(
                Event {
                    key: &format!("{}:{}", self.process, self.sequence),
                    device: &self.connection.target.map(address).unwrap_or_default(),
                    connection: &format!("{}:{}", self.process, self.connection.generation),
                    utc: self.utc_now(),
                    monotonic_ns: Some(self.mono_now()),
                    kind: "queue_overflow",
                    uuid: "",
                    raw: &[],
                    detail: "Some callbacks could not be queued; collection coverage is uncertain",
                },
                None,
            )?;
            self.view.lock().unwrap().error =
                Some("A capture queue overflow occurred; check collection gaps".into());
        }
        Ok(())
    }

    pub(super) fn finish(&mut self) -> Result<(), String> {
        if self.options.development {
            self.store
                .set_setting("development_clock", &self.utc_now().to_rfc3339())?;
        }
        if self.discovery.available() {
            self.discovery.stop();
        }
        drop(self.link.take());
        self.sequence += 1;
        self.store.record(
            Event {
                key: &format!("{}:{}", self.process, self.sequence),
                device: &self.connection.target.map(address).unwrap_or_default(),
                connection: &format!("{}:{}", self.process, self.connection.generation),
                utc: self.utc_now(),
                monotonic_ns: Some(self.mono_now()),
                kind: "recorder_exited",
                uuid: "",
                raw: &[],
                detail: "Exit sends no movement command; disconnect does not stop the belt",
            },
            None,
        )?;
        Ok(())
    }
}
