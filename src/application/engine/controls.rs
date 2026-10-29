//! Runtime controls operations. State and serialization belong to Runtime.
use super::*;
impl Runtime<'_> {
    pub(super) fn begin_control(
        &mut self,
        transaction: control::Transaction,
    ) -> Result<(), String> {
        self.sequence += 1;
        self.store.record(
            Event {
                key: &format!("{}:{}", self.process, self.sequence),
                device: &self.connection.target.map(address).unwrap_or_default(),
                connection: &format!("{}:{}", self.process, self.connection.generation),
                utc: self.utc_now(),
                monotonic_ns: Some(self.mono_now()),
                kind: "control_write",
                uuid: control::UUID,
                raw: transaction.bytes(),
                detail: "Explicit user command; no automatic retry",
            },
            None,
        )?;
        let write = match self
            .link
            .as_ref()
            .unwrap()
            .control_write(transaction.bytes())
        {
            Ok(write) => write,
            Err(error) => {
                self.link.as_mut().unwrap().control_uncertain = true;
                self.controls.discard_speeds();
                return Err(error);
            }
        };
        self.controls.exchange = Some(Exchange {
            transaction,
            write: Some(write),
            written: false,
        });
        Ok(())
    }
    pub(super) fn control_failed(&mut self, error: String) -> Result<(), String> {
        self.controls.discard_speeds();
        if let Some(current) = self.link.as_mut() {
            current.control_uncertain = true;
        }
        self.sequence += 1;
        self.store.record(
            Event {
                key: &format!("{}:{}", self.process, self.sequence),
                device: &self.connection.target.map(address).unwrap_or_default(),
                connection: &format!("{}:{}", self.process, self.connection.generation),
                utc: self.utc_now(),
                monotonic_ns: Some(self.mono_now()),
                kind: "control_error",
                uuid: control::UUID,
                raw: &[],
                detail: &error,
            },
            None,
        )?;
        Ok(())
    }
    pub(super) fn advance_controls(&mut self) -> Result<(), String> {
        let now_ns = self.mono_now();
        let utc = self.utc_now();
        if let Some(pending) = self.controls.exchange.as_mut() {
            let progress = self
                .link
                .as_ref()
                .ok_or_else(|| "Connection lost; control outcome unknown".to_string())
                .and_then(|current| pending.advance(current, now_ns));
            match progress {
                Err(error) => {
                    self.controls.exchange = None;
                    self.control_failed(error)?;
                }
                Ok(next) => {
                    if pending.transaction.stage() != control::Stage::Access {
                        self.controls.access_granted = true;
                    }
                    let at_boundary =
                        next.is_some() || pending.transaction.stage() == control::Stage::Settling;
                    if self.controls.pause_requested && at_boundary {
                        self.controls.exchange = None;
                    } else if pending.transaction.command().is_speed()
                        && pending.transaction.stage() == control::Stage::Settling
                    {
                        if self.controls.selected_speed.is_some() {
                            self.controls.speed_confirmation = Some(pending.transaction.clone());
                        }
                        self.controls.exchange = None;
                    } else if let Some(frame) = next {
                        if pending.transaction.command().is_speed()
                            && (self.controls.selected_speed.is_none()
                                || control::current_speed(
                                    self.controls.fresh_speed,
                                    pending.transaction.generation(),
                                    now_ns,
                                )
                                .is_none_or(|s| !(1.0..=6.0).contains(&s)))
                        {
                            self.controls.exchange = None;
                            self.control_failed(
                                "Speed telemetry changed before sending; discarded speed click"
                                    .into(),
                            )?;
                            return Ok(());
                        }
                        // Persist intent before the asynchronous action write.
                        self.sequence += 1;
                        self.store.record(
                            Event {
                                key: &format!("{}:{}", self.process, self.sequence),
                                device: &self.connection.target.map(address).unwrap_or_default(),
                                connection: &format!(
                                    "{}:{}",
                                    self.process, self.connection.generation
                                ),
                                utc,
                                monotonic_ns: Some(now_ns),
                                kind: "control_write",
                                uuid: control::UUID,
                                raw: &frame,
                                detail: "Explicit user command; no automatic retry",
                            },
                            None,
                        )?;
                        match self.link.as_ref().unwrap().control_write(&frame) {
                            Ok(write) => pending.write = Some(write),
                            Err(error) => {
                                self.controls.exchange = None;
                                self.control_failed(error)?;
                            }
                        }
                    }
                }
            }
        }
        // Drain late speed indications even when no exchange is waiting. A
        // success is not motion confirmation; an explicit rejection is an error.
        if self.controls.exchange.is_none() {
            let result = (|| -> Result<(), String> {
                if let Some(link) = &self.link {
                    while let Some(bytes) = ports::ControlChannel::next_reply(link)? {
                        control::response(&bytes, 2)?;
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                self.control_failed(error)?;
            }
            if self
                .controls
                .speed_confirmation
                .as_ref()
                .is_some_and(|t| t.expired(now_ns))
            {
                self.control_failed("Selected speed was not observed before timeout; outcome unknown; no automatic retry".into())?;
            }
        }
        if let Some((command, generation)) = self
            .controls
            .take_pause(self.link.as_ref().map(|l| l.generation))
        {
            self.user_command(UserCommand::Control {
                command,
                generation,
            })?;
        }
        match self
            .controls
            .next_speed(self.link.as_ref().map(|l| l.generation), now_ns)
        {
            Ok(Some(transaction)) => {
                if let Err(error) = self.begin_control(transaction) {
                    self.control_failed(error)?;
                }
            }
            Err(error) => self.control_failed(error)?,
            Ok(None) => {}
        }
        Ok(())
    }
}
