//! Control coordination shared by every surface. All decisions use explicit time.
use super::{exchange::Exchange, ports::PendingWrite};
use crate::control::{self, Command, Transaction};
use std::collections::VecDeque;

/// Measured motion must persist this long before the start speed is sent; an
/// upstream report links speed changes during spin-up to BLE disruption.
pub const START_SETTLE_NS: i64 = 2_000_000_000;
/// A pending start speed that cannot be sent within this window is dropped.
pub const START_WINDOW_NS: i64 = 10_000_000_000;
/// A remote start needs fresh evidence at most this old before the first
/// moving sample: a stopped sample, or a countdown followed by Machine Status
/// started. Older or missing evidence never triggers the override.
pub const REMOTE_STOPPED_NS: i64 = 10_000_000_000;

/// Owner preferences for the speed a newly started belt is brought to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StartSpeed {
    /// Target in 0.1 km/h units; `None` leaves the pad's own start speed.
    pub target: Option<u8>,
    /// Also apply after a start this app did not send (remote, pad, other app).
    pub remote: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PendingStart {
    pub generation: u64,
    pub target: u8,
    pub due: i64,
}

pub(super) struct Controls<W: PendingWrite> {
    pub exchange: Option<Exchange<W>>,
    pub pause_requested: bool,
    pub fresh_speed: Option<(u64, f64, i64)>,
    pub increment: u8,
    pub access_granted: bool,
    pub selected_speed: Option<(u64, f64, i64)>,
    pub speed_confirmation: Option<Transaction>,
    pub start_speed: StartSpeed,
    pub pending_start: Option<PendingStart>,
    /// Latest Training Status countdown frame on a connection.
    countdown: Option<(u64, i64)>,
    /// Machine Status started after a countdown with no app Start in flight.
    status_start: Option<(u64, i64)>,
    speed_queue: VecDeque<(Transaction, i64)>,
}

