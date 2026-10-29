//! Runtime events operations. State and serialization belong to Runtime.
use super::*;
impl Runtime<'_> {
    pub(super) fn device_event(&mut self, input: DeviceEvent) -> Result<bool, String> {
        match input {
            DeviceEvent::ScanStopped { error, interrupted } => {
                // Radio-off aborts discovery. Keep the worker alive and retry
                // after availability returns; do not leave a stale running flag.
                // Successful stops are already handled by their caller. A late
                // callback from a mode change must not interrupt the new scan.
                if interrupted {
                    self.connection.interrupt_scan();
                    self.connection.next_scan_try = Instant::now() + Duration::from_secs(2);
                }
                self.sequence += 1;
                self.store.record(
                    Event {
                        key: &format!("{}:{}", self.process, self.sequence),
                        device: &self.connection.target.map(address).unwrap_or_default(),
                        connection: &format!("{}:{}", self.process, self.connection.generation),
                        utc: self.utc_now(),
                        monotonic_ns: Some(self.mono_now()),
                        kind: "scan_stopped",
                        uuid: "",
                        raw: &[],
                        detail: &error,
                    },
                    None,
                )?;
            }
            DeviceEvent::Found(candidate, received) => {
                if self.connection.target == Some(candidate.address)
                    && !candidate.name.trim().is_empty()
                    && self.connection.target_name.as_deref() != Some(candidate.name.as_str())
                {
                    self.store
                        .save_device(candidate.address, Some(&candidate.name))?;
                    self.connection.target_name = Some(candidate.name.clone());
                }
                self.connection.seen(candidate, received);
            }
            DeviceEvent::Lost(lost) => {
                if self.link.as_ref().is_some_and(|l| l.generation == lost) {
                    self.controls.reset();
                    drop(self.link.take());
                    self.connection.name_scan_used = false;
                    self.connection.next_try = Instant::now() + self.connection.retry;
                    self.status = if self.connection.enabled {
                        "Bluetooth disconnected: searching again"
                    } else {
                        "Bluetooth disconnected: automatic connection disabled"
                    }
                    .into();
                    self.sequence += 1;
                    self.store.record(
                        Event {
                            key: &format!("{}:{}", self.process, self.sequence),
                            device: &self.connection.target.map(address).unwrap_or_default(),
                            connection: &format!("{}:{lost}", self.process),
                            utc: self.utc_now(),
                            monotonic_ns: Some(self.mono_now()),
                            kind: "disconnected",
                            uuid: "",
                            raw: &[],
                            detail: "",
                        },
                        None,
                    )?;
                }
            }
            DeviceEvent::Raw {
                generation: raw_gen,
                device,
                utc,
                mono,
                kind,
                uuid,
                bytes,
                detail,
            } => {
                let decoded = if uuid == protocol::TREADMILL && kind == "notification" {
                    Some(protocol::decode(&bytes))
                } else {
                    None
                };
                let eligible = self.link.as_ref().is_some_and(|l| l.generation == raw_gen);
                if eligible && kind == "notification" && uuid == MACHINE_STATUS && bytes == [0x04] {
                    self.telemetry_watch.started(raw_gen, Instant::now());
                }
                let sample = if eligible {
                    decoded.as_ref().and_then(|r| r.as_ref().ok())
                } else {
                    None
                };
                let detail = decoded
                    .as_ref()
                    .and_then(|r| r.as_ref().err())
                    .cloned()
                    .unwrap_or(detail);
                self.sequence += 1;
                self.store.record(
                    Event {
                        key: &format!("{}:{}", self.process, self.sequence),
                        device: &address(device),
                        connection: &format!("{}:{raw_gen}", self.process),
                        utc,
                        monotonic_ns: Some(mono),
                        kind,
                        uuid: &uuid,
                        raw: &bytes,
                        detail: &detail,
                    },
                    sample,
                )?;
                if eligible
                    && kind == "notification"
                    && uuid == control::UUID
                    && let Err(error) = control::response(&bytes, 2)
                {
                    self.control_failed(error)?;
                }
                if let Some(sample) = sample {
                    if let Some(speed) = sample.speed_kmh {
                        let _ = self.controls.observe(raw_gen, speed, mono);
                    }
                    self.telemetry_watch.received(raw_gen);
                    let mut v = self.view.lock().unwrap();
                    if v.error.as_deref() == Some(MISSING_TELEMETRY) {
                        v.error = None;
                    }
                    v.last_sample = Some(
                        utc.with_timezone(&self.options.timezone)
                            .format("%H:%M:%S")
                            .to_string(),
                    );
                    v.speed = sample.speed_kmh;
                    v.device_steps = sample.steps;
                    self.status = if sample.speed_kmh.is_some_and(|s| s > 0.0) {
                        "Walking: recording".into()
                    } else {
                        "Connected: belt stopped in last sample".into()
                    };
                } else if decoded.as_ref().is_some_and(|r| r.is_err()) {
                    self.view.lock().unwrap().error =
                        Some(format!("Packet decode error: {detail}"));
                } else if eligible && kind == "notification_error" {
                    self.view.lock().unwrap().error =
                        Some(format!("Could not receive notification: {detail}"));
                }
            }
        }
        Ok(true)
    }
}
