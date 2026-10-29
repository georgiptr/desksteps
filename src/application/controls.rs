//! Control coordination shared by every surface. All decisions use explicit time.
use super::{exchange::Exchange, ports::PendingWrite};
use crate::control::{self, Command, Transaction};
use std::collections::VecDeque;

pub(super) struct Controls<W: PendingWrite> {
    pub exchange: Option<Exchange<W>>,
    pub pause_requested: bool,
    pub fresh_speed: Option<(u64, f64, i64)>,
    pub increment: u8,
    pub access_granted: bool,
    pub selected_speed: Option<(u64, f64, i64)>,
    pub speed_confirmation: Option<Transaction>,
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
            speed_queue: VecDeque::new(),
        }
    }
    pub fn reset(&mut self) {
        self.exchange = None;
        self.pause_requested = false;
        self.fresh_speed = None;
        self.access_granted = false;
        self.discard_speeds();
    }
    pub fn discard_speeds(&mut self) {
        self.speed_queue.clear();
        self.selected_speed = None;
        self.speed_confirmation = None;
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
    pub fn observe(&mut self, generation: u64, speed: f64, mono: i64) -> Option<Command> {
        self.fresh_speed = Some((generation, speed, mono));
        if speed < 1.0 && self.selected_speed.is_some_and(|(g, _, _)| g == generation) {
            self.discard_speeds();
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
}
