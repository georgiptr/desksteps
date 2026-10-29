//! Discovery/reconnection policy. Deadlines are supplied by the runtime.
use super::Candidate;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
pub(super) struct ConnectionPolicy {
    pub target: Option<u64>,
    pub target_name: Option<String>,
    pub generation: u64,
    pub enabled: bool,
    pub automatic_connection: bool,
    pub candidates: BTreeMap<u64, (Candidate, Instant)>,
    pub retry: Duration,
    pub next_try: Instant,
    pub active_until: Option<Instant>,
    pub name_scan_used: bool,
    pub scan_running: bool,
    pub next_scan_try: Instant,
    pub scan_requested: bool,
    scan_matches: BTreeSet<u64>,
    scan_valid: bool,
}

impl ConnectionPolicy {
    pub fn new(target: Option<u64>, enabled: bool, now: Instant) -> Self {
        Self {
            target,
            target_name: None,
            generation: 0,
            enabled,
            automatic_connection: enabled,
            candidates: BTreeMap::new(),
            retry: Duration::from_secs(2),
            next_try: now,
            active_until: None,
            name_scan_used: false,
            scan_running: false,
            next_scan_try: now,
            scan_requested: enabled && target.is_none(),
            scan_matches: BTreeSet::new(),
            scan_valid: false,
        }
    }
    pub fn restart(&mut self, now: Instant) {
        self.next_try = now;
        self.retry = Duration::from_secs(2);
        self.name_scan_used = false;
        if self.target.is_none() {
            self.request_scan();
        }
    }
    pub fn failed(&mut self, now: Instant) {
        self.next_try = now + self.retry;
        self.retry = (self.retry * 2).min(Duration::from_secs(60));
    }
    pub fn seen(&mut self, mut candidate: Candidate, now: Instant) {
        if candidate.name.trim().is_empty()
            && let Some((prior, _)) = self.candidates.get(&candidate.address)
        {
            candidate.name.clone_from(&prior.name);
        }
        if self
            .active_until
            .is_some_and(|until| now >= until - Duration::from_secs(10) && now < until)
            && candidate.name == crate::protocol::MODEL_NAME
            && self.scan_matches.len() < 2
        {
            self.scan_matches.insert(candidate.address);
        }
        if self.candidates.len() < 8 || self.candidates.contains_key(&candidate.address) {
            self.candidates.insert(candidate.address, (candidate, now));
        }
    }
    pub fn request_scan(&mut self) {
        if self.target.is_none() {
            // Old menu entries must not crowd out this scan's candidates.
            self.candidates.clear();
        }
        self.scan_requested = true;
        self.active_until = None;
        self.scan_matches.clear();
        self.scan_valid = false;
    }
    pub fn scan_started(&mut self, now: Instant) {
        self.scan_running = true;
        if self.scan_requested {
            self.scan_requested = false;
            self.active_until = Some(now + Duration::from_secs(10));
            self.scan_matches.clear();
            self.scan_valid = true;
        }
    }
    pub fn invalidate_scan(&mut self) {
        self.scan_valid = false;
    }
    pub fn interrupt_scan(&mut self) {
        self.scan_running = false;
        if self.active_until.is_some() {
            self.request_scan();
        }
    }
    pub fn cancel_scan(&mut self) {
        self.scan_requested = false;
        self.active_until = None;
        self.scan_matches.clear();
        self.scan_valid = false;
    }
    /// Decide only after the whole scan, never on the first advertisement.
    pub fn finish_scan(&mut self, now: Instant) -> Option<u64> {
        if !self.active_until.is_some_and(|until| now >= until) {
            return None;
        }
        let selected = (self.enabled
            && self.target.is_none()
            && self.scan_valid
            && self.scan_matches.len() == 1)
            .then(|| *self.scan_matches.first().unwrap());
        self.cancel_scan();
        selected
    }
    pub fn can_attempt(&self, target: u64, now: Instant) -> bool {
        now >= self.next_try
            && self.candidates.get(&target).is_some_and(|(_, seen)| {
                now.saturating_duration_since(*seen) < Duration::from_secs(30)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(address: u64) -> Candidate {
        Candidate {
            address,
            name: "synthetic".into(),
        }
    }
    #[test]
    fn reconnect_backoff_is_bounded_and_resettable() {
        let now = Instant::now();
        let mut c = ConnectionPolicy::new(Some(1), true, now);
        for delay in [2, 4, 8, 16, 32, 60, 60] {
            c.failed(now);
            assert_eq!(c.next_try, now + Duration::from_secs(delay));
        }
        c.name_scan_used = true;
        c.restart(now);
        assert_eq!(c.retry, Duration::from_secs(2));
        assert!(!c.name_scan_used);
        assert_eq!(c.next_try, now);
    }
    #[test]
    fn candidate_freshness_and_retry_deadline_both_gate_connection() {
        let now = Instant::now();
        let mut c = ConnectionPolicy::new(Some(1), true, now);
        assert!(!c.can_attempt(1, now));
        c.seen(candidate(1), now);
        assert!(c.can_attempt(1, now + Duration::from_secs(29)));
        assert!(!c.can_attempt(1, now + Duration::from_secs(30)));
        c.failed(now);
        assert!(!c.can_attempt(1, now + Duration::from_secs(1)));
        assert!(c.can_attempt(1, now + Duration::from_secs(2)));
    }
    #[test]
    fn discovery_limit_does_not_prevent_refreshing_existing_candidates() {
        let now = Instant::now();
        let mut c = ConnectionPolicy::new(None, true, now);
        for address in 0..20 {
            c.seen(candidate(address), now);
        }
        assert_eq!(c.candidates.len(), 8);
        assert!(!c.candidates.contains_key(&8));
        c.seen(candidate(0), now + Duration::from_secs(40));
        assert!(c.can_attempt(0, now + Duration::from_secs(40)));
    }
    fn pad(address: u64) -> Candidate {
        Candidate {
            address,
            name: crate::protocol::MODEL_NAME.into(),
        }
    }
    #[test]
    fn first_use_requests_active_scan_but_saved_device_and_preview_do_not() {
        let now = Instant::now();
        assert!(ConnectionPolicy::new(None, true, now).scan_requested);
        assert!(!ConnectionPolicy::new(Some(1), true, now).scan_requested);
        assert!(!ConnectionPolicy::new(None, false, now).scan_requested);
    }
    #[test]
    fn single_pad_is_selected_only_after_full_successfully_started_scan() {
        let now = Instant::now();
        let mut c = ConnectionPolicy::new(None, true, now);
        // Failed starts and old sightings do not count toward a new scan.
        c.seen(pad(2), now);
        assert_eq!(c.finish_scan(now + Duration::from_secs(60)), None);
        let started = now + Duration::from_secs(60);
        c.scan_started(started);
        c.seen(pad(1), started);
        c.seen(pad(1), started + Duration::from_secs(9));
        assert_eq!(c.finish_scan(started + Duration::from_secs(9)), None);
        assert_eq!(c.finish_scan(started + Duration::from_secs(10)), Some(1));
        assert_eq!(c.finish_scan(started + Duration::from_secs(11)), None);
        assert!(!c.scan_requested);
    }
    #[test]
    fn no_pads_or_multiple_pads_require_manual_selection() {
        let now = Instant::now();
        for addresses in [vec![], vec![1, 2], (0..20).collect()] {
            let mut c = ConnectionPolicy::new(None, true, now);
            c.scan_started(now);
            for addr in addresses {
                c.seen(pad(addr), now);
            }
            assert_eq!(c.finish_scan(now + Duration::from_secs(10)), None);
            assert!(c.active_until.is_none());
            assert!(!c.scan_requested);
        }
    }
    #[test]
    fn only_matching_names_received_inside_this_scan_count() {
        let now = Instant::now();
        let mut c = ConnectionPolicy::new(None, true, now);
        c.seen(pad(2), now);
        c.scan_started(now);
        c.seen(pad(4), now - Duration::from_secs(1));
        c.seen(candidate(3), now);
        c.seen(pad(1), now);
        c.seen(pad(2), now + Duration::from_secs(10));
        assert_eq!(c.finish_scan(now + Duration::from_secs(10)), Some(1));
    }
    #[test]
    fn saved_or_manually_selected_pad_is_never_replaced_by_scan() {
        let now = Instant::now();
        for target in [None, Some(2)] {
            let mut c = ConnectionPolicy::new(target, true, now);
            c.request_scan();
            c.scan_started(now);
            c.seen(pad(1), now);
            c.target = Some(2);
            assert_eq!(c.finish_scan(now + Duration::from_secs(10)), None);
            assert_eq!(c.target, Some(2));
        }
    }
    #[test]
    fn interrupted_scan_starts_a_fresh_full_window_without_old_matches() {
        let now = Instant::now();
        let mut c = ConnectionPolicy::new(None, true, now);
        c.scan_started(now);
        c.seen(pad(1), now);
        c.interrupt_scan();
        assert!(c.scan_requested);
        assert!(!c.scan_running);
        assert_eq!(c.finish_scan(now + Duration::from_secs(10)), None);
        let later = now + Duration::from_secs(20);
        c.scan_started(later);
        c.seen(pad(2), later);
        assert_eq!(c.finish_scan(later + Duration::from_secs(10)), Some(2));
    }
    #[test]
    fn disabled_cancelled_or_incomplete_scans_never_select_a_default() {
        let now = Instant::now();
        for action in 0..3 {
            let mut c = ConnectionPolicy::new(None, true, now);
            c.scan_started(now);
            c.seen(pad(1), now);
            match action {
                0 => c.enabled = false,
                1 => c.cancel_scan(),
                _ => c.invalidate_scan(),
            }
            assert_eq!(c.finish_scan(now + Duration::from_secs(10)), None);
        }
    }
    #[test]
    fn retry_with_no_default_discards_old_candidates_and_scans_again() {
        let now = Instant::now();
        let mut c = ConnectionPolicy::new(None, true, now);
        for address in 0..8 {
            c.seen(pad(address), now);
        }
        c.restart(now);
        assert!(c.scan_requested);
        c.scan_started(now);
        c.seen(pad(9), now);
        assert_eq!(c.finish_scan(now + Duration::from_secs(10)), Some(9));
        assert!(c.can_attempt(9, now + Duration::from_secs(10)));
    }
}
