//! Runtime commands operations. State and serialization belong to Runtime.
use super::*;
impl Runtime<'_> {
    pub(super) fn user_command(&mut self, input: UserCommand) -> Result<bool, String> {
        match input {
            UserCommand::WindowsStartup(enabled) => {
                // Preview, smoke and simulator runs cannot register any executable.
                if !self.options.no_bluetooth && !self.options.development {
                    let result = crate::windows_startup::configure(enabled);
                    let mut view = self.view.lock().unwrap();
                    match result {
                        Ok(actual) => {
                            view.windows_startup = actual;
                            view.startup_error = None;
                        }
                        Err(error) => view.startup_error = Some(error),
                    }
                }
            }
            UserCommand::WindowPreference(preference) => {
                let (key, enabled) = preference.setting();
                match self
                    .store
                    .set_setting(key, if enabled { "true" } else { "false" })
                {
                    Ok(()) => {
                        let mut view = self.view.lock().unwrap();
                        preference.apply(&mut view.window_behavior);
                        view.startup_error = None;
                    }
                    Err(error) => self.view.lock().unwrap().startup_error = Some(error),
                }
            }
            UserCommand::Theme(preference) => {
                self.store.set_setting("theme", preference.as_str())?;
                self.view.lock().unwrap().theme_preference = preference;
            }
            UserCommand::Simulator(action) => {
                self.simulator_action(action)?;
            }
            UserCommand::Dashboard(request) => {
                self.dashboard_request = request;
                self.next_dashboard = Instant::now();
                if self.dashboard_request.is_none() {
                    self.view.lock().unwrap().dashboard = None;
                }
            }
            UserCommand::SavePreferences(preferences) => {
                let result = self.store.save_preferences(&preferences);
                self.view.lock().unwrap().ui_message = Some(match result {
                    Ok(()) => format!(
                        "Saved from {}. Earlier dates keep their settings.",
                        preferences.effective
                    ),
                    Err(error) => error,
                });
                self.view.lock().unwrap().dashboard = None;
                self.next_dashboard = Instant::now();
            }
            UserCommand::Increment(value) => {
                if (1..=10).contains(&value) {
                    self.store
                        .set_setting("speed_increment", &value.to_string())?;
                    self.controls.increment = value;
                }
            }
            UserCommand::Shortcuts(value) => match value.validate() {
                Ok(()) => {
                    self.store.set_setting(
                        "shortcuts",
                        &serde_json::to_string(&value).map_err(|e| e.to_string())?,
                    )?;
                    self.view.lock().unwrap().shortcuts = value;
                }
                Err(error) => self.view.lock().unwrap().shortcut_error = Some(error),
            },
            UserCommand::Control {
                command,
                generation: requested_generation,
            } => {
                if self.controls.defer_pause(
                    command,
                    requested_generation,
                    self.link.as_ref().map(|l| l.generation),
                ) {
                    return Ok(true);
                }
                let result = (|| -> Result<(), String> {
                    let transaction = self.controls.prepare(
                        command,
                        requested_generation,
                        self.link.as_ref().map(|l| l.generation),
                        self.options.controls_enabled
                            && (!self.options.no_bluetooth || self.options.development),
                        self.link.as_ref().is_some_and(|l| l.control_uncertain),
                        self.mono_now(),
                    )?;
                    if command.is_speed() {
                        self.controls.queue_speed(transaction, self.mono_now())?;
                    } else {
                        self.controls.discard_speeds();
                        self.begin_control(transaction)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
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
                }
            }
            UserCommand::Exit => return Ok(false),
            UserCommand::Select(selected) => {
                self.controls.reset();
                self.cancel.reset();
                if self.link.is_some()
                    && let Some(previous) = self.connection.target
                {
                    self.sequence += 1;
                    self.store.record(Event {key:&format!("{}:{}",self.process,self.sequence),device:&address(previous),connection:&format!("{}:{}",self.process,self.connection.generation),utc:self.utc_now(),monotonic_ns:Some(self.mono_now()),kind:"manual_disconnect",uuid:"",raw:&[],detail:"Device selection changed; continuity is unknown; no movement command"},None)?;
                }
                drop(self.link.take());
                self.save_target(selected)?;
                self.connection.enabled =
                    self.connection.automatic_connection && !self.options.no_bluetooth;
                if self.connection.enabled {
                    self.connection.restart(Instant::now());
                    self.status = "Searching for selected WalkingPad".into();
                } else {
                    self.status = "WalkingPad selected: automatic connection disabled".into();
                }
            }
            UserCommand::AutomaticConnection(enabled) => {
                self.set_automatic_connection(enabled)?;
            }
            UserCommand::RecoverGaps(enabled) => {
                self.store
                    .set_setting("recover_gaps", if enabled { "true" } else { "false" })?;
                self.view.lock().unwrap().recover_gaps = enabled;
            }
            UserCommand::Disconnect if self.options.development => {
                self.simulator_action(SimAction::Disconnect)?;
            }
            UserCommand::Disconnect => {
                self.set_automatic_connection(false)?;
                self.controls.reset();
                drop(self.link.take());
                self.status = "Disconnected: automatic connection disabled".into();
                self.sequence += 1;
                self.store.record(
                    Event {
                        key: &format!("{}:{}", self.process, self.sequence),
                        device: &self.connection.target.map(address).unwrap_or_default(),
                        connection: &format!("{}:{}", self.process, self.connection.generation),
                        utc: self.utc_now(),
                        monotonic_ns: Some(self.mono_now()),
                        kind: "manual_disconnect",
                        uuid: "",
                        raw: &[],
                        detail: "No movement command sent",
                    },
                    None,
                )?;
            }
            UserCommand::ActiveScan if self.options.development => {
                self.simulator_action(SimAction::Scan)?;
            }
            UserCommand::ActiveScan => {
                self.cancel.reset();
                if self.link.is_none() && !self.options.no_bluetooth && self.discovery.available() {
                    self.discovery.stop();
                    self.connection.scan_running = false;
                    self.connection.request_scan();
                    self.status = "Finding WalkingPad (10-second scan)".into();
                }
            }
            UserCommand::DashboardVisible(value) => {
                self.view.lock().unwrap().dashboard_visible = value;
            }
            UserCommand::ShortcutStatus(value) => {
                self.view.lock().unwrap().shortcut_error = value;
            }
            UserCommand::ClearUiMessage => {
                self.view.lock().unwrap().ui_message = None;
            }
        }
        Ok(true)
    }
    pub(super) fn set_automatic_connection(&mut self, enabled: bool) -> Result<(), String> {
        // Save before changing runtime behavior so a failed write cannot leave
        // the checkbox and next launch with different preferences.
        self.store.set_setting(
            "automatic_connection",
            if enabled { "true" } else { "false" },
        )?;
        self.connection.automatic_connection = enabled;
        self.connection.enabled =
            enabled && (!self.options.no_bluetooth || self.options.development);
        if self.connection.enabled {
            self.cancel.reset();
            self.connection.restart(Instant::now());
            if self.link.is_none() {
                self.status = "Searching for WalkingPad".into();
            }
        } else {
            self.connection.cancel_scan();
            if self.discovery.available() {
                self.discovery.stop();
                self.discovery.active(false)?;
            }
            self.connection.scan_running = false;
            if self.link.is_none() && !self.options.no_bluetooth {
                self.status = "Automatic connection disabled".into();
            }
        }
        Ok(())
    }
}
