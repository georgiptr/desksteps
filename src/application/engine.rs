//! Worker event loop. This is the only owner of the store and active exchange.
use super::*;
mod commands;
mod connection;
mod controls;
mod events;
mod publication;
mod recording;
mod simulation;

pub(super) fn work(
    mut options: Options,
    notify: &impl Fn(),
    view: Arc<Mutex<View>>,
    tx: MessageSender,
    rx: Receiver<Input>,
    cancel: Cancellation,
) -> Result<(), String> {
    // Synthetic/offline work never creates WinRT Bluetooth objects.
    let _apartment = if options.no_bluetooth {
        None
    } else {
        Some(Apartment::enter()?)
    };
    let mut store = Store::open_owned(
        &options.database,
        options.timezone,
        options.database_ownership.take(),
    )?;
    let simulator = if options.development {
        if !options.no_bluetooth {
            return Err("Development requires Bluetooth disabled".into());
        }
        development_data::initialize(&mut store, Utc::now())?;
        let saved = store
            .setting("development_clock")?
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|s| s.with_timezone(&Utc)))
            .transpose()
            .map_err(|e| e.to_string())?;
        Some(Arc::new(Mutex::new(simulator::Simulator::new(saved))))
    } else {
        None
    };
    let target = if options.development {
        Some(simulator::ADDRESS)
    } else {
        options.device.or(store
            .setting("device")?
            .map(|s| crate::parse_address(&s))
            .transpose()?)
    };
    if let Some(target) = options.device {
        store.save_device(target, None)?;
    }
    let automatic_connection = store.setting("automatic_connection")?.as_deref() != Some("false");
    let initial_utc = Utc::now()
        + simulator
            .as_ref()
            .map(|s| s.lock().unwrap().offset)
            .unwrap_or_default();
    let origin = Instant::now();
    let process = Utc::now().timestamp_nanos_opt().unwrap().to_string();
    store.record(
        Event {
            key: &format!("{process}:0"),
            device: &target.map(address).unwrap_or_default(),
            connection: &format!("{process}:0"),
            utc: initial_utc,
            monotonic_ns: Some(0),
            kind: "recorder_started",
            uuid: "",
            raw: &[],
            detail: if options.development {
                "Synthetic development recorder; Bluetooth disabled"
            } else if options.no_bluetooth {
                "Bluetooth disabled"
            } else if automatic_connection {
                "App launch; automatic connection enabled; movement requires explicit user action"
            } else {
                "App launch; automatic connection disabled; movement requires explicit user action"
            },
        },
        None,
    )?;
    let overflow = Arc::new(AtomicBool::new(false));
    let shared_target = Arc::new(Mutex::new(target));
    let discovery = Discovery::new(
        !options.no_bluetooth && !options.development,
        tx.clone(),
        overflow.clone(),
        shared_target.clone(),
    )?;
    let link: Option<Link> = None;
    let sequence = 0u64;
    let mut connection = super::connection::ConnectionPolicy::new(
        target,
        automatic_connection && (!options.no_bluetooth || options.development),
        Instant::now(),
    );
    connection.automatic_connection = automatic_connection;
    connection.target_name = target.map(|a| store.device_name(a)).transpose()?.flatten();
    let telemetry_watch = TelemetryWatch::default();
    let increment = store
        .setting("speed_increment")?
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|n| (1..=10).contains(n))
        .unwrap_or(1);
    load_preferences(&store, &mut view.lock().unwrap())?;
    let controls = super::controls::Controls::<WindowsWrite>::new(increment);
    let dashboard_request: Option<crate::statistics::Request> = None;
    let next_dashboard = Instant::now();
    let next_session_check = Instant::now();
    let status: String = if options.no_bluetooth {
        "Preview: Bluetooth disabled".into()
    } else if automatic_connection {
        "Searching for WalkingPad".into()
    } else {
        "Automatic connection disabled".into()
    };
    Runtime {
        simulator,
        connection,
        options,
        notify,
        view,
        tx,
        rx,
        cancel,
        store,
        origin,
        process,
        overflow,
        shared_target,
        discovery,
        link,
        sequence,
        telemetry_watch,
        controls,
        dashboard_request,
        next_dashboard,
        next_session_check,
        status,
    }
    .run()
}
fn load_preferences(store: &Store, view: &mut View) -> Result<(), String> {
    view.window_behavior.start_minimized =
        store.setting("start_minimized")?.as_deref() != Some("false");
    view.window_behavior.minimize_to_tray =
        store.setting("minimize_to_tray")?.as_deref() == Some("true");
    view.window_behavior.keep_running_on_close =
        store.setting("keep_running_on_close")?.as_deref() != Some("false");
    view.windows_startup_available = !view.no_bluetooth && !view.development;
    if view.windows_startup_available {
        match crate::windows_startup::enabled() {
            Ok(enabled) => view.windows_startup = enabled,
            Err(error) => view.startup_error = Some(error),
        }
    }
    view.shortcuts = store
        .setting("shortcuts")?
        .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or_default();
    view.development_shortcuts =
        view.development && store.setting("development_shortcuts")?.as_deref() == Some("true");
    view.recover_gaps = store.setting("recover_gaps")?.as_deref() != Some("false");
    view.theme_preference =
        crate::appearance::ThemePreference::from_setting(store.setting("theme")?.as_deref());
    Ok(())
}
struct Runtime<'a> {
    simulator: Option<Arc<Mutex<simulator::Simulator>>>,
    connection: super::connection::ConnectionPolicy,
    options: Options,
    notify: &'a dyn Fn(),
    view: Arc<Mutex<View>>,
    tx: MessageSender,
    rx: Receiver<Input>,
    cancel: Cancellation,
    store: Store,
    origin: Instant,
    process: String,
    overflow: Arc<AtomicBool>,
    shared_target: Arc<Mutex<Option<u64>>>,
    discovery: Discovery,
    link: Option<Link>,
    sequence: u64,
    telemetry_watch: TelemetryWatch,
    controls: super::controls::Controls<WindowsWrite>,
    dashboard_request: Option<crate::statistics::Request>,
    next_dashboard: Instant,
    next_session_check: Instant,
    status: String,
}
impl Runtime<'_> {
    fn run(mut self) -> Result<(), String> {
        loop {
            self.advance_simulator()?;
            self.advance_controls()?;
            if self
                .connection
                .active_until
                .is_some_and(|until| Instant::now() >= until)
                && !self.drain_scan_events()?
            {
                break;
            }
            self.check_recording()?;
            self.maintain_connection()?;
            self.refresh_dashboard()?;
            self.publish()?;
            let input = match self.rx.recv_timeout(self.delay()) {
                Ok(input) => input,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if !self.dispatch(input)? {
                break;
            }
        }
        self.finish()
    }
    fn dispatch(&mut self, input: Input) -> Result<bool, String> {
        match input {
            Input::User(command) => self.user_command(command),
            Input::Device(event) => self.device_event(event),
        }
    }
    fn drain_scan_events(&mut self) -> Result<bool, String> {
        // Include queued advertisements/scan failures before deciding that
        // only one pad was seen. Receipt times retain the scan's boundaries.
        for _ in 0..512 {
            match self.rx.try_recv() {
                Ok(input) => {
                    if !self.dispatch(input)? {
                        return Ok(false);
                    }
                }
                Err(_) => return Ok(true),
            }
        }
        // Never choose automatically from a potentially incomplete batch.
        self.connection.invalidate_scan();
        Ok(true)
    }
}