impl<W: PendingWrite> Controls<W> {
    pub fn new(increment: u8) -> Self {
        Self {
            exchange: None,
            pause_requested: false,
            fresh_speed: None,
            increment,
            access_granted: false,
            selected_speed: None,
            speed_confirmation: None,
            start_speed: StartSpeed::default(),
            pending_start: None,
            countdown: None,
            status_start: None,
            speed_queue: VecDeque::new(),
        }
    }
    pub fn reset(&mut self) {
        self.exchange = None;
        self.pause_requested = false;
        self.fresh_speed = None;
        self.access_granted = false;
        self.countdown = None;
        self.status_start = None;
        self.discard_speeds();
    }
    /// Training Status countdown (`03 ...`) seen on the current connection.
    pub fn countdown(&mut self, generation: u64, mono: i64) {
        self.countdown = Some((generation, mono));
    }
    /// Machine Status started (`04`). The tested pad sends no treadmill data
    /// while idle after connecting, so a fresh countdown followed by this
    /// status is the evidence of a start this app did not command.
    pub fn machine_started(&mut self, generation: u64, mono: i64) {
        let counted = self.countdown.take().is_some_and(|(g, t)| {
            g == generation
                && mono
                    .checked_sub(t)
                    .is_some_and(|age| (0..=REMOTE_STOPPED_NS).contains(&age))
        });
        self.status_start = (counted && self.exchange.is_none()).then_some((generation, mono));
    }
    pub fn discard_speeds(&mut self) {
        self.speed_queue.clear();
        self.selected_speed = None;
        self.speed_confirmation = None;
        self.pending_start = None;
    }
    pub fn speed_available(&self) -> bool {
        !self.pause_requested
            && self
                .exchange
                .as_ref()
                .is_none_or(|e| e.transaction.command().is_speed())
    }
    pub fn adjustable_speed(&self, generation: u64, now: i64) -> Option<f64> {
        let measured = control::current_speed(self.fresh_speed, generation, now)
            .filter(|s| (1.0..=6.0).contains(s))?;
        Some(
            self.selected_speed
                .filter(|(g, _, _)| *g == generation)
                .map(|(_, speed, _)| speed)
                .unwrap_or(measured),
        )
    }
    pub fn queue_speed(&mut self, transaction: Transaction, now: i64) -> Result<(), String> {
        if self.speed_queue.len() >= 32 {
            return Err("Speed queue full; click was not queued".into());
        }
        self.selected_speed = Some((transaction.generation(), transaction.target().unwrap(), now));
        self.speed_queue.push_back((transaction, now));
        Ok(())
    }
    pub fn next_speed(
        &mut self,
        current: Option<u64>,
        now: i64,
    ) -> Result<Option<Transaction>, String> {
        if self.exchange.is_some() || self.pause_requested {
            return Ok(None);
        }
        let Some((mut transaction, requested)) = self.speed_queue.pop_front() else {
            return Ok(None);
        };
        if current != Some(transaction.generation())
            || now.saturating_sub(requested) >= 3_000_000_000
            || self
                .adjustable_speed(transaction.generation(), now)
                .is_none()
        {
            self.discard_speeds();
            return Err(
                "Speed click expired or connection/telemetry changed; discarded queued speeds"
                    .into(),
            );
        }
        if self.access_granted {
            transaction.use_access(now);
        }
        Ok(Some(transaction))
    }
    /// Consume the owner's explicit Pause intent at an exchange boundary. It
    /// stays outside the callback queue, so a full queue cannot discard it.
    pub fn take_pause(&mut self, current: Option<u64>) -> Option<(Command, u64)> {
        if self.exchange.is_some() || !self.pause_requested {
            return None;
        }
        self.pause_requested = false;
        current.map(|generation| (Command::Pause, generation))
    }
    pub fn defer_pause(&mut self, command: Command, requested: u64, current: Option<u64>) -> bool {
        if command == Command::Pause && self.exchange.is_some() && current == Some(requested) {
            self.pause_requested = true;
            self.discard_speeds();
            true
        } else {
            false
        }
    }
    pub fn prepare(
        &self,
        command: Command,
        requested: u64,
        current: Option<u64>,
        enabled: bool,
        uncertain: bool,
        now: i64,
    ) -> Result<Transaction, String> {
        if !enabled {
            return Err("Controls are unavailable".into());
        }
        if matches!(command, Command::SetSpeed(_)) {
            return Err("Start speed is sent only by its setting".into());
        }
        if command == Command::StartResume && self.speed_confirmation.is_some() {
            return Err("A selected speed is still awaiting measured telemetry".into());
        }
        if self.exchange.is_some() && !(command.is_speed() && self.speed_available()) {
            return Err("A control operation is pending; input was not queued".into());
        }
        if command.is_speed() && !self.speed_available() {
            return Err("Pause is pending; speed click was not queued".into());
        }
        let generation = current.ok_or("Pad is disconnected")?;
        if generation != requested {
            return Err("Connection changed; discarded stale control request".into());
        }
        if uncertain && command != Command::Pause {
            return Err(
                "Previous outcome uncertain; reconnect before further movement commands".into(),
            );
        }
        let speed = if command.is_speed() {
            self.adjustable_speed(generation, now)
        } else {
            control::current_speed(self.fresh_speed, generation, now)
        };
        if command == Command::StartResume && speed.is_some_and(|s| s > 0.0) {
            return Err("The belt is already moving".into());
        }
        Transaction::new(command, speed, self.increment, generation, now)
    }
    /// Build the one automatic start-speed write once motion has settled.
    /// Anything stale, busy, or already at target drops it without a write.
    pub fn take_start(
        &mut self,
        current: Option<u64>,
        now: i64,
    ) -> Result<Option<Transaction>, String> {
        let Some(pending) = self.pending_start else {
            return Ok(None);
        };
        if current != Some(pending.generation) || now >= pending.due + START_WINDOW_NS {
            self.pending_start = None;
            return Err("Start speed not sent: connection changed or motion did not settle".into());
        }
        if now < pending.due
            || self.exchange.is_some()
            || self.pause_requested
            || !self.speed_queue.is_empty()
        {
            return Ok(None);
        }
        self.pending_start = None;
        if self.selected_speed.is_some() || self.speed_confirmation.is_some() {
            return Ok(None); // an explicit speed click wins
        }
        let Some(measured) = control::current_speed(self.fresh_speed, pending.generation, now)
            .filter(|s| (1.0..=6.0).contains(s))
        else {
            return Err("Start speed not sent: no fresh moving telemetry".into());
        };
        if (measured * 10.0).round() as i32 == i32::from(pending.target) {
            return Ok(None);
        }
        Transaction::new(
            Command::SetSpeed(pending.target),
            Some(measured),
            self.increment,
            pending.generation,
            now,
        )
        .map(Some)
    }
    pub fn observe(&mut self, generation: u64, speed: f64, mono: i64) -> Option<Command> {
        let previous = self.fresh_speed.replace((generation, speed, mono));
        if speed < 1.0 && self.selected_speed.is_some_and(|(g, _, _)| g == generation) {
            self.discard_speeds();
        }
        if speed.is_nan() || speed < 1.0 {
            self.pending_start = None;
        }
        let started = self.exchange.as_ref().is_some_and(|p| {
            p.transaction.command() == Command::StartResume
                && p.transaction.confirmed(generation, mono, speed)
        });
        // A stop-to-moving change with no app Start in flight was started
        // elsewhere. Reconnecting to a moving belt has no stopped sample.
        let recent = |evidence: Option<(u64, i64)>, at_least: i64| {
            evidence.is_some_and(|(g, t)| {
                g == generation
                    && mono
                        .checked_sub(t)
                        .is_some_and(|age| (at_least..=REMOTE_STOPPED_NS).contains(&age))
            })
        };
        let stopped = previous
            .filter(|(_, s, _)| *s <= 0.0)
            .map(|(g, _, t)| (g, t));
        let status = if speed >= 1.0 {
            self.status_start.take()
        } else {
            None
        };
        let external =
            self.exchange.is_none() && speed >= 1.0 && (recent(stopped, 1) || recent(status, 0));
        if let Some(target) = self.start_speed.target
            && (started || (external && self.start_speed.remote))
        {
            self.pending_start = Some(PendingStart {
                generation,
                target,
                due: mono.saturating_add(START_SETTLE_NS),
            });
        }
        if let Some(sent) = &self.speed_confirmation
            && sent.confirmed(generation, mono, speed)
            && self
                .selected_speed
                .is_some_and(|(_, target, _)| (target - speed).abs() < 0.005)
            && self.speed_queue.is_empty()
            && self.exchange.is_none()
        {
            let command = sent.command();
            self.discard_speeds();
            return Some(command);
        }
        if self
            .exchange
            .as_ref()
            .is_some_and(|p| p.transaction.confirmed(generation, mono, speed))
        {
            let command = self.exchange.as_ref().unwrap().transaction.command();
            self.exchange = None;
            Some(command)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::test_transport::Write;
    fn controls() -> Controls<Write> {
        Controls::new(1)
    }
    #[test]
    fn deferred_pause_is_consumed_once_only_after_the_exchange_boundary() {
        let mut c = controls();
        c.exchange = Some(Exchange {
            transaction: Transaction::new(Command::StartResume, None, 1, 7, 0).unwrap(),
            write: None,
            written: false,
        });
        assert!(c.defer_pause(Command::Pause, 7, Some(7)));
        assert_eq!(c.take_pause(Some(7)), None);
        assert!(c.pause_requested);
        c.exchange = None;
        assert_eq!(c.take_pause(Some(7)), Some((Command::Pause, 7)));
        assert_eq!(c.take_pause(Some(7)), None);
        c.pause_requested = true;
        assert_eq!(c.take_pause(None), None);
        assert!(!c.pause_requested);
    }
    #[test]
    fn every_request_checks_availability_connection_and_generation() {
        for command in [
            Command::StartResume,
            Command::Pause,
            Command::Increase,
            Command::Decrease,
        ] {
            let c = controls();
            assert!(
                c.prepare(command, 7, Some(7), false, false, 0)
                    .unwrap_err()
                    .contains("unavailable")
            );
            assert!(
                c.prepare(command, 7, None, true, false, 0)
                    .unwrap_err()
                    .contains("disconnected")
            );
            assert!(
                c.prepare(command, 6, Some(7), true, false, 0)
                    .unwrap_err()
                    .contains("stale")
            );
        }
    }
    #[test]
    fn uncertainty_allows_only_pause() {
        let c = controls();
        for command in [Command::StartResume, Command::Increase, Command::Decrease] {
            assert!(
                c.prepare(command, 7, Some(7), true, true, 0)
                    .unwrap_err()
                    .contains("uncertain")
            );
        }
        assert!(c.prepare(Command::Pause, 7, Some(7), true, true, 0).is_ok());
    }
    #[test]
    fn speed_adjustments_need_fresh_moving_telemetry_but_do_not_wait_for_settling() {
        let mut c = controls();
        c.observe(7, 3.0, 0);
        assert!(
            c.prepare(Command::Increase, 7, Some(7), true, false, 1)
                .is_ok()
        );
        c.observe(7, 3.0, 1_000_000_000);
        assert_eq!(
            c.prepare(Command::Increase, 7, Some(7), true, false, 1_000_000_001)
                .unwrap()
                .target(),
            Some(3.1)
        );
        assert!(
            c.prepare(Command::Decrease, 7, Some(7), true, false, 4_000_000_000)
                .is_err()
        );
        c.observe(7, 3.1, 2_000_000_000);
        assert_eq!(
            c.prepare(Command::Increase, 7, Some(7), true, false, 2_000_000_001)
                .unwrap()
                .target(),
            Some(3.2)
        );
    }
    #[test]
    fn start_rejects_known_motion_but_preserves_unknown_state_request() {
        let mut c = controls();
        assert!(
            c.prepare(Command::StartResume, 7, Some(7), true, false, 0)
                .is_ok()
        );
        c.observe(7, 2.0, 1);
        assert!(
            c.prepare(Command::StartResume, 7, Some(7), true, false, 2)
                .unwrap_err()
                .contains("already moving")
        );
    }
    #[test]
    fn pause_is_the_only_deferred_command_and_must_match_generation() {
        let mut c = controls();
        c.exchange = Some(Exchange {
            transaction: Transaction::new(Command::StartResume, None, 1, 7, 0).unwrap(),
            write: Some(Write::completed()),
            written: false,
        });
        assert!(!c.defer_pause(Command::Increase, 7, Some(7)));
        assert!(!c.defer_pause(Command::Pause, 6, Some(7)));
        assert!(c.defer_pause(Command::Pause, 7, Some(7)));
        assert!(c.pause_requested);
        assert!(
            c.prepare(Command::Increase, 7, Some(7), true, false, 1)
                .unwrap_err()
                .contains("not queued")
        );
        c.reset();
        assert!(!c.pause_requested);
        assert!(c.exchange.is_none());
        assert!(c.fresh_speed.is_none());
        assert!(c.selected_speed.is_none());
        assert!(!c.access_granted);
    }
    #[test]
    fn only_fresh_matching_motion_completes_an_accepted_exchange() {
        let mut c = controls();
        let mut transaction = Transaction::new(Command::Pause, None, 1, 7, 0).unwrap();
        transaction.reply(&[0x80, 0, 1], 1).unwrap();
        transaction.reply(&[0x80, 8, 1], 2).unwrap();
        c.exchange = Some(Exchange {
            transaction,
            write: None,
            written: true,
        });
        assert_eq!(c.observe(8, 0.0, 3), None);
        assert!(c.exchange.is_some());
        assert_eq!(c.observe(7, 1.0, 4), None);
        assert_eq!(c.observe(7, 0.0, 5), Some(Command::Pause));
        assert!(c.exchange.is_none());
    }
    fn click(c: &mut Controls<Write>, command: Command, now: i64) {
        let t = c.prepare(command, 7, Some(7), true, false, now).unwrap();
        c.queue_speed(t, now).unwrap();
    }
    #[test]
    fn clicks_accumulate_absolute_targets_in_order_before_replies_or_motion() {
        let mut c = controls();
        c.observe(7, 3.0, 0);
        for command in [Command::Increase, Command::Increase, Command::Decrease] {
            click(&mut c, command, 1);
        }
        assert_eq!(c.adjustable_speed(7, 2), Some(3.1));
        assert_eq!(c.adjustable_speed(8, 2), None);
        assert_eq!(
            c.next_speed(Some(7), 2).unwrap().unwrap().target(),
            Some(3.1)
        );
        c.access_granted = true;
        let t = c.next_speed(Some(7), 3).unwrap().unwrap();
        assert_eq!(t.bytes(), [2, 64, 1]); // 3.2, no new request-control exchange
        assert_eq!(
            c.next_speed(Some(7), 4).unwrap().unwrap().target(),
            Some(3.1)
        );
        assert!(c.next_speed(Some(7), 5).unwrap().is_none());
        let mut sent = t;
        sent.speed_written(6);
        c.speed_confirmation = Some(sent);
        assert_eq!(c.observe(7, 3.2, 7), None); // older target cannot clear the latest selection
        c.discard_speeds();
        assert_eq!(c.adjustable_speed(7, 8), Some(3.2));
        click(&mut c, Command::Increase, 9);
        let mut sent = c.next_speed(Some(7), 10).unwrap().unwrap();
        sent.speed_written(11);
        c.speed_confirmation = Some(sent);
        assert_eq!(c.observe(7, 3.3, 12), Some(Command::Increase));
        assert!(c.selected_speed.is_none());
    }
    #[test]
    fn queued_clicks_expire_on_stale_telemetry_generation_pause_and_disconnect() {
        for (generation, now) in [(Some(8), 2), (None, 2), (Some(7), 3_000_000_001)] {
            let mut c = controls();
            c.observe(7, 3.0, 0);
            click(&mut c, Command::Increase, 1);
            assert!(c.next_speed(generation, now).is_err());
            assert!(c.selected_speed.is_none());
            assert!(c.next_speed(Some(7), now).unwrap().is_none());
        }
        let mut c = controls();
        c.observe(7, 3.0, 0);
        click(&mut c, Command::Increase, 1);
        c.pause_requested = true;
        assert!(c.next_speed(Some(7), 2).unwrap().is_none());
        assert!(
            c.prepare(Command::Increase, 7, Some(7), true, false, 2)
                .is_err()
        );
        c.pause_requested = false;
        c.observe(7, 0.0, 3);
        assert!(c.next_speed(Some(7), 4).unwrap().is_none());
        assert_eq!(c.adjustable_speed(7, 4), None);
        c.observe(7, 3.0, 5);
        click(&mut c, Command::Increase, 6);
        c.reset();
        assert!(c.next_speed(Some(7), 7).unwrap().is_none());
    }
    #[test]
    fn queue_is_bounded_and_pause_clears_it_while_a_speed_write_is_inflight() {
        let mut c = controls();
        c.observe(7, 3.0, 0);
        let t = c
            .prepare(Command::Increase, 7, Some(7), true, false, 1)
            .unwrap();
        for _ in 0..32 {
            c.queue_speed(t.clone(), 1).unwrap();
        }
        assert!(c.queue_speed(t.clone(), 1).is_err());
        c.exchange = Some(Exchange {
            transaction: t,
            write: Some(Write::completed()),
            written: false,
        });
        assert!(c.speed_available());
        assert!(
            c.prepare(Command::Increase, 7, Some(7), true, false, 2)
                .is_ok()
        );
        assert!(c.next_speed(Some(7), 2).unwrap().is_none());
        assert!(c.defer_pause(Command::Pause, 7, Some(7)));
        assert!(!c.speed_available());
        assert!(c.selected_speed.is_none());
        c.exchange = None;
        assert_eq!(c.take_pause(Some(7)), Some((Command::Pause, 7)));
        assert!(c.next_speed(Some(7), 3).unwrap().is_none());
    }

    fn started(c: &mut Controls<Write>, generation: u64, at: i64) {
        let mut t = Transaction::new(Command::StartResume, None, 1, generation, at).unwrap();
        t.reply(&[0x80, 0, 1], at + 1).unwrap();
        t.reply(&[0x80, 7, 1], at + 2).unwrap();
        c.exchange = Some(Exchange {
            transaction: t,
            write: None,
            written: true,
        });
    }
    fn with_start(target: Option<u8>, remote: bool) -> Controls<Write> {
        let mut c = controls();
        c.start_speed = StartSpeed { target, remote };
        c
    }
    const S: i64 = 1_000_000_000;
    #[test]
    fn app_start_sends_one_absolute_start_speed_after_settled_motion() {
        let mut c = with_start(Some(30), false);
        c.observe(7, 0.0, 0);
        started(&mut c, 7, 0);
        assert_eq!(c.observe(7, 1.0, 3), Some(Command::StartResume));
        assert_eq!(
            c.pending_start,
            Some(PendingStart {
                generation: 7,
                target: 30,
                due: 3 + START_SETTLE_NS
            })
        );
        c.observe(7, 1.0, S);
        assert!(c.take_start(Some(7), S).unwrap().is_none()); // still settling
        assert!(c.pending_start.is_some());
        c.observe(7, 1.0, 2 * S + 3);
        let t = c.take_start(Some(7), 2 * S + 3).unwrap().unwrap();
        assert_eq!(t.command(), Command::SetSpeed(30));
        assert_eq!(t.target(), Some(3.0));
        assert!(c.pending_start.is_none());
        assert!(c.take_start(Some(7), 2 * S + 4).unwrap().is_none()); // once only
        // Without a configured target the pad's own start speed is left alone.
        let mut c = with_start(None, true);
        c.observe(7, 0.0, 0);
        started(&mut c, 7, 0);
        c.observe(7, 1.0, 3);
        assert!(c.pending_start.is_none());
        c.exchange = None;
        c.observe(7, 0.0, 4);
        c.observe(7, 1.0, 5);
        assert!(c.pending_start.is_none());
    }
    #[test]
    fn remote_start_needs_the_setting_and_a_recent_stopped_sample_in_this_connection() {
        let mut c = with_start(Some(25), false);
        c.observe(7, 0.0, 0);
        c.observe(7, 1.0, S);
        assert!(c.pending_start.is_none()); // remote override off
        let mut c = with_start(Some(25), true);
        c.observe(7, 0.0, 0);
        c.observe(7, 1.0, S);
        assert_eq!(c.pending_start.map(|p| p.target), Some(25));
        // Reconnect while walking: no stopped sample, never a speed command.
        let mut c = with_start(Some(25), true);
        c.observe(8, 2.0, 0);
        c.observe(8, 2.0, S);
        assert!(c.pending_start.is_none());
        for (previous, at) in [
            ((7, 0.0, 0), REMOTE_STOPPED_NS + 1), // stopped evidence too old
            ((6, 0.0, 0), S),                     // other connection
            ((7, 0.5, 0), S),                     // not a stopped sample
            ((7, 0.0, S), S),                     // same instant, no interval
        ] {
            let mut c = with_start(Some(25), true);
            c.fresh_speed = Some(previous);
            c.observe(7, 1.0, at);
            assert!(c.pending_start.is_none(), "{previous:?} {at}");
        }
        // Moving with an unrelated exchange in flight is not a remote start.
        let mut c = with_start(Some(25), true);
        c.observe(7, 0.0, 0);
        c.exchange = Some(Exchange {
            transaction: Transaction::new(Command::Pause, None, 1, 7, 0).unwrap(),
            write: None,
            written: false,
        });
        c.observe(7, 1.0, S);
        assert!(c.pending_start.is_none());
    }
    fn pending(c: &mut Controls<Write>) {
        c.observe(7, 0.0, 0);
        c.observe(7, 1.0, S);
        c.observe(7, 1.0, 3 * S);
        assert!(c.pending_start.is_some());
    }
    #[test]
    fn stop_pause_clicks_and_reconnect_cancel_the_start_speed_without_a_write() {
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        c.observe(7, 0.0, 3 * S + 1);
        assert!(c.pending_start.is_none());
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        c.discard_speeds(); // Pause, errors and Start all discard it
        assert!(c.pending_start.is_none());
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        c.reset();
        assert!(c.pending_start.is_none());
        // An explicit speed selection wins over the setting.
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        let click = c
            .prepare(Command::Increase, 7, Some(7), true, false, 3 * S)
            .unwrap();
        c.queue_speed(click, 3 * S).unwrap();
        assert!(c.take_start(Some(7), 3 * S + 1).unwrap().is_none());
        assert!(c.pending_start.is_some()); // waits behind the queued click
        c.next_speed(Some(7), 3 * S + 2).unwrap().unwrap();
        assert!(c.take_start(Some(7), 3 * S + 3).unwrap().is_none());
        assert!(c.pending_start.is_none());
        // Pause or another exchange holds it back without consuming it.
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        c.pause_requested = true;
        assert!(c.take_start(Some(7), 3 * S + 1).unwrap().is_none());
        assert!(c.pending_start.is_some());
    }
    #[test]
    fn stale_connection_telemetry_or_matching_speed_never_writes() {
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        assert!(c.take_start(Some(8), 3 * S + 1).is_err());
        assert!(c.pending_start.is_none());
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        assert!(c.take_start(None, 3 * S + 1).is_err());
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        assert!(
            c.take_start(Some(7), S + START_SETTLE_NS + START_WINDOW_NS)
                .is_err()
        );
        let mut c = with_start(Some(30), true);
        pending(&mut c);
        assert!(c.take_start(Some(7), 6 * S + 1).is_err()); // telemetry older than 3 s
        assert!(c.pending_start.is_none());
        let mut c = with_start(Some(10), true);
        pending(&mut c);
        assert!(c.take_start(Some(7), 3 * S + 1).unwrap().is_none()); // already at 1.0
        assert!(c.pending_start.is_none());
        assert!(
            c.prepare(Command::SetSpeed(30), 7, Some(7), true, false, 3 * S)
                .unwrap_err()
                .contains("only by its setting")
        );
    }

    #[test]
    fn countdown_then_started_status_is_a_remote_start_when_the_pad_is_silent_while_idle() {
        // Device-recorded order: no treadmill data while idle, countdown,
        // Machine Status started, then the first reading already at 1.0.
        let mut c = with_start(Some(25), true);
        c.countdown(7, 0);
        c.countdown(7, 3 * S);
        c.machine_started(7, 3 * S + 50);
        c.observe(7, 1.0, 3 * S + 200);
        assert_eq!(c.pending_start.map(|p| p.target), Some(25));
        // Used once: later readings do not re-arm it.
        c.pending_start = None;
        c.observe(7, 1.0, 4 * S);
        assert!(c.pending_start.is_none());
        // The remote option is required.
        let mut c = with_start(Some(25), false);
        c.countdown(7, 0);
        c.machine_started(7, 1);
        c.observe(7, 1.0, 2);
        assert!(c.pending_start.is_none());
        for (countdown, started, moving) in [
            (None, 0, S),         // started status without a countdown
            (Some((6, 0)), 1, S), // countdown from another connection
            (Some((7, 0)), REMOTE_STOPPED_NS + 1, REMOTE_STOPPED_NS + 2), // countdown too old
            (Some((7, 0)), 1, REMOTE_STOPPED_NS + 2), // moving reading too late
        ] {
            let mut c = with_start(Some(25), true);
            if let Some((g, t)) = countdown {
                c.countdown(g, t);
            }
            c.machine_started(7, started);
            c.observe(7, 1.0, moving);
            assert!(
                c.pending_start.is_none(),
                "{countdown:?} {started} {moving}"
            );
        }
        // A stopped reading in between keeps the evidence for the next moving one.
        let mut c = with_start(Some(25), true);
        c.countdown(7, 0);
        c.machine_started(7, 1);
        c.observe(7, 0.0, 2);
        c.observe(7, 1.0, S);
        assert!(c.pending_start.is_some());
        // An app Start in flight owns its own start; reconnect clears evidence.
        let mut c = with_start(Some(25), true);
        started(&mut c, 7, 0);
        c.countdown(7, 1);
        c.machine_started(7, 2);
        c.exchange = None;
        c.observe(7, 1.0, 3);
        assert!(c.pending_start.is_none());
        let mut c = with_start(Some(25), true);
        c.countdown(7, 0);
        c.machine_started(7, 1);
        c.reset();
        c.observe(7, 1.0, 2);
        assert!(c.pending_start.is_none());
    }
}
