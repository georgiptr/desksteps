//! Runtime connection operations. State and serialization belong to Runtime.
use super::*;
impl Runtime<'_> {
    pub(super) fn maintain_connection(&mut self) -> Result<(), String> {
        if self.options.development {
            return Ok(());
        }
        if self
            .connection
            .active_until
            .is_some_and(|until| Instant::now() >= until)
        {
            if self.discovery.available() {
                self.discovery.stop();
                self.discovery.active(false)?;
                self.connection.scan_running = false;
            }
            self.complete_scan(Instant::now())?;
            if !self.connection.enabled {
                self.status = "Scan complete: enable Connect Automatically to connect".into();
            }
        }
        if (self.connection.enabled
            || self.connection.scan_requested
            || self.connection.active_until.is_some())
            && self.link.is_none()
        {
            if self.discovery.available()
                && !self.connection.scan_running
                && Instant::now() >= self.connection.next_scan_try
            {
                if self.connection.scan_requested {
                    self.discovery.active(true)?;
                }
                match self.discovery.start() {
                    Ok(()) => self.connection.scan_started(Instant::now()),
                    Err(error) => {
                        self.connection.next_scan_try = Instant::now() + Duration::from_secs(2);
                        self.status = "Bluetooth unavailable: retrying discovery".into();
                        self.sequence += 1;
                        self.store.record(
                            Event {
                                key: &format!("{}:{}", self.process, self.sequence),
                                device: &self.connection.target.map(address).unwrap_or_default(),
                                connection: &format!(
                                    "{}:{}",
                                    self.process, self.connection.generation
                                ),
                                utc: self.utc_now(),
                                monotonic_ns: Some(self.mono_now()),
                                kind: "scan_error",
                                uuid: "",
                                raw: &[],
                                detail: &error.to_string(),
                            },
                            None,
                        )?;
                    }
                }
            }
            if let Some(target_address) = self.connection.target.filter(|_| self.connection.enabled)
            {
                if self.connection.can_attempt(target_address, Instant::now()) {
                    self.status = "Connecting to WalkingPad".into();
                    {
                        self.view.lock().unwrap().status = self.status.clone();
                    }
                    (self.notify)();
                    self.connection.generation += 1;
                    match Link::open(
                        target_address,
                        self.connection.generation,
                        self.tx.clone(),
                        self.overflow.clone(),
                        self.origin,
                        self.options.controls_enabled,
                        &self.cancel,
                    ) {
                        Ok(connected) => {
                            self.link = Some(connected);
                            self.connection.retry = Duration::from_secs(2);
                            self.status = "Connected: waiting for telemetry".into();
                            self.view.lock().unwrap().error = None;
                            if self.discovery.available() {
                                self.discovery.stop();
                                self.connection.scan_running = false;
                            }
                        }
                        Err(error) => {
                            let refresh_name = matches!(error, ConnectError::NameUnavailable(_));
                            let error = error.to_string();
                            self.status = "Connection failed: retrying".into();
                            self.view.lock().unwrap().error = Some(error.clone());
                            self.sequence += 1;
                            self.store.record(
                                Event {
                                    key: &format!("{}:{}", self.process, self.sequence),
                                    device: &address(target_address),
                                    connection: &format!(
                                        "{}:{}",
                                        self.process, self.connection.generation
                                    ),
                                    utc: self.utc_now(),
                                    monotonic_ns: Some(self.mono_now()),
                                    kind: "connect_error",
                                    uuid: "",
                                    raw: &[],
                                    detail: &error,
                                },
                                None,
                            )?;
                            // An address-only passive advertisement can arrive
                            // before Windows learns the scan-response name.
                            // Make one bounded attempt per connection episode,
                            // keeping exact name verification before any write.
                            if refresh_name
                                && !self.connection.name_scan_used
                                && self.connection.active_until.is_none()
                                && self.discovery.available()
                            {
                                self.discovery.stop();
                                self.connection.scan_running = false;
                                self.connection.request_scan();
                                self.connection.name_scan_used = true;
                                self.status = "Resolving WalkingPad name (10-second scan)".into();
                                self.sequence += 1;
                                self.store.record(Event {
                                    key: &format!("{}:{}",self.process,self.sequence),
                                    device: &address(target_address),
                                    connection: &format!("{}:{}",self.process,self.connection.generation),
                                    utc: self.utc_now(),
                                    monotonic_ns: Some(self.mono_now()),
                                    kind: "name_refresh_scan",
                                    uuid: "",
                                    raw: &[],
                                    detail: "One bounded 10-second active scan; unresolved names authorize no handshake",
                                }, None)?;
                            }
                            self.connection.failed(Instant::now());
                        }
                    }
                    self.sequence += 1;
                    self.store.record(
                        Event {
                            key: &format!("{}:{}", self.process, self.sequence),
                            device: &address(target_address),
                            connection: &format!("{}:{}", self.process, self.connection.generation),
                            utc: self.utc_now(),
                            monotonic_ns: Some(self.mono_now()),
                            kind: if self.link.is_some() {
                                "connected"
                            } else {
                                "connection_attempt_finished"
                            },
                            uuid: "",
                            raw: &[],
                            detail: "",
                        },
                        None,
                    )?;
                }
            } else {
                self.status =
                    if self.connection.active_until.is_some() || self.connection.scan_requested {
                        "Finding WalkingPad (10-second scan)".into()
                    } else {
                        "Choose device from the tray menu; use Scan to scan again".into()
                    };
            }
        }
        Ok(())
    }
    pub(super) fn complete_scan(&mut self, now: Instant) -> Result<(), String> {
        if let Some(selected) = self.connection.finish_scan(now) {
            // Persist first: a failed settings write must not silently select a
            // device for this run without saving the same default for next time.
            self.save_target(selected)?;
            self.connection.restart(now);
            self.sequence += 1;
            self.store.record(
                Event {
                    key: &format!("{}:{}", self.process, self.sequence),
                    device: &address(selected),
                    connection: &format!("{}:{}", self.process, self.connection.generation),
                    utc: self.utc_now(),
                    monotonic_ns: Some(self.mono_now()),
                    kind: "device_auto_selected",
                    uuid: "",
                    raw: &[],
                    detail: "Only matching pad in completed 10-second scan; saved as default; no movement command",
                },
                None,
            )?;
        }
        Ok(())
    }
    pub(super) fn save_target(&mut self, selected: u64) -> Result<(), String> {
        let name = self
            .connection
            .candidates
            .get(&selected)
            .map(|(c, _)| c.name.as_str());
        self.store.save_device(selected, name)?;
        self.connection.target_name = self.store.device_name(selected)?;
        self.connection.target = Some(selected);
        *self.shared_target.lock().unwrap() = Some(selected);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_runtime(target: Option<u64>, test: impl FnOnce(&mut Runtime<'_>)) {
        let now = Instant::now();
        let (tx, rx) = mpsc::sync_channel(512);
        let tx = MessageSender(tx);
        let overflow = Arc::new(AtomicBool::new(false));
        let shared_target = Arc::new(Mutex::new(target));
        let store = Store::initialize(
            rusqlite::Connection::open_in_memory().unwrap(),
            chrono_tz::UTC,
        )
        .unwrap();
        let mut runtime = Runtime {
            simulator: None,
            connection: crate::application::connection::ConnectionPolicy::new(target, true, now),
            options: Options {
                database_ownership: None,
                database: "unused-synthetic.sqlite".into(),
                timezone: chrono_tz::UTC,
                device: target,
                no_bluetooth: true,
                development: false,
                smoke_seconds: None,
                controls_enabled: false,
            },
            notify: &|| {},
            view: Arc::new(Mutex::new(View::default())),
            discovery: Discovery::new(false, tx.clone(), overflow.clone(), shared_target.clone())
                .unwrap(),
            tx,
            rx,
            overflow,
            shared_target,
            store,
            cancel: Cancellation::default(),
            origin: now,
            process: "synthetic-scan".into(),
            link: None,
            sequence: 0,
            telemetry_watch: TelemetryWatch::default(),
            controls: crate::application::controls::Controls::<WindowsWrite>::new(1),
            dashboard_request: None,
            next_dashboard: now,
            next_session_check: now,
            status: String::new(),
        };
        test(&mut runtime);
        assert!(!runtime.discovery.available());
        assert!(runtime.link.is_none());
        assert_eq!(
            runtime
                .store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE kind='control_write'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    fn found(address: u64, received: Instant) -> DeviceEvent {
        DeviceEvent::Found(
            Candidate {
                address,
                name: protocol::MODEL_NAME.into(),
            },
            received,
        )
    }
    #[test]
    fn selected_device_learns_refreshes_and_retains_its_name() {
        with_runtime(Some(1), |r| {
            let now = Instant::now();
            r.device_event(found(1, now)).unwrap();
            assert_eq!(
                r.store.device_name(1).unwrap().as_deref(),
                Some(protocol::MODEL_NAME)
            );
            for name in ["Renamed synthetic pad", "", "  "] {
                r.device_event(DeviceEvent::Found(
                    Candidate {
                        address: 1,
                        name: name.into(),
                    },
                    now,
                ))
                .unwrap();
            }
            r.connection.candidates.clear();
            r.publish().unwrap();
            assert_eq!(
                r.view.lock().unwrap().selected_name.as_deref(),
                Some("Renamed synthetic pad")
            );
            r.save_target(2).unwrap();
            assert_eq!(r.connection.target_name, None);
            r.save_target(1).unwrap();
            assert_eq!(
                r.connection.target_name.as_deref(),
                Some("Renamed synthetic pad")
            );
        });
    }
    #[test]
    fn automatic_connection_setting_is_saved_and_published_without_enabling_preview_bluetooth() {
        with_runtime(None, |r| {
            for enabled in [false, true, false] {
                r.user_command(UserCommand::AutomaticConnection(enabled))
                    .unwrap();
                assert_eq!(r.connection.automatic_connection, enabled);
                assert!(!r.connection.enabled);
                assert!(!r.connection.scan_requested);
                assert_eq!(
                    r.store.setting("automatic_connection").unwrap().as_deref(),
                    Some(if enabled { "true" } else { "false" })
                );
                r.publish().unwrap();
                assert_eq!(r.view.lock().unwrap().automatic_connection, enabled);
            }
            r.user_command(UserCommand::AutomaticConnection(true))
                .unwrap();
            r.user_command(UserCommand::Disconnect).unwrap();
            assert!(!r.connection.automatic_connection);
            assert_eq!(
                r.store.setting("automatic_connection").unwrap().as_deref(),
                Some("false")
            );
        });
    }
    #[test]
    fn disabling_automatic_connection_cancels_scan_and_prevents_default_selection() {
        with_runtime(None, |r| {
            let start = Instant::now();
            r.connection.scan_started(start);
            r.device_event(found(1, start)).unwrap();
            r.user_command(UserCommand::AutomaticConnection(false))
                .unwrap();
            assert!(!r.connection.scan_running);
            assert!(r.connection.active_until.is_none());
            r.complete_scan(start + Duration::from_secs(10)).unwrap();
            assert_eq!(r.store.setting("device").unwrap(), None);
            // A subsequent explicit scan discovers candidates but cannot
            // silently re-enable automatic selection or a connection.
            r.connection.request_scan();
            r.connection.scan_started(start);
            r.device_event(found(2, start)).unwrap();
            r.complete_scan(start + Duration::from_secs(10)).unwrap();
            assert_eq!(r.connection.target, None);
            assert_eq!(r.connection.candidates.len(), 1);
            assert!(!r.connection.enabled);
            r.maintain_connection().unwrap();
            r.user_command(UserCommand::Select(2)).unwrap();
            assert!(!r.connection.enabled);
            assert!(!r.connection.automatic_connection);
        });
    }
    #[test]
    fn failed_automatic_connection_write_preserves_runtime_preference_and_scan() {
        with_runtime(None, |r| {
            let start = Instant::now();
            r.connection.scan_started(start);
            r.store.conn.execute_batch("CREATE TRIGGER fail_automatic BEFORE INSERT ON settings WHEN NEW.key='automatic_connection' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
            assert!(
                r.user_command(UserCommand::AutomaticConnection(false))
                    .unwrap_err()
                    .contains("synthetic failure")
            );
            assert!(r.connection.automatic_connection);
            assert!(r.connection.enabled);
            assert!(r.connection.scan_running);
            assert_eq!(
                r.connection.active_until,
                Some(start + Duration::from_secs(10))
            );
        });
    }
    #[test]
    fn completed_unique_scan_persists_and_publishes_default_without_movement() {
        with_runtime(None, |r| {
            let start = Instant::now();
            r.connection.scan_started(start);
            r.device_event(found(0x112233445566, start)).unwrap();
            r.complete_scan(start + Duration::from_secs(9)).unwrap();
            assert_eq!(r.store.setting("device").unwrap(), None);
            r.complete_scan(start + Duration::from_secs(10)).unwrap();
            assert_eq!(
                r.store.setting("device").unwrap().as_deref(),
                Some("11:22:33:44:55:66")
            );
            assert_eq!(r.connection.target, Some(0x112233445566));
            assert_eq!(
                r.store.device_name(0x112233445566).unwrap().as_deref(),
                Some(protocol::MODEL_NAME)
            );
            assert_eq!(*r.shared_target.lock().unwrap(), r.connection.target);
            r.publish().unwrap();
            assert_eq!(r.view.lock().unwrap().selected, r.connection.target);
            assert!(
                r.connection
                    .can_attempt(0x112233445566, start + Duration::from_secs(10))
            );
            assert_eq!(
                r.store
                    .conn
                    .query_row(
                        "SELECT COUNT(*) FROM events WHERE kind='device_auto_selected'",
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                1
            );
        });
    }
    #[test]
    fn queued_second_pad_prevents_automatic_choice_at_scan_boundary() {
        with_runtime(None, |r| {
            let start = Instant::now() - Duration::from_secs(11);
            r.connection.scan_started(start);
            r.device_event(found(1, start)).unwrap();
            r.tx.try_send(found(2, start + Duration::from_secs(9)))
                .unwrap();
            assert!(r.drain_scan_events().unwrap());
            r.complete_scan(Instant::now()).unwrap();
            assert_eq!(r.connection.target, None);
            assert_eq!(r.store.setting("device").unwrap(), None);
            r.user_command(UserCommand::Select(2)).unwrap();
            assert_eq!(
                r.store.setting("device").unwrap().as_deref(),
                Some("00:00:00:00:00:02")
            );
        });
    }
    #[test]
    fn aborted_scan_restarts_but_late_successful_stop_does_not_reset_new_scan() {
        with_runtime(None, |r| {
            let start = Instant::now();
            r.connection.scan_started(start);
            r.device_event(found(1, start)).unwrap();
            r.device_event(DeviceEvent::ScanStopped {
                error: "radio unavailable".into(),
                interrupted: true,
            })
            .unwrap();
            assert!(r.connection.scan_requested);
            r.connection.scan_started(start + Duration::from_secs(20));
            r.device_event(DeviceEvent::ScanStopped {
                error: "success".into(),
                interrupted: false,
            })
            .unwrap();
            assert!(r.connection.scan_running);
            assert_eq!(
                r.connection.active_until,
                Some(start + Duration::from_secs(30))
            );
            r.complete_scan(start + Duration::from_secs(30)).unwrap();
            assert_eq!(r.store.setting("device").unwrap(), None);
        });
    }
    #[test]
    fn queue_loss_or_disconnect_prevents_saving_single_seen_pad() {
        for disconnect in [false, true] {
            with_runtime(None, |r| {
                let start = Instant::now();
                r.connection.scan_started(start);
                r.device_event(found(1, start)).unwrap();
                if disconnect {
                    r.user_command(UserCommand::Disconnect).unwrap();
                } else {
                    r.overflow.store(true, Ordering::Relaxed);
                    r.check_recording().unwrap();
                }
                r.complete_scan(start + Duration::from_secs(10)).unwrap();
                assert_eq!(r.store.setting("device").unwrap(), None);
            });
        }
    }
    #[test]
    fn failed_default_write_does_not_change_runtime_target() {
        with_runtime(None, |r| {
            let start = Instant::now();
            r.connection.scan_started(start);
            r.device_event(found(1, start)).unwrap();
            r.store.conn.execute_batch("CREATE TRIGGER fail_device BEFORE INSERT ON settings WHEN NEW.key='device' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
            assert!(
                r.complete_scan(start + Duration::from_secs(10))
                    .unwrap_err()
                    .contains("synthetic failure")
            );
            assert_eq!(r.connection.target, None);
            assert_eq!(*r.shared_target.lock().unwrap(), None);
        });
    }
    #[test]
    fn queued_exit_is_honored_before_scan_completion() {
        with_runtime(None, |r| {
            r.tx.try_send(UserCommand::Exit).unwrap();
            assert!(!r.drain_scan_events().unwrap());
            assert_eq!(r.store.setting("device").unwrap(), None);
        });
    }
    #[test]
    fn full_queued_batch_does_not_authorize_automatic_selection() {
        with_runtime(None, |r| {
            let start = Instant::now();
            r.connection.scan_started(start);
            for _ in 0..512 {
                r.tx.try_send(found(1, start)).unwrap();
            }
            assert!(r.drain_scan_events().unwrap());
            r.complete_scan(start + Duration::from_secs(10)).unwrap();
            assert_eq!(r.store.setting("device").unwrap(), None);
        });
    }
    #[test]
    fn saved_default_is_kept_even_when_another_pad_is_the_only_scan_match() {
        with_runtime(Some(2), |r| {
            r.store.set_setting("device", &address(2)).unwrap();
            let start = Instant::now();
            r.connection.request_scan();
            r.connection.scan_started(start);
            r.device_event(found(1, start)).unwrap();
            r.complete_scan(start + Duration::from_secs(10)).unwrap();
            assert_eq!(r.connection.target, Some(2));
            assert_eq!(r.store.setting("device").unwrap(), Some(address(2)));
        });
    }
}
