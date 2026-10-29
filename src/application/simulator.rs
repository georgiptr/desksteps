//! Synthetic pad transport. Never creates a Windows Bluetooth object.
//! Replies, raw FTMS packets and virtual time feed the production coordinator.
use super::*;
use std::collections::VecDeque;

pub(crate) const ADDRESS: u64 = 0x020000000001;
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SimAction {
    Connect,
    Disconnect,
    Scan,
    ToggleTelemetry,
    RejectNext,
    DelayNext,
    TimeoutNext,
    FailWrite,
    NoMotionNext,
    Malformed,
    ResetCounter,
    PauseFiveMinutes,
    DayBoundary,
    ToggleShortcuts,
    ResetData,
}
pub(crate) const ACTIONS: &[(SimAction, &str)] = &[
    (SimAction::Connect, "Connect / reconnect"),
    (SimAction::Disconnect, "Disconnect"),
    (SimAction::Scan, "Discover synthetic pad"),
    (SimAction::ToggleTelemetry, "Toggle telemetry"),
    (SimAction::RejectNext, "Reject next command"),
    (SimAction::DelayNext, "Delay next response (2 s)"),
    (SimAction::TimeoutNext, "Drop next response"),
    (SimAction::FailWrite, "Fail next write"),
    (SimAction::NoMotionNext, "Accept without motion"),
    (SimAction::Malformed, "Malformed packet"),
    (SimAction::ResetCounter, "Reset step counter"),
    (SimAction::PauseFiveMinutes, "Five-minute pause"),
    (SimAction::DayBoundary, "Cross local midnight"),
    (SimAction::ToggleShortcuts, "Toggle development shortcuts"),
    (SimAction::ResetData, "Reset development data…"),
];
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Fault {
    #[default]
    None,
    Reject,
    Delay,
    Timeout,
    Write,
    NoMotion,
}
pub(super) struct Simulator {
    pub speed: f64,
    pub steps: u32,
    pub elapsed: u32,
    pub telemetry: bool,
    pub fault: Fault,
    pub offset: chrono::Duration,
    pub last_tick: Instant,
    fraction: f64,
    active_fault: Fault,
    replies: VecDeque<(Instant, Vec<u8>)>,
    pub raw: VecDeque<(String, Vec<u8>)>,
}
impl Simulator {
    pub fn new(saved: Option<DateTime<Utc>>) -> Self {
        Self {
            speed: 0.0,
            steps: 0,
            elapsed: 0,
            telemetry: true,
            fault: Fault::None,
            offset: saved
                .map(|s| (s - Utc::now()).max(chrono::Duration::zero()))
                .unwrap_or_default(),
            last_tick: Instant::now(),
            fraction: 0.0,
            active_fault: Fault::None,
            replies: VecDeque::new(),
            raw: VecDeque::new(),
        }
    }
    pub fn reconnect(&mut self) {
        // A reconnect observes the existing simulated belt; it never starts it.
        self.replies.clear();
        self.raw.clear();
        self.active_fault = Fault::None;
    }
    pub fn write(&mut self, frame: &[u8], now: Instant) -> Result<(), String> {
        let opcode = *frame.first().ok_or("Empty synthetic command")?;
        if opcode == 0 {
            self.active_fault = std::mem::take(&mut self.fault);
        }
        let fault = if opcode == 0 {
            self.active_fault
        } else if self.fault != Fault::None {
            std::mem::take(&mut self.fault)
        } else {
            std::mem::take(&mut self.active_fault)
        };
        if fault == Fault::Write {
            return Err("Synthetic GATT write failed; outcome unknown".into());
        }
        if fault == Fault::Timeout {
            return Ok(());
        }
        let accepted = fault != Fault::Reject;
        if accepted && opcode != 0 && fault != Fault::NoMotion {
            match frame {
                [7] => {
                    if self.speed == 0.0 {
                        self.speed = 1.0;
                    }
                    self.raw.push_back((MACHINE_STATUS.into(), vec![4]));
                }
                [8, 2] => {
                    self.speed = 0.0;
                    self.raw.push_back((MACHINE_STATUS.into(), vec![2, 2]));
                }
                [2, lo, hi] => self.speed = u16::from_le_bytes([*lo, *hi]) as f64 / 100.0,
                _ => return Err("Unsupported synthetic FTMS command".into()),
            }
        }
        let delay = if fault == Fault::Delay {
            Duration::from_secs(2)
        } else {
            Duration::from_millis(80)
        };
        self.replies.push_back((
            now + delay,
            vec![0x80, opcode, if accepted { 1 } else { 4 }],
        ));
        Ok(())
    }
    pub fn reply(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.replies.front().is_some_and(|(due, _)| now >= *due) {
            self.replies.pop_front().map(|(_, r)| {
                self.raw.push_back((control::UUID.into(), r.clone()));
                r
            })
        } else {
            None
        }
    }
    pub fn packet(&mut self, seconds: u32) -> Vec<u8> {
        if self.speed > 0.0 {
            self.elapsed = self.elapsed.saturating_add(seconds);
            // Chosen synthetic cadence, not a physiological estimate.
            self.fraction += self.speed * 0.6 * seconds as f64;
            let increment = self.fraction.floor() as u32;
            self.fraction -= increment as f64;
            self.steps = self.steps.wrapping_add(increment) & 0x00ff_ffff;
        }
        packet(self.speed, self.steps, self.elapsed)
    }
}
pub(super) fn packet(speed: f64, steps: u32, elapsed: u32) -> Vec<u8> {
    let mut bytes = vec![0x80, 0x24]; // speed, energy, elapsed seconds and 24-bit steps
    bytes.extend(((speed * 100.0).round() as u16).to_le_bytes());
    // Arbitrary synthetic pad calories, deliberately independent of user weight.
    bytes.extend(((steps / 20).min(u16::MAX as u32) as u16).to_le_bytes());
    bytes.extend(100u16.to_le_bytes());
    bytes.push(0);
    bytes.extend((elapsed.min(u16::MAX as u32) as u16).to_le_bytes());
    bytes.extend(&steps.to_le_bytes()[..3]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simulated_pad_reports_weight_independent_cumulative_calories() {
        for (steps, kcal) in [(0, 0), (19, 0), (20, 1), (400, 20)] {
            let sample = protocol::decode(&packet(3.0, steps, 100)).unwrap();
            assert_eq!(
                (
                    sample.steps,
                    sample.elapsed_s,
                    sample.calories_kcal,
                    sample.calories_per_hour
                ),
                (Some(steps), Some(100), Some(kcal), Some(100))
            );
        }
    }
    #[test]
    fn transport_models_acceptance_rejection_missing_replies_and_write_failure() {
        let mut pad = Simulator::new(None);
        let now = Instant::now();
        pad.write(&[0], now).unwrap();
        assert_eq!(pad.reply(now), None);
        assert_eq!(
            pad.reply(now + Duration::from_secs(1)),
            Some(vec![0x80, 0, 1])
        );
        pad.write(&[7], now).unwrap();
        assert_eq!(
            protocol::decode(&pad.packet(2)).unwrap().speed_kmh,
            Some(1.0)
        );
        assert!(pad.steps > 0);
        pad.fault = Fault::Reject;
        pad.write(&[0], now).unwrap();
        pad.reply(now + Duration::from_secs(1)); // prior action reply
        assert_eq!(
            pad.reply(now + Duration::from_secs(1)),
            Some(vec![0x80, 0, 4])
        );
        pad.fault = Fault::Timeout;
        pad.write(&[0], now).unwrap();
        assert_eq!(pad.reply(now + Duration::from_secs(20)), None);
        pad.fault = Fault::Write;
        assert!(pad.write(&[0], now).is_err());
        pad.reconnect();
        assert_eq!(pad.speed, 1.0);
        pad.fault = Fault::NoMotion;
        pad.write(&[0], now).unwrap();
        pad.write(&[8, 2], now).unwrap();
        assert_eq!(pad.speed, 1.0);
        pad.fault = Fault::Delay;
        pad.write(&[0], now).unwrap();
        pad.reconnect();
        pad.write(&[8, 2], now).unwrap();
        assert_eq!(pad.speed, 0.0);
    }
}
