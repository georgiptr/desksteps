use crate::{
    Options, address, control, protocol,
    storage::{Event, Store, Totals},
};
use chrono::{DateTime, Utc};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
mod api;
mod bluetooth;
mod connection;
mod controls;
mod engine;
mod exchange;
mod messages;
mod ports;
use api::MessageSender;
pub(crate) use api::{CommandSender, SnapshotReader};
mod development_data;
mod simulator;
mod telemetry;
mod transport;
use bluetooth::*;
pub(crate) use simulator::{ACTIONS as SIM_ACTIONS, SimAction};
use transport::{Link, WindowsWrite};
type Exchange = exchange::Exchange<WindowsWrite>;
use engine::work;
pub(crate) use messages::{Candidate, UserCommand, View};
use messages::{DeviceEvent, Input};
use telemetry::*;
pub const UPDATE_MESSAGE: u32 = 0x8001;
const MACHINE_STATUS: &str = "00002ada-0000-1000-8000-00805f9b34fb";

pub struct Worker {
    pub sender: CommandSender,
    pub view: SnapshotReader,
    cancellation: Cancellation,
    pub join: std::thread::JoinHandle<()>,
}
pub fn start(options: Options, notify: impl Fn() + Send + Sync + 'static) -> Worker {
    let (tx, rx) = mpsc::sync_channel(512);
    let tx = MessageSender(tx);
    let view = Arc::new(Mutex::new(View {
        no_bluetooth: options.no_bluetooth,
        development: options.development,
        controls_enabled: options.controls_enabled
            && (!options.no_bluetooth || options.development),
        increment: 1,
        ..Default::default()
    }));
    let cancellation = Cancellation::default();
    let cancel = cancellation.clone();
    let shared = view.clone();
    let sender = tx.clone();
    let join = std::thread::spawn(move || {
        if let Err(error) = work(options, &notify, shared.clone(), sender, rx, cancel) {
            {
                let mut v = shared.lock().unwrap();
                v.status = "Recording stopped".into();
                v.error = Some(error);
                v.connected = false;
            }
            notify();
        }
    });
    Worker {
        cancellation,
        sender: CommandSender(tx),
        view: SnapshotReader(view),
        join,
    }
}
impl Worker {
    pub fn cancel_setup(&self) {
        self.cancellation.cancel();
    }
}
#[derive(Clone, Default)]
pub(super) struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub(super) fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub(super) fn reset(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
    pub(super) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn development_worker_wakes_for_commands_and_expires_speed_without_telemetry() {
        let folder = std::env::temp_dir().join(format!(
            "desksteps-worker-wakeup-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let database = folder.join("synthetic.sqlite");
        let worker = start(
            Options {
                database_ownership: None,
                database: database.clone(),
                timezone: chrono_tz::UTC,
                device: None,
                no_bluetooth: true,
                development: true,
                smoke_seconds: None,
                controls_enabled: true,
            },
            || {},
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let wait_for = |condition: &dyn Fn(&View) -> bool, timeout: Duration| {
                let deadline = Instant::now() + timeout;
                loop {
                    let v = worker.view.snapshot();
                    assert!(v.error.is_none(), "unexpected worker error: {:?}", v.error);
                    if condition(&v) {
                        return v;
                    }
                    assert!(Instant::now() < deadline, "timed out: {v:?}");
                    std::thread::sleep(Duration::from_millis(10));
                }
            };
            let wait =
                |condition: &dyn Fn(&View) -> bool| wait_for(condition, Duration::from_secs(10));
            // Cold startup seeds seven days of synthetic history on disk. Hosted
            // runners need more time for that than for subsequent commands.
            let initial = wait_for(
                &|v| v.connected && v.fresh_speed == Some(0.0),
                Duration::from_secs(60),
            );
            let generation = initial.generation;
            let send = |command| {
                worker
                    .sender
                    .send(UserCommand::Control {
                        command,
                        generation,
                    })
                    .unwrap()
            };
            send(control::Command::StartResume);
            wait(&|v| v.fresh_speed.is_some_and(|speed| speed > 0.0) && !v.control_busy);
            let started = worker.view.snapshot().totals.recorded_steps;
            wait(&|v| v.totals.recorded_steps > started);
            send(control::Command::Increase);
            wait(&|v| v.fresh_speed == Some(1.1) && !v.control_busy);
            worker
                .sender
                .send(UserCommand::Simulator(SimAction::ToggleTelemetry))
                .unwrap();
            wait(&|v| v.fresh_speed.is_none() && v.adjustable_speed.is_none());
            worker
                .sender
                .send(UserCommand::Simulator(SimAction::ToggleTelemetry))
                .unwrap();
            wait(&|v| v.fresh_speed == Some(1.1));
            worker
                .sender
                .send(UserCommand::Simulator(SimAction::DelayNext))
                .unwrap();
            send(control::Command::Increase);
            wait(&|v| v.control_busy);
            send(control::Command::Pause);
            wait(&|v| v.fresh_speed == Some(0.0) && !v.control_busy);
            worker
                .sender
                .send(UserCommand::Simulator(SimAction::Disconnect))
                .unwrap();
            wait(&|v| !v.connected);
            worker
                .sender
                .send(UserCommand::Simulator(SimAction::Connect))
                .unwrap();
            wait(&|v| v.connected && v.generation > generation && v.fresh_speed == Some(0.0));
        }));
        let _ = worker.sender.send(UserCommand::Exit);
        worker.join.join().unwrap();
        result.unwrap();
        assert!(std::fs::metadata(database).unwrap().len() > 0);
    }
    #[test]
    fn saved_device_name_is_published_after_worker_restart_without_discovery() {
        let folder = std::env::temp_dir().join(format!(
            "desksteps-device-name-worker-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let database = folder.join("synthetic.sqlite");
        {
            let mut store = Store::open(&database, chrono_tz::UTC).unwrap();
            store.save_device(1, Some("Synthetic named pad")).unwrap();
        }
        for device in [None, Some(2), Some(1)] {
            let worker = start(
                Options {
                    database_ownership: None,
                    database: database.clone(),
                    timezone: chrono_tz::UTC,
                    device,
                    no_bluetooth: true,
                    development: false,
                    smoke_seconds: None,
                    controls_enabled: false,
                },
                || {},
            );
            worker.sender.send(UserCommand::Exit).unwrap();
            worker.join.join().unwrap();
            let view = worker.view.snapshot();
            assert!(view.error.is_none(), "{:?}", view.error);
            assert!(view.candidates.is_empty());
            assert!(!view.connected);
            assert_eq!(view.selected, device.or(Some(1)));
            assert_eq!(
                view.selected_name.as_deref(),
                if device == Some(2) {
                    None
                } else {
                    Some("Synthetic named pad")
                }
            );
        }
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn theme_is_published_and_persisted_across_worker_restarts() {
        use crate::appearance::ThemePreference;
        let folder = std::env::temp_dir().join(format!(
            "desksteps-theme-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let database = folder.join("test.sqlite");
        let options = || Options {
            database_ownership: None,
            database: database.clone(),
            timezone: chrono_tz::UTC,
            device: None,
            no_bluetooth: true,
            development: false,
            smoke_seconds: None,
            controls_enabled: false,
        };
        for preference in ThemePreference::ALL {
            let worker = start(options(), || {});
            worker.sender.send(UserCommand::Theme(preference)).unwrap();
            worker.sender.send(UserCommand::Exit).unwrap();
            worker.join.join().unwrap();
            assert!(worker.view.snapshot().error.is_none());
            assert_eq!(worker.view.snapshot().theme_preference, preference);
            let worker = start(options(), || {});
            worker.sender.send(UserCommand::Exit).unwrap();
            worker.join.join().unwrap();
            assert!(worker.view.snapshot().error.is_none());
            assert_eq!(worker.view.snapshot().theme_preference, preference);
            assert!(!worker.view.snapshot().connected);
        }
    }
    #[test]
    fn window_preferences_survive_restart_and_offline_modes_ignore_windows_startup() {
        use crate::window_behavior::{Preference, WindowBehavior};
        let folder = std::env::temp_dir().join(format!(
            "desksteps-window-preferences-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        for development in [false, true] {
            let database = folder.join(format!("synthetic-{development}.sqlite"));
            let options = || Options {
                database_ownership: None,
                database: database.clone(),
                timezone: chrono_tz::UTC,
                device: None,
                no_bluetooth: true,
                development,
                smoke_seconds: None,
                controls_enabled: false,
            };
            for expected in [
                WindowBehavior::default(),
                WindowBehavior {
                    start_minimized: false,
                    minimize_to_tray: true,
                    keep_running_on_close: false,
                },
                WindowBehavior::default(),
            ] {
                let worker = start(options(), || {});
                worker
                    .sender
                    .send(UserCommand::WindowPreference(Preference::StartMinimized(
                        expected.start_minimized,
                    )))
                    .unwrap();
                worker
                    .sender
                    .send(UserCommand::WindowPreference(Preference::MinimizeToTray(
                        expected.minimize_to_tray,
                    )))
                    .unwrap();
                worker
                    .sender
                    .send(UserCommand::WindowPreference(
                        Preference::KeepRunningOnClose(expected.keep_running_on_close),
                    ))
                    .unwrap();
                worker
                    .sender
                    .send(UserCommand::WindowsStartup(true))
                    .unwrap();
                worker.sender.send(UserCommand::Exit).unwrap();
                worker.join.join().unwrap();
                let view = worker.view.snapshot();
                assert!(view.error.is_none(), "{:?}", view.error);
                assert_eq!(view.window_behavior, expected);
                assert!(!view.windows_startup && !view.windows_startup_available);
                let worker = start(options(), || {});
                worker.sender.send(UserCommand::Exit).unwrap();
                worker.join.join().unwrap();
                assert!(worker.view.snapshot().error.is_none());
                assert_eq!(worker.view.snapshot().window_behavior, expected);
            }
        }
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn automatic_connection_checkbox_survives_worker_restarts_and_disconnect_clears_it() {
        let folder = std::env::temp_dir().join(format!(
            "desksteps-automatic-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let database = folder.join("test.sqlite");
        for (expected_at_start, command) in [
            (true, Some(UserCommand::AutomaticConnection(false))),
            (false, Some(UserCommand::AutomaticConnection(true))),
            (true, Some(UserCommand::Disconnect)),
            (false, None),
        ] {
            let worker = start(
                Options {
                    database_ownership: None,
                    database: database.clone(),
                    timezone: chrono_tz::UTC,
                    device: None,
                    no_bluetooth: true,
                    development: false,
                    smoke_seconds: None,
                    controls_enabled: false,
                },
                || {},
            );
            // Queue Exit after startup, then inspect the final published view.
            worker.sender.send(UserCommand::Exit).unwrap();
            worker.join.join().unwrap();
            assert_eq!(
                worker.view.snapshot().automatic_connection,
                expected_at_start
            );
            assert!(!worker.view.snapshot().connected);
            if let Some(command) = command {
                let worker = start(
                    Options {
                        database_ownership: None,
                        database: database.clone(),
                        timezone: chrono_tz::UTC,
                        device: None,
                        no_bluetooth: true,
                        development: false,
                        smoke_seconds: None,
                        controls_enabled: false,
                    },
                    || {},
                );
                worker.sender.send(command).unwrap();
                worker.sender.send(UserCommand::Exit).unwrap();
                worker.join.join().unwrap();
            }
        }
    }
    #[test]
    fn preview_worker_rejects_movement_and_stale_samples_without_any_ble_connection() {
        let folder = std::env::temp_dir().join(format!(
            "desksteps-worker-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let database = folder.join("test.sqlite");
        let worker = start(
            Options {
                database_ownership: None,
                database: database.clone(),
                timezone: chrono_tz::Europe::Helsinki,
                device: None,
                no_bluetooth: true,
                development: false,
                smoke_seconds: None,
                controls_enabled: true,
            },
            || {},
        );
        for input in Vec::<Input>::from([
            DeviceEvent::Found(
                Candidate {
                    address: 0x112233445566,
                    name: protocol::MODEL_NAME.into(),
                },
                Instant::now(),
            )
            .into(),
            UserCommand::Select(0x112233445566).into(),
            UserCommand::AutomaticConnection(true).into(),
            UserCommand::ActiveScan.into(),
            UserCommand::Control {
                command: control::Command::StartResume,
                generation: 0,
            }
            .into(),
            DeviceEvent::ScanStopped {
                error: "synthetic radio unavailable".into(),
                interrupted: true,
            }
            .into(),
            DeviceEvent::Lost(99).into(),
            UserCommand::Disconnect.into(),
        ]) {
            worker.sender.0.send(input).unwrap();
        }
        worker
            .sender
            .0
            .send(DeviceEvent::Raw {
                generation: 99,
                device: 0x112233445566,
                utc: Utc::now(),
                mono: 1,
                kind: "notification",
                uuid: protocol::TREADMILL.into(),
                bytes: vec![0],
                detail: String::new(),
            })
            .unwrap();
        worker.sender.send(UserCommand::Exit).unwrap();
        worker.join.join().unwrap();
        let conn = rusqlite::Connection::open(&database).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE kind='control_write'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE kind='control_error'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM samples", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE kind='scan_stopped'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE kind='recorder_exited'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        drop(conn);
        std::fs::remove_file(&database).unwrap();
        std::fs::remove_dir(&folder).unwrap();
    }

    #[test]
    fn name_resolution_cannot_accept_an_unverified_model() {
        let target = 0x1122_3344_aabb;
        assert!(matches!(
            verify_device_name("", target),
            Err(ConnectError::NameUnavailable(_))
        ));
        assert!(verify_device_name(protocol::MODEL_NAME, target).is_ok());
        for name in ["Other treadmill", "KS-HD-Z1D-other", " KS-HD-Z1D", " "] {
            assert!(matches!(
                verify_device_name(name, target),
                Err(ConnectError::Other(_))
            ));
        }
    }

    #[test]
    fn windows_address_placeholder_requires_resolution_and_never_counts_as_a_model() {
        let target = 0x1122_3344_aabb;
        for name in ["Bluetooth 11:22:33:44:aa:bb", "Bluetooth 11:22:33:44:AA:BB"] {
            assert!(matches!(
                verify_device_name(name, target),
                Err(ConnectError::NameUnavailable(_))
            ));
        }
        for name in [
            "Bluetooth 11:22:33:44:aa:bc",
            "Bluetooth invalid",
            "Bluetooth KS-HD-Z1D",
        ] {
            assert!(matches!(
                verify_device_name(name, target),
                Err(ConnectError::Other(_))
            ));
        }
    }

    #[test]
    fn start_without_data_reports_missing_once_and_repeated_status_cannot_delay_it() {
        let now = Instant::now();
        let mut watch = TelemetryWatch::default();
        watch.started(2, now);
        watch.started(2, now + Duration::from_secs(7));
        assert!(!watch.missing(Some(2), now + Duration::from_secs(7)));
        assert!(watch.missing(Some(2), now + Duration::from_secs(8)));
        assert!(!watch.missing(Some(2), now + Duration::from_secs(20)));
    }

    #[test]
    fn only_current_connection_data_clears_start_expectation() {
        let now = Instant::now();
        let mut watch = TelemetryWatch::default();
        watch.started(2, now);
        watch.received(1);
        assert!(watch.missing(Some(2), now + Duration::from_secs(8)));
        watch.started(3, now);
        watch.received(3);
        assert!(!watch.missing(Some(3), now + Duration::from_secs(9)));
        watch.started(4, now);
        assert!(!watch.missing(None, now + Duration::from_secs(9)));
        watch.started(4, now);
        assert!(!watch.missing(Some(5), now + Duration::from_secs(9)));
    }
}

#[cfg(test)]
pub(crate) use api::testing;

#[cfg(test)]
#[path = "tests/transport.rs"]
mod test_transport;

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[test]
    fn cancellation_is_shared_only_within_one_worker_and_can_be_reset() {
        let first = Cancellation::default();
        let shared = first.clone();
        let second = Cancellation::default();
        first.cancel();
        assert!(shared.is_cancelled());
        assert!(!second.is_cancelled());
        shared.reset();
        assert!(!first.is_cancelled());
    }
}
