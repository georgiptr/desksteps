//! Read-only snapshots and typed user command submission. No UI mutates worker state.
use super::*;
#[derive(Clone)]
pub(super) struct MessageSender(pub(super) SyncSender<Input>);
impl MessageSender {
    pub(super) fn try_send(&self, input: impl Into<Input>) -> Result<(), String> {
        self.0.try_send(input.into()).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => "Recorder busy; input was not queued".into(),
            mpsc::TrySendError::Disconnected(_) => {
                "Recorder unavailable; input was not queued".into()
            }
        })
    }
    pub(super) fn send(&self, input: impl Into<Input>) -> Result<(), String> {
        self.0
            .send(input.into())
            .map_err(|_| "Recorder unavailable; input was not queued".into())
    }
}
#[derive(Clone)]
pub(crate) struct CommandSender(pub(super) MessageSender);
impl CommandSender {
    pub fn try_send(&self, input: UserCommand) -> Result<(), String> {
        self.0.try_send(input)
    }
    pub fn send(&self, input: UserCommand) -> Result<(), String> {
        self.0.send(input)
    }
}
#[derive(Clone)]
pub(crate) struct SnapshotReader(pub(super) Arc<Mutex<View>>);
impl SnapshotReader {
    pub fn snapshot(&self) -> View {
        self.0.lock().unwrap().clone()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    pub(crate) fn inert_worker(initial: View) -> (Worker, Commands) {
        let (sender, view, _, commands) = channels(32, initial);
        (
            Worker {
                sender,
                view,
                cancellation: Cancellation::default(),
                join: std::thread::spawn(|| {}),
            },
            commands,
        )
    }
    pub(crate) struct Commands(Receiver<Input>);
    impl Commands {
        pub fn recv(&self) -> Result<UserCommand, mpsc::RecvError> {
            match self.0.recv()? {
                Input::User(command) => Ok(command),
                Input::Device(_) => panic!("UI emitted a device callback"),
            }
        }
    }
    pub(crate) fn channels(
        capacity: usize,
        initial: View,
    ) -> (CommandSender, SnapshotReader, Arc<Mutex<View>>, Commands) {
        let (tx, rx) = mpsc::sync_channel(capacity);
        let raw = Arc::new(Mutex::new(initial));
        (
            CommandSender(MessageSender(tx)),
            SnapshotReader(raw.clone()),
            raw,
            Commands(rx),
        )
    }
    pub(crate) fn publish(reader: &SnapshotReader, change: impl FnOnce(&mut View)) {
        change(&mut reader.0.lock().unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dashboard_snapshots_share_immutable_data_and_keep_their_original_version() {
        let original = Arc::new(crate::statistics::Dashboard::default());
        let shared = Arc::new(Mutex::new(View {
            dashboard: Some(original.clone()),
            ..Default::default()
        }));
        let reader = SnapshotReader(shared.clone());
        let first = reader.snapshot();
        let second = reader.snapshot();
        assert!(Arc::ptr_eq(
            first.dashboard.as_ref().unwrap(),
            second.dashboard.as_ref().unwrap()
        ));
        let mut replacement = (*original).clone();
        replacement.day.steps = 100;
        shared.lock().unwrap().dashboard = Some(Arc::new(replacement));
        assert_eq!(reader.snapshot().dashboard.unwrap().day.steps, 100);
        assert_eq!(first.dashboard.unwrap().day.steps, 0);
    }
    #[test]
    fn queue_full_is_reported_without_blocking_or_dropping_earlier_commands() {
        let (tx, rx) = mpsc::sync_channel(1);
        let sender = CommandSender(MessageSender(tx));
        sender.try_send(UserCommand::Increment(2)).unwrap();
        assert!(
            sender
                .try_send(UserCommand::Increment(3))
                .unwrap_err()
                .contains("busy")
        );
        assert!(matches!(
            rx.recv().unwrap(),
            Input::User(UserCommand::Increment(2))
        ));
        sender.try_send(UserCommand::Increment(4)).unwrap();
        assert!(matches!(
            rx.recv().unwrap(),
            Input::User(UserCommand::Increment(4))
        ));
    }
    #[test]
    fn disconnected_worker_is_distinct_from_a_busy_worker() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        let sender = CommandSender(MessageSender(tx));
        assert!(
            sender
                .try_send(UserCommand::Exit)
                .unwrap_err()
                .contains("unavailable")
        );
        assert!(
            sender
                .send(UserCommand::Exit)
                .unwrap_err()
                .contains("unavailable")
        );
    }
    #[test]
    fn snapshots_are_independent_values_not_mutable_worker_references() {
        let raw = Arc::new(Mutex::new(View {
            status: "initial".into(),
            ..Default::default()
        }));
        let reader = SnapshotReader(raw.clone());
        let mut snapshot = reader.snapshot();
        snapshot.status = "edited locally".into();
        assert_eq!(reader.snapshot().status, "initial");
        raw.lock().unwrap().status = "worker update".into();
        assert_eq!(reader.snapshot().status, "worker update");
    }
}
