//! Runtime publication operations. State and serialization belong to Runtime.
use super::*;
impl Runtime<'_> {
    pub(super) fn refresh_dashboard(&mut self) -> Result<(), String> {
        {
            let mut v = self.view.lock().unwrap();
            if !v.dashboard_visible {
                self.dashboard_request = None;
                v.dashboard = None;
            }
        }
        if let Some(request) = &self.dashboard_request
            && Instant::now() >= self.next_dashboard
        {
            let cached = self.view.lock().unwrap().dashboard.clone().filter(|d| {
                d.day.date == request.day
                    && d.days.len() == request.range.clamp(1, crate::statistics::MAX_RANGE) as usize
            });
            let short = crate::statistics::Request {
                day: request.day.clone(),
                range: 1,
            };
            match self.store.dashboard(
                if cached.is_some() { &short } else { request },
                self.utc_now(),
            ) {
                Ok(mut data) => {
                    if let Some(cached) = cached {
                        data.days = cached.days.clone();
                        *data.days.last_mut().unwrap() = data.day.clone();
                    }
                    self.view.lock().unwrap().dashboard = Some(Arc::new(data));
                }
                Err(error) => {
                    self.view.lock().unwrap().ui_message =
                        Some(format!("Statistics unavailable: {error}"))
                }
            }
            self.next_dashboard = Instant::now() + Duration::from_secs(1);
        }
        Ok(())
    }

    pub(super) fn publish(&mut self) -> Result<(), String> {
        {
            let mut v = self.view.lock().unwrap();
            if let Some(pad) = &self.simulator {
                let s = pad.lock().unwrap();
                v.simulator_status = format!(
                    "{} | {:.1} km/h | {} steps | telemetry {} | next fault {:?} | virtual {} | shortcuts {}",
                    v.simulator_status
                        .split(" | ")
                        .next()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Ready"),
                    s.speed,
                    s.steps,
                    if s.telemetry { "on" } else { "off" },
                    s.fault,
                    (Utc::now() + s.offset)
                        .with_timezone(&self.options.timezone)
                        .format("%Y-%m-%d %H:%M:%S %Z"),
                    if v.development_shortcuts { "on" } else { "off" }
                );
            }
            v.status = if self.options.development {
                format!("Synthetic: {}", self.status)
            } else {
                self.status.clone()
            };
            v.selected = self.connection.target;
            v.selected_name.clone_from(&self.connection.target_name);
            v.automatic_connection = self.connection.automatic_connection;
            v.connected = self.link.is_some();
            v.generation = self.connection.generation;
            v.increment = self.controls.increment;
            v.fresh_speed = self.link.as_ref().and_then(|l| {
                control::current_speed(self.controls.fresh_speed, l.generation, self.mono_now())
            });
            v.control_busy = self.controls.exchange.is_some()
                || self.controls.pause_requested
                || self.controls.speed_confirmation.is_some();
            v.speed_available = self.controls.speed_available();
            v.selected_speed = self.controls.selected_speed.map(|(_, speed, _)| speed);
            v.adjustable_speed = self.link.as_ref().and_then(|l| {
                self.controls
                    .adjustable_speed(l.generation, self.mono_now())
            });
            v.control_uncertain = self.link.as_ref().is_some_and(|l| l.control_uncertain);
            v.candidates = self
                .connection
                .candidates
                .values()
                .map(|(c, _)| c.clone())
                .collect();
            v.totals = self.store.totals(self.utc_now())?;
        }
        (self.notify)();
        Ok(())
    }

    pub(super) fn delay(&self) -> Duration {
        let mut delay = if let Some(simulator) = &self.simulator
            && self.link.is_some()
        {
            // User commands wake recv_timeout immediately. Only simulator time
            // needs a periodic tick; pending controls retain their 50 ms bound.
            Duration::from_secs(1).saturating_sub(simulator.lock().unwrap().last_tick.elapsed())
        } else if self.options.development
            || self.connection.active_until.is_some()
            || (self.connection.enabled
                && self.link.is_none()
                && self.connection.next_try > Instant::now())
        {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(60)
        };
        if let Some((_, deadline)) = self.telemetry_watch.pending {
            delay = delay.min(deadline.saturating_duration_since(Instant::now()));
        }
        if (self.connection.enabled || self.connection.scan_requested)
            && self.link.is_none()
            && !self.connection.scan_running
        {
            delay = delay.min(Duration::from_secs(2));
        }
        if self.dashboard_request.is_some() {
            delay = delay.min(
                self.next_dashboard
                    .saturating_duration_since(Instant::now()),
            );
        }
        if self.controls.exchange.is_some() || self.controls.speed_confirmation.is_some() {
            delay = delay.min(Duration::from_millis(50));
        }
        if let Some((_, _, mono)) = self.controls.fresh_speed {
            let remaining = mono.saturating_add(3_000_000_000) - (self.mono_now());
            if remaining > 0 {
                delay = delay.min(Duration::from_nanos(remaining as u64));
            }
        }
        delay
    }
}
