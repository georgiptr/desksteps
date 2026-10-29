//! Reconcile a confirmed pause across a recorder/link interruption.
use crate::protocol::Sample;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Counters {
    steps: Option<u32>,
    distance: Option<u32>,
    elapsed: Option<u32>,
}
impl Counters {
    fn from_sample(s: &Sample) -> Self {
        Self {
            steps: s.steps,
            distance: s.distance_m,
            elapsed: s.elapsed_s,
        }
    }
    fn unchanged_activity(self, s: &Sample) -> bool {
        self.steps.zip(s.steps).is_some_and(|(a, b)| a == b)
            && self.distance.zip(s.distance_m).is_some_and(|(a, b)| a == b)
            // Startup after reconnect can advance the workout clock before
            // any steps. A decreased clock still makes the baseline uncertain.
            && self.elapsed.zip(s.elapsed_s).is_some_and(|(a, b)| b >= a)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Restart {
    latest: Option<Counters>,
    paused: Option<Counters>,
    reconciled: bool,
}
impl Restart {
    pub(super) fn interrupt(&mut self) {
        if self.reconciled {
            // A second interruption needs its own counter reconciliation.
            self.paused = self.latest;
            self.reconciled = false;
        }
    }
    pub(super) fn end(&mut self, paused: bool) {
        self.paused = if paused { self.latest } else { None };
        self.reconciled = false;
    }
    pub(super) fn observe(&mut self, s: &Sample, quality: &str) {
        if let Some(baseline) = self.paused {
            let ordered = matches!(quality, "unchanged" | "observed");
            if !ordered || !self.reconciled && !baseline.unchanged_activity(s) {
                // Once uncertain, later unchanged samples cannot erase it.
                self.paused = None;
                self.reconciled = false;
            } else {
                self.reconciled = true;
            }
        }
        self.latest = Some(Counters::from_sample(s));
    }
    pub(super) fn resume(&mut self) -> bool {
        self.paused = None;
        std::mem::take(&mut self.reconciled)
    }
}
