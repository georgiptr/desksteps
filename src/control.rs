//! Verified FTMS commands and an offline-testable, non-retrying transaction.
pub const UUID: &str = "00002ad9-0000-1000-8000-00805f9b34fb";
pub const REPLY_NS: i64 = 5_000_000_000;
pub const SETTLE_NS: i64 = 15_000_000_000;
pub fn current_speed(
    observation: Option<(u64, f64, i64)>,
    generation: u64,
    now_ns: i64,
) -> Option<f64> {
    observation
        .filter(|(g, s, t)| {
            *g == generation
                && s.is_finite()
                && now_ns
                    .checked_sub(*t)
                    .is_some_and(|age| (0..3_000_000_000).contains(&age))
        })
        .map(|(_, s, _)| s)
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    StartResume,
    Pause,
    Increase,
    Decrease,
}
impl Command {
    pub fn is_speed(self) -> bool {
        matches!(self, Self::Increase | Self::Decrease)
    }
    pub fn frame(self, speed: Option<f64>, increment: u8) -> Result<Vec<u8>, String> {
        match self {
            Self::StartResume => Ok(vec![7]),
            Self::Pause => Ok(vec![8, 2]),
            Self::Increase | Self::Decrease => {
                if !(1..=10).contains(&increment) {
                    return Err("Speed increment must be 0.1–1.0 km/h".into());
                }
                let speed = speed
                    .filter(|s| s.is_finite() && (1.0..=6.0).contains(s))
                    .ok_or("Speed change requires fresh moving telemetry within 1–6 km/h")?;
                let units = (speed * 10.0).round() as i32;
                let target = (units
                    + if self == Self::Increase {
                        i32::from(increment)
                    } else {
                        -i32::from(increment)
                    })
                .clamp(10, 60);
                if target == units {
                    return Err("Already at the speed boundary".into());
                }
                let bytes = (target as u16 * 10).to_le_bytes();
                Ok(vec![2, bytes[0], bytes[1]])
            }
        }
    }
}
pub fn response(bytes: &[u8], opcode: u8) -> Result<bool, String> {
    if bytes.len() < 3 || bytes[0] != 0x80 || bytes[1] != opcode {
        return Ok(false);
    }
    if bytes[2] != 1 {
        return Err(format!(
            "FTMS opcode {opcode:02x} rejected with result {:02x}",
            bytes[2]
        ));
    }
    Ok(true)
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    Access,
    Action,
    Settling,
}
#[derive(Clone, Debug)]
pub struct Transaction {
    generation: u64,
    command: Command,
    frame: Vec<u8>,
    stage: Stage,
    deadline: i64,
    after: i64,
}
impl Transaction {
    /// Reuse control access only within the current connection. Speed writes
    /// finish at GATT completion; indications remain diagnostic evidence.
    pub fn use_access(&mut self, now: i64) {
        if self.command.is_speed() {
            self.stage = Stage::Action;
            self.deadline = now.saturating_add(REPLY_NS);
        }
    }
    pub fn speed_written(&mut self, now: i64) {
        if self.command.is_speed() && self.stage == Stage::Action {
            self.stage = Stage::Settling;
            self.after = now;
            self.deadline = now.saturating_add(SETTLE_NS);
        }
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn command(&self) -> Command {
        self.command
    }
    pub fn stage(&self) -> Stage {
        self.stage
    }
    pub fn new(
        command: Command,
        speed: Option<f64>,
        increment: u8,
        generation: u64,
        now: i64,
    ) -> Result<Self, String> {
        Ok(Self {
            generation,
            command,
            frame: command.frame(speed, increment)?,
            stage: Stage::Access,
            deadline: now.saturating_add(REPLY_NS),
            after: now,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        if self.stage == Stage::Access {
            &[0]
        } else {
            &self.frame
        }
    }
    pub fn target(&self) -> Option<f64> {
        match self.command {
            Command::Pause => Some(0.0),
            Command::StartResume => None,
            _ => Some(u16::from_le_bytes([self.frame[1], self.frame[2]]) as f64 / 100.0),
        }
    }
    pub fn reply(&mut self, bytes: &[u8], now: i64) -> Result<bool, String> {
        if self.stage == Stage::Settling || !response(bytes, self.bytes()[0])? {
            return Ok(false);
        }
        if self.stage == Stage::Action && bytes.len() > 3 && bytes[3..] != self.frame[1..] {
            return Err("Control parameter echo differs; outcome uncertain".into());
        }
        self.stage = if self.stage == Stage::Access {
            Stage::Action
        } else {
            Stage::Settling
        };
        self.after = now;
        self.deadline = now.saturating_add(if self.stage == Stage::Settling {
            SETTLE_NS
        } else {
            REPLY_NS
        });
        Ok(true)
    }
    pub fn confirmed(&self, generation: u64, mono: i64, speed: f64) -> bool {
        self.stage == Stage::Settling
            && generation == self.generation
            && mono > self.after
            && mono <= self.deadline
            && speed.is_finite()
            && match self.target() {
                Some(target) => (target - speed).abs() < 0.005,
                None => speed > 0.0,
            }
    }
    pub fn expired(&self, now: i64) -> bool {
        now >= self.deadline
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freshness_and_generation() {
        assert_eq!(current_speed(None, 2, 4_000_000_000), None);
        for obs in [
            (1, 1.0, 3_000_000_000),
            (2, 1.0, 1_000_000_000),
            (2, 1.0, 4_000_000_001),
            (2, f64::NAN, 3_000_000_000),
            (2, 1.0, i64::MIN),
        ] {
            assert_eq!(current_speed(Some(obs), 2, 4_000_000_000), None);
        }
        assert_eq!(
            current_speed(Some((2, 3.0, 3_000_000_000)), 2, 4_000_000_000),
            Some(3.0)
        );
    }
    #[test]
    fn all_increments_and_boundaries_encode_independent_integer_targets() {
        for current in 10..=60 {
            for delta in 1..=10 {
                for command in [Command::Increase, Command::Decrease] {
                    let expected = (current
                        + if command == Command::Increase {
                            i32::from(delta)
                        } else {
                            -i32::from(delta)
                        })
                    .clamp(10, 60);
                    let result = command.frame(Some(current as f64 / 10.0), delta);
                    if expected == current {
                        assert!(result.is_err());
                    } else {
                        let frame = result.unwrap();
                        assert_eq!(frame[0], 2);
                        assert_eq!(
                            u16::from_le_bytes([frame[1], frame[2]]),
                            expected as u16 * 10
                        );
                    }
                }
            }
        }
        assert_eq!(Command::Increase.frame(Some(3.0), 1).unwrap(), [2, 54, 1]);
        for speed in [
            None,
            Some(0.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(6.1),
        ] {
            assert!(Command::Increase.frame(speed, 1).is_err());
        }
        for delta in [0, 11, 255] {
            assert!(Command::Decrease.frame(Some(3.0), delta).is_err());
        }
        assert_eq!(Command::StartResume.frame(None, 1).unwrap(), [7]);
        assert_eq!(Command::Pause.frame(None, 1).unwrap(), [8, 2]);
    }
    #[test]
    fn transaction_requires_access_reply_and_new_matching_telemetry() {
        let mut t = Transaction::new(Command::Increase, Some(3.0), 1, 7, 0).unwrap();
        assert_eq!(t.bytes(), [0]);
        assert!(!t.reply(&[0x80, 2, 1], 1).unwrap());
        assert!(!t.reply(&[0, 0, 1], 1).unwrap());
        assert!(!t.reply(&[0x80, 0], 1).unwrap());
        assert!(t.reply(&[0x80, 0, 1], 2).unwrap());
        assert_eq!(t.bytes(), [2, 54, 1]);
        assert!(!t.confirmed(7, 3, 3.1));
        assert!(t.reply(&[0x80, 2, 1, 54, 1], 4).unwrap());
        assert!(!t.reply(&[0x80, 2, 1], 5).unwrap());
        assert!(!t.confirmed(6, 5, 3.1));
        assert!(!t.confirmed(7, 4, 3.1));
        assert!(!t.confirmed(7, 5, 3.0));
        assert!(!t.confirmed(7, 5, f64::NAN));
        assert!(t.confirmed(7, 5, 3.1));
        assert!(!t.expired(t.deadline - 1));
        assert!(t.expired(t.deadline));
        assert!(!t.confirmed(7, t.deadline + 1, 3.1));
    }
    #[test]
    fn pause_start_errors_and_echoes() {
        for command in [Command::Pause, Command::StartResume] {
            let mut t = Transaction::new(command, None, 1, 1, 0).unwrap();
            assert!(t.reply(&[0x80, 0, 5], 1).is_err());
            t.reply(&[0x80, 0, 1], 2).unwrap();
            let op = t.frame[0];
            t.reply(&[0x80, op, 1], 3).unwrap();
            assert_eq!(t.confirmed(1, 4, 0.0), command == Command::Pause);
            assert_eq!(t.confirmed(1, 4, 1.0), command == Command::StartResume);
        }
        let mut t = Transaction::new(Command::Decrease, Some(3.0), 1, 1, 0).unwrap();
        t.reply(&[0x80, 0, 1], 1).unwrap();
        assert!(t.reply(&[0x80, 2, 1, 0, 0], 2).is_err());
        assert!(Transaction::new(Command::Increase, None, 1, 1, 0).is_err());
    }
}
