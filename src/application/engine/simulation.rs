//! Development scenarios run on the worker, through normal event accounting.
use super::*;
impl Runtime<'_> {
    pub(super) fn utc_now(&self) -> DateTime<Utc> {
        Utc::now()
            + self
                .simulator
                .as_ref()
                .map(|s| s.lock().unwrap().offset)
                .unwrap_or_default()
    }
    pub(super) fn mono_now(&self) -> i64 {
        (self.origin.elapsed().as_nanos() as i64).saturating_add(
            self.simulator
                .as_ref()
                .and_then(|s| s.lock().unwrap().offset.num_nanoseconds())
                .unwrap_or(0),
        )
    }
    fn sim_raw(&mut self, bytes: Vec<u8>, uuid: &str) -> Result<(), String> {
        self.device_event(DeviceEvent::Raw {
            generation: self.connection.generation,
            device: simulator::ADDRESS,
            utc: self.utc_now(),
            mono: self.mono_now(),
            kind: "notification",
            uuid: uuid.into(),
            bytes,
            detail: "Synthetic development pad; no physical Bluetooth activity".into(),
        })
        .map(|_| ())
    }
    fn sim_connect(&mut self) -> Result<(), String> {
        let Some(pad) = self.simulator.clone() else {
            return Ok(());
        };
        self.controls.reset();
        self.connection.generation += 1;
        self.link = Some(Link::synthetic(pad.clone(), self.connection.generation));
        self.view.lock().unwrap().error = None;
        self.sequence += 1;
        self.store.record(
            Event {
                key: &format!("{}:{}", self.process, self.sequence),
                device: &address(simulator::ADDRESS),
                connection: &format!("{}:{}", self.process, self.connection.generation),
                utc: self.utc_now(),
                monotonic_ns: Some(self.mono_now()),
                kind: "connected",
                uuid: "",
                raw: &[],
                detail: "Synthetic development pad connected; no movement command",
            },
            None,
        )?;
        let bytes = pad.lock().unwrap().packet(0);
        self.sim_raw(bytes, protocol::TREADMILL)
    }
    pub(super) fn advance_simulator(&mut self) -> Result<(), String> {
        let Some(pad) = self.simulator.clone() else {
            return Ok(());
        };
        if self.connection.enabled
            && self.link.is_none()
            && Instant::now() >= self.connection.next_try
        {
            self.sim_connect()?;
        }
        if self.link.is_none() {
            return Ok(());
        }
        let (raw, packet) = {
            let mut sim = pad.lock().unwrap();
            let raw = sim.raw.drain(..).collect::<Vec<_>>();
            let seconds = sim.last_tick.elapsed().as_secs().min(3600) as u32;
            let packet = if seconds > 0 {
                sim.last_tick = Instant::now();
                let bytes = sim.packet(seconds);
                sim.telemetry.then_some(bytes)
            } else {
                None
            };
            (raw, packet)
        };
        for (uuid, bytes) in raw {
            self.sim_raw(bytes, &uuid)?;
        }
        if let Some(bytes) = packet {
            self.sim_raw(bytes, protocol::TREADMILL)?;
        }
        Ok(())
    }
    fn fast_forward(&mut self, seconds: i64) -> Result<(), String> {
        if self.controls.exchange.is_some() || self.controls.pause_requested {
            return Err("Finish the current simulated command before changing virtual time".into());
        }
        let pad = self.simulator.clone().ok_or("Simulator unavailable")?;
        for _ in 0..seconds {
            let bytes = {
                let mut sim = pad.lock().unwrap();
                sim.offset += chrono::Duration::seconds(1);
                sim.packet(1)
            };
            self.sim_raw(bytes, protocol::TREADMILL)?;
        }
        self.store.expire_sessions(self.utc_now())?;
        self.store
            .set_setting("development_clock", &self.utc_now().to_rfc3339())?;
        self.view.lock().unwrap().dashboard = None;
        self.next_dashboard = Instant::now();
        Ok(())
    }
    pub(super) fn simulator_action(&mut self, action: SimAction) -> Result<(), String> {
        let Some(pad) = self.simulator.clone() else {
            return Ok(());
        };
        let result = (|| -> Result<(), String> {
            match action {
                SimAction::Connect => {
                    self.set_automatic_connection(true)?;
                    self.sim_connect()?;
                }
                SimAction::Disconnect => {
                    self.set_automatic_connection(false)?;
                    let generation = self.connection.generation;
                    self.device_event(DeviceEvent::Lost(generation))?;
                    self.status = "Synthetic pad disconnected".into();
                }
                SimAction::Scan => {
                    self.device_event(DeviceEvent::Found(
                        Candidate {
                            address: simulator::ADDRESS,
                            name: "Synthetic WalkingPad (development)".into(),
                        },
                        Instant::now(),
                    ))?;
                    self.view.lock().unwrap().simulator_status =
                        "Discovered one synthetic pad; no radio scan".into();
                }
                SimAction::ToggleTelemetry => {
                    let mut s = pad.lock().unwrap();
                    s.telemetry = !s.telemetry;
                }
                SimAction::RejectNext => pad.lock().unwrap().fault = simulator::Fault::Reject,
                SimAction::DelayNext => pad.lock().unwrap().fault = simulator::Fault::Delay,
                SimAction::TimeoutNext => pad.lock().unwrap().fault = simulator::Fault::Timeout,
                SimAction::FailWrite => pad.lock().unwrap().fault = simulator::Fault::Write,
                SimAction::NoMotionNext => pad.lock().unwrap().fault = simulator::Fault::NoMotion,
                SimAction::Malformed => {
                    self.sim_raw(vec![0, 0, 0], protocol::TREADMILL)?;
                }
                SimAction::ResetCounter => {
                    pad.lock().unwrap().steps = 0;
                    let bytes = pad.lock().unwrap().packet(0);
                    self.sim_raw(bytes, protocol::TREADMILL)?;
                }
                SimAction::PauseFiveMinutes | SimAction::DayBoundary => {
                    if self.link.is_none() {
                        return Err("Connect the synthetic pad first".into());
                    }
                    if self.controls.exchange.is_some() || self.controls.pause_requested {
                        return Err("Wait for the simulated command to finish".into());
                    }
                    pad.lock().unwrap().telemetry = true;
                    if action == SimAction::PauseFiveMinutes {
                        pad.lock().unwrap().speed = 0.0;
                        let bytes = pad.lock().unwrap().packet(0);
                        self.sim_raw(bytes, protocol::TREADMILL)?;
                        self.fast_forward(301)?;
                    } else {
                        let now = self.utc_now();
                        let tomorrow = now
                            .with_timezone(&self.options.timezone)
                            .date_naive()
                            .succ_opt()
                            .ok_or("Date overflow")?;
                        use chrono::TimeZone;
                        let midnight = self
                            .options
                            .timezone
                            .from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).unwrap())
                            .earliest()
                            .ok_or("Local midnight is unavailable in this timezone")?
                            .with_timezone(&Utc);
                        let jump = (midnight - now - chrono::Duration::seconds(5))
                            .max(chrono::Duration::zero());
                        pad.lock().unwrap().offset += jump;
                        pad.lock().unwrap().speed = 2.0;
                        self.fast_forward(12)?;
                    }
                }
                SimAction::ToggleShortcuts => {
                    let enabled = !self.view.lock().unwrap().development_shortcuts;
                    self.store.set_setting(
                        "development_shortcuts",
                        if enabled { "true" } else { "false" },
                    )?;
                    self.view.lock().unwrap().development_shortcuts = enabled;
                }
                SimAction::ResetData => {
                    self.controls.reset();
                    self.link = None;
                    development_data::reset(&mut self.store, Utc::now())?;
                    self.simulator = Some(Arc::new(Mutex::new(simulator::Simulator::new(None))));
                    self.connection = super::super::connection::ConnectionPolicy::new(
                        Some(simulator::ADDRESS),
                        true,
                        Instant::now(),
                    );
                    let mut v = self.view.lock().unwrap();
                    v.development_shortcuts = false;
                    v.shortcuts = Default::default();
                    v.theme_preference = Default::default();
                    v.recover_gaps = true;
                    v.dashboard = None;
                    v.error = None;
                    self.controls.increment = 1;
                    self.next_dashboard = Instant::now();
                }
            }
            Ok(())
        })();
        self.view.lock().unwrap().simulator_status = match result {
            Ok(()) => format!("{action:?} applied to synthetic pad"),
            Err(e) => e,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_ticks_sleep_but_control_freshness_and_telemetry_deadlines_still_win() {
        with_simulator(|r| {
            r.simulator.as_ref().unwrap().lock().unwrap().last_tick = Instant::now();
            assert!(r.delay() > Duration::from_millis(800));
            control(r, control::Command::StartResume);
            assert!(r.delay() <= Duration::from_millis(50));
            r.controls.reset();
            r.telemetry_watch.pending = Some((
                r.connection.generation,
                Instant::now() + Duration::from_millis(20),
            ));
            assert!(r.delay() <= Duration::from_millis(20));
            r.telemetry_watch.pending = None;
            r.controls.fresh_speed =
                Some((r.connection.generation, 0.0, r.mono_now() - 2_980_000_000));
            assert!(r.delay() <= Duration::from_millis(20));
            r.controls.fresh_speed = None;
            r.simulator_action(SimAction::Disconnect).unwrap();
            r.simulator.as_ref().unwrap().lock().unwrap().last_tick =
                Instant::now() - Duration::from_secs(10);
            assert!(
                r.delay() > Duration::from_millis(800),
                "a disconnected simulator must not busy-loop on an expired tick"
            );
        });
    }
    #[test]
    fn recovery_setting_is_default_on_and_published_only_after_persistence() {
        with_simulator(|r| {
            r.publish().unwrap();
            assert!(r.view.lock().unwrap().recover_gaps);
            r.user_command(UserCommand::RecoverGaps(false)).unwrap();
            r.publish().unwrap();
            assert!(!r.view.lock().unwrap().recover_gaps);
            assert_eq!(
                r.store.setting("recover_gaps").unwrap().as_deref(),
                Some("false")
            );
            r.store
                .conn
                .pragma_update(None, "query_only", true)
                .unwrap();
            assert!(r.user_command(UserCommand::RecoverGaps(true)).is_err());
            assert!(!r.view.lock().unwrap().recover_gaps);
        });
    }
    fn with_simulator(test: impl FnOnce(&mut Runtime<'_>)) {
        let now = Instant::now();
        let (tx, rx) = mpsc::sync_channel(512);
        let tx = MessageSender(tx);
        let overflow = Arc::new(AtomicBool::new(false));
        let shared_target = Arc::new(Mutex::new(Some(simulator::ADDRESS)));
        let mut store = Store::initialize(
            rusqlite::Connection::open_in_memory().unwrap(),
            chrono_tz::Europe::Helsinki,
        )
        .unwrap();
        development_data::initialize(&mut store, Utc::now()).unwrap();
        let mut runtime = Runtime {
            simulator: Some(Arc::new(Mutex::new(simulator::Simulator::new(None)))),
            connection: super::super::super::connection::ConnectionPolicy::new(
                Some(simulator::ADDRESS),
                true,
                now,
            ),
            options: Options {
                database_ownership: None,
                database: "unused-synthetic.sqlite".into(),
                timezone: chrono_tz::Europe::Helsinki,
                device: None,
                no_bluetooth: true,
                development: true,
                smoke_seconds: None,
                controls_enabled: true,
            },
            notify: &|| {},
            view: Arc::new(Mutex::new(View {
                development: true,
                no_bluetooth: true,
                controls_enabled: true,
                ..Default::default()
            })),
            discovery: Discovery::new(false, tx.clone(), overflow.clone(), shared_target.clone())
                .unwrap(),
            tx,
            rx,
            overflow,
            shared_target,
            store,
            cancel: Cancellation::default(),
            origin: now,
            process: "synthetic-runtime-test".into(),
            link: None,
            sequence: 0,
            telemetry_watch: TelemetryWatch::default(),
            controls: super::super::super::controls::Controls::new(1),
            dashboard_request: None,
            next_dashboard: now,
            next_session_check: now,
            status: String::new(),
        };
        super::super::load_preferences(&runtime.store, &mut runtime.view.lock().unwrap()).unwrap();
        runtime.advance_simulator().unwrap();
        assert!(!runtime.discovery.available());
        test(&mut runtime);
    }
    fn control(runtime: &mut Runtime<'_>, command: control::Command) {
        runtime
            .user_command(UserCommand::Control {
                command,
                generation: runtime.connection.generation,
            })
            .unwrap();
    }
    fn exchange(runtime: &mut Runtime<'_>) {
        for _ in 0..2 {
            std::thread::sleep(Duration::from_millis(100));
            runtime.advance_controls().unwrap();
        }
        let pad = runtime.simulator.clone().unwrap();
        let bytes = pad.lock().unwrap().packet(1);
        runtime.sim_raw(bytes, protocol::TREADMILL).unwrap();
        runtime.publish().unwrap();
    }
    fn writes(runtime: &Runtime<'_>) -> i64 {
        runtime
            .store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='control_write'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
    #[test]
    fn controls_use_real_exchange_and_accounting_pause_groups_and_midnight_uses_local_time() {
        with_simulator(|r| {
            assert_eq!(r.simulator.as_ref().unwrap().lock().unwrap().speed, 0.0);
            assert_eq!(writes(r), 0); // no movement on connection
            control(r, control::Command::StartResume);
            exchange(r);
            assert!(r.controls.exchange.is_none());
            assert_eq!(writes(r), 2); // access and action are distinct
            r.fast_forward(4).unwrap();
            assert!(r.controls.fresh_speed.is_some());
            control(r, control::Command::Increase);
            exchange(r);
            assert_eq!(r.simulator.as_ref().unwrap().lock().unwrap().speed, 1.1);
            r.fast_forward(2).unwrap(); // settle the newly observed speed
            control(r, control::Command::Increase);
            r.advance_controls().unwrap(); // begin the speed GATT write
            control(r, control::Command::Pause); // Pause waits for access boundary
            assert!(r.controls.pause_requested);
            for _ in 0..4 {
                exchange(r);
            }
            assert_eq!(r.simulator.as_ref().unwrap().lock().unwrap().speed, 0.0);
            r.simulator_action(SimAction::PauseFiveMinutes).unwrap();
            let unfinished: i64 = r
                .store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM ui_sessions WHERE state!='Finished'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(unfinished, 0);
            let before = r.utc_now().with_timezone(&r.options.timezone).date_naive();
            r.simulator_action(SimAction::DayBoundary).unwrap();
            assert_eq!(
                r.utc_now().with_timezone(&r.options.timezone).date_naive(),
                before.succ_opt().unwrap()
            );
            assert!(r.store.totals(r.utc_now()).unwrap().recorded_steps > 0);
            let saved = DateTime::parse_from_rfc3339(
                &r.store.setting("development_clock").unwrap().unwrap(),
            )
            .unwrap()
            .with_timezone(&Utc);
            let restored = simulator::Simulator::new(Some(saved));
            assert!((Utc::now() + restored.offset) >= saved);
        });
    }
    #[test]
    fn faults_never_retry_and_reconnect_clears_uncertainty_without_starting_motion() {
        with_simulator(|r| {
            for action in [
                SimAction::RejectNext,
                SimAction::TimeoutNext,
                SimAction::FailWrite,
                SimAction::NoMotionNext,
            ] {
                r.simulator_action(SimAction::Connect).unwrap();
                r.simulator.as_ref().unwrap().lock().unwrap().speed = 0.0;
                r.simulator_action(action).unwrap();
                control(r, control::Command::StartResume);
                exchange(r);
                r.simulator.as_ref().unwrap().lock().unwrap().offset +=
                    chrono::Duration::seconds(20);
                r.advance_controls().unwrap();
                let count = writes(r);
                for _ in 0..4 {
                    r.advance_controls().unwrap();
                }
                assert_eq!(writes(r), count);
                assert!(r.controls.exchange.is_none());
                assert_eq!(r.simulator.as_ref().unwrap().lock().unwrap().speed, 0.0);
                r.simulator_action(SimAction::Disconnect).unwrap();
                assert!(r.link.is_none());
                r.simulator_action(SimAction::Connect).unwrap();
                assert!(!r.link.as_ref().unwrap().control_uncertain);
            }
            r.simulator_action(SimAction::DelayNext).unwrap();
            control(r, control::Command::StartResume);
            r.advance_controls().unwrap();
            assert!(r.controls.exchange.is_some());
            assert_eq!(r.simulator.as_ref().unwrap().lock().unwrap().speed, 0.0);
        });
    }
    #[test]
    fn rapid_speed_clicks_send_each_target_before_acknowledgements_and_measured_motion() {
        with_simulator(|r| {
            control(r, control::Command::StartResume);
            exchange(r);
            assert!(r.controls.access_granted);
            let before = writes(r);
            r.simulator_action(SimAction::DelayNext).unwrap();
            for command in [
                control::Command::Increase,
                control::Command::Increase,
                control::Command::Decrease,
            ] {
                control(r, command);
                r.advance_controls().unwrap();
            }
            r.advance_controls().unwrap();
            assert_eq!(writes(r), before + 3); // one absolute speed write per click, no access round trips
            let frames: Vec<Vec<u8>> = r
                .store
                .conn
                .prepare("SELECT raw FROM events WHERE kind='control_write' ORDER BY rowid")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                &frames[2..],
                &[vec![2, 110, 0], vec![2, 120, 0], vec![2, 110, 0]]
            );
            assert!(r.controls.exchange.is_none());
            r.publish().unwrap();
            let v = r.view.lock().unwrap().clone();
            assert_eq!(v.fresh_speed, Some(1.0)); // no new measured telemetry supplied
            assert_eq!(v.selected_speed, Some(1.1));
            assert!(v.speed_available);
            for _ in 0..4 {
                r.advance_controls().unwrap();
            }
            assert_eq!(writes(r), before + 3); // no automatic retry
            let bytes = r.simulator.as_ref().unwrap().lock().unwrap().packet(0);
            r.sim_raw(bytes, protocol::TREADMILL).unwrap();
            assert!(r.controls.selected_speed.is_none());
        });
    }
    #[test]
    fn pause_discards_unsent_speed_clicks_and_late_speed_rejection_cannot_cancel_pause() {
        with_simulator(|r| {
            control(r, control::Command::StartResume);
            exchange(r);
            control(r, control::Command::Increase);
            r.advance_controls().unwrap();
            let before = writes(r);
            control(r, control::Command::Increase);
            control(r, control::Command::Increase);
            control(r, control::Command::Pause);
            r.advance_controls().unwrap();
            assert_eq!(writes(r), before + 1); // Pause access, neither queued speed
            assert_eq!(
                r.controls.exchange.as_ref().unwrap().transaction.command(),
                control::Command::Pause
            );
            r.sim_raw(vec![0x80, 2, 4], control::UUID).unwrap();
            assert!(r.link.as_ref().unwrap().control_uncertain);
            assert_eq!(
                r.controls.exchange.as_ref().unwrap().transaction.command(),
                control::Command::Pause
            );
            exchange(r);
            assert_eq!(r.simulator.as_ref().unwrap().lock().unwrap().speed, 0.0);
            assert!(r.controls.exchange.is_none());
        });
    }
    #[test]
    fn speed_write_failure_and_unobserved_target_report_uncertainty_without_retry() {
        for fault in [SimAction::FailWrite, SimAction::NoMotionNext] {
            with_simulator(|r| {
                control(r, control::Command::StartResume);
                exchange(r);
                r.simulator_action(fault).unwrap();
                control(r, control::Command::Increase);
                r.advance_controls().unwrap();
                r.advance_controls().unwrap();
                r.simulator.as_ref().unwrap().lock().unwrap().offset +=
                    chrono::Duration::seconds(16);
                r.advance_controls().unwrap();
                assert!(r.link.as_ref().unwrap().control_uncertain);
                assert!(r.controls.selected_speed.is_none());
                let count = writes(r);
                control(r, control::Command::Increase);
                r.advance_controls().unwrap();
                assert_eq!(writes(r), count);
            });
        }
    }
    #[test]
    fn panel_scenarios_preserve_raw_errors_disable_shortcuts_and_reset_only_synthetic_data() {
        with_simulator(|r| {
            r.simulator_action(SimAction::Scan).unwrap();
            assert_eq!(r.connection.candidates.len(), 1);
            r.simulator_action(SimAction::ToggleTelemetry).unwrap();
            assert!(!r.simulator.as_ref().unwrap().lock().unwrap().telemetry);
            r.telemetry_watch.started(
                r.connection.generation,
                Instant::now() - Duration::from_secs(9),
            );
            r.check_recording().unwrap();
            assert_eq!(
                r.view.lock().unwrap().error.as_deref(),
                Some(MISSING_TELEMETRY)
            );
            r.simulator_action(SimAction::Malformed).unwrap();
            assert!(
                r.view
                    .lock()
                    .unwrap()
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("decode")
            );
            r.simulator_action(SimAction::ResetCounter).unwrap();
            assert_eq!(r.simulator.as_ref().unwrap().lock().unwrap().steps, 0);
            assert!(!r.view.lock().unwrap().development_shortcuts);
            r.simulator_action(SimAction::ToggleShortcuts).unwrap();
            assert!(r.view.lock().unwrap().development_shortcuts);
            r.simulator_action(SimAction::ResetData).unwrap();
            assert!(!r.view.lock().unwrap().development_shortcuts);
            assert!(r.link.is_none());
            assert_eq!(writes(r), 0);
            assert_eq!(
                r.store
                    .setting("development_seed_complete")
                    .unwrap()
                    .as_deref(),
                Some("1")
            );
            r.advance_simulator().unwrap();
            assert!(r.link.is_some());
        });
    }
}
