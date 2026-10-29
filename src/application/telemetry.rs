//! Missing-telemetry deadline, independent of Bluetooth.
use std::time::{Duration, Instant};
pub(super) const MISSING_TELEMETRY: &str =
    "The pad reported a start, but no treadmill data arrived. Recording is incomplete.";
#[derive(Default)]
pub(super) struct TelemetryWatch {
    pub(super) pending: Option<(u64, Instant)>,
}
impl TelemetryWatch {
    pub(super) fn started(&mut self, generation: u64, now: Instant) {
        if !self.pending.is_some_and(|(g, _)| g == generation) {
            self.pending = Some((generation, now + Duration::from_secs(8)));
        }
    }
    pub(super) fn received(&mut self, generation: u64) {
        if self.pending.is_some_and(|(g, _)| g == generation) {
            self.pending = None;
        }
    }
    pub(super) fn missing(&mut self, current: Option<u64>, now: Instant) -> bool {
        let Some((generation, deadline)) = self.pending else {
            return false;
        };
        if current != Some(generation) {
            self.pending = None;
            return false;
        }
        if now >= deadline {
            self.pending = None;
            return true;
        }
        false
    }
}
