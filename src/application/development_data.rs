//! Explicitly synthetic history. Never imports or reads a personal database.
use super::*;
pub(super) fn initialize(store: &mut Store, now: DateTime<Utc>) -> Result<(), String> {
    if store.setting("development_data")?.as_deref() != Some("1") {
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if count != 0 {
            return Err(
                "Development storage contains unmarked records; refusing to reuse it".into(),
            );
        }
        store.set_setting("development_data", "1")?;
    }
    if store.setting("development_seed_complete")?.as_deref() == Some("1") {
        return Ok(());
    }
    // Persist the origin first; interrupted seeding resumes idempotently.
    let origin = match store.setting("development_seed_origin")? {
        Some(stamp) => DateTime::parse_from_rfc3339(&stamp)
            .map_err(|e| e.to_string())?
            .with_timezone(&Utc),
        None => {
            store.set_setting("development_seed_origin", &now.to_rfc3339())?;
            now
        }
    };
    store.save_preferences(&crate::statistics::Preferences {
        effective: (origin.with_timezone(&store.timezone).date_naive() - chrono::Duration::days(7))
            .to_string(),
        weight: Some(75.0),
        steps: Some(1000),
        minutes: Some(30.0),
        calories: Some(120.0),
    })?;
    for day in (0..7).rev() {
        let start = origin - chrono::Duration::days(day) - chrono::Duration::minutes(10);
        for second in 0..=180 {
            let speed = if second < 180 {
                2.0 + (day % 3) as f64 * 0.5
            } else {
                0.0
            };
            let bytes = simulator::packet(speed, second as u32 * 2, second as u32);
            let sample = protocol::decode(&bytes)?;
            store.record(
                Event {
                    key: &format!("development-seed-{day}-{second}"),
                    device: "synthetic-history",
                    connection: &format!("development-seed-{day}:1"),
                    utc: start + chrono::Duration::seconds(second),
                    monotonic_ns: Some(second * 1_000_000_000),
                    kind: "notification",
                    uuid: protocol::TREADMILL,
                    raw: &bytes,
                    detail: "Synthetic development sample; not personal walking data",
                },
                Some(&sample),
            )?;
        }
        // Confirm rest before the next day's connection clears pause evidence.
        store.expire_sessions(start + chrono::Duration::seconds(481))?;
    }
    store.expire_sessions(origin)?;
    store.set_setting("development_seed_complete", "1")
}
pub(super) fn reset(store: &mut Store, now: DateTime<Utc>) -> Result<(), String> {
    if store.setting("development_data")?.as_deref() != Some("1") {
        return Err("Reset is allowed only for marked synthetic development storage".into());
    }
    // The owner confirms this action in the development panel. No automatic
    // retention/deletion policy is introduced for any diagnostic or real data.
    let tx = store.conn.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(
        "DELETE FROM pad_calories; DELETE FROM recovery_credits; DELETE FROM recovery_baselines;
        DELETE FROM ui_intervals; DELETE FROM ui_processed; DELETE FROM samples;
        DELETE FROM events; DELETE FROM anchors; DELETE FROM ui_anchors; DELETE FROM ui_sessions;
        DELETE FROM ui_runs; DELETE FROM ui_preferences;
        DELETE FROM settings WHERE key != 'timezone' AND key != 'development_data';",
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    initialize(store, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_is_useful_idempotent_and_reset_is_restricted_to_synthetic_storage() {
        let now = DateTime::parse_from_rfc3339("2026-10-23T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut store = Store::initialize(
            rusqlite::Connection::open_in_memory().unwrap(),
            chrono_tz::UTC,
        )
        .unwrap();
        assert!(reset(&mut store, now).is_err());
        initialize(&mut store, now).unwrap();
        let count = |s: &Store| {
            s.conn
                .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        let initial = count(&store);
        assert!(initial > 1000);
        let sessions: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM ui_sessions WHERE state='Finished'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sessions, 7);
        assert!(store.totals(now).unwrap().recorded_steps > 0);
        initialize(&mut store, now + chrono::Duration::days(1)).unwrap();
        assert_eq!(count(&store), initial);
        reset(&mut store, now).unwrap();
        assert_eq!(count(&store), initial);
        store
            .conn
            .execute("DELETE FROM settings WHERE key='development_data'", [])
            .unwrap();
        assert!(initialize(&mut store, now).is_err());
    }
}
