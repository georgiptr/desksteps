//! Schema initialization. Original recorder tables stay compatible; projections are additive.
use super::*;
pub(crate) fn initialize_statistics(conn: &Connection) -> rusqlite::Result<()> {
    // Keep recorder schema version 1: these independent additive tables are
    // ignored by older recorders. Missing derived rows remain explicit legacy.
    conn.execute_batch("CREATE TABLE IF NOT EXISTS ui_preferences(day TEXT PRIMARY KEY, weight REAL, steps INTEGER, minutes REAL, calories REAL);
        CREATE TABLE IF NOT EXISTS ui_sessions(id INTEGER PRIMARY KEY, device TEXT NOT NULL, start TEXT NOT NULL, end TEXT NOT NULL, state TEXT NOT NULL, gap INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS ui_anchors(device TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS ui_runs(process TEXT PRIMARY KEY, excluded INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS ui_processed(event_id INTEGER PRIMARY KEY REFERENCES events(id), excluded INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS ui_intervals(event_id INTEGER PRIMARY KEY REFERENCES events(id), session INTEGER NOT NULL REFERENCES ui_sessions(id), day TEXT NOT NULL, steps INTEGER NOT NULL, active_ms INTEGER NOT NULL, factor REAL NOT NULL, model TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS ui_intervals_day ON ui_intervals(day,session);
        CREATE TABLE IF NOT EXISTS recovery_baselines(device TEXT PRIMARY KEY, sample TEXT NOT NULL, utc TEXT NOT NULL, connection TEXT NOT NULL, mono INTEGER, excluded INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS recovery_credits(event_id INTEGER PRIMARY KEY REFERENCES events(id), steps INTEGER NOT NULL, active_ms INTEGER NOT NULL, factor REAL);
        CREATE INDEX IF NOT EXISTS events_device_id ON events(device,id);
        CREATE TABLE IF NOT EXISTS pad_calories(event_id INTEGER PRIMARY KEY REFERENCES samples(event_id) ON DELETE CASCADE, kcal INTEGER, recovered INTEGER NOT NULL, incomplete INTEGER NOT NULL, baseline INTEGER NOT NULL);")
}
impl Store {
    pub(crate) fn initialize(conn: Connection, timezone: Tz) -> Result<Self, String> {
        conn.busy_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 1 {
            return Err("This database requires a newer DeskSteps version".into());
        }
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events(
                id INTEGER PRIMARY KEY, source_key TEXT NOT NULL UNIQUE, device TEXT NOT NULL,
                connection_id TEXT NOT NULL, utc TEXT NOT NULL, monotonic_ns INTEGER,
                kind TEXT NOT NULL, uuid TEXT NOT NULL, raw BLOB NOT NULL, detail TEXT NOT NULL,
                timezone TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS samples(
                event_id INTEGER PRIMARY KEY REFERENCES events(id), decoded TEXT NOT NULL,
                step_delta INTEGER NOT NULL, quality TEXT NOT NULL, local_day TEXT);
            CREATE INDEX IF NOT EXISTS samples_day ON samples(local_day, quality);
            CREATE TABLE IF NOT EXISTS anchors(
                device TEXT PRIMARY KEY, connection_id TEXT NOT NULL, utc TEXT NOT NULL,
                monotonic_ns INTEGER, steps INTEGER NOT NULL, elapsed_s INTEGER, local_day TEXT NOT NULL);
            PRAGMA user_version=1;").map_err(|e| e.to_string())?;
        let timezone_name = timezone.name();
        let previous: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key='timezone'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        if previous.as_deref().is_some_and(|s| s != timezone_name) {
            return Err(
                "Database timezone differs; use the original timezone or a separate database"
                    .into(),
            );
        }
        conn.execute(
            "INSERT OR IGNORE INTO settings VALUES('timezone',?1)",
            [timezone_name],
        )
        .map_err(|e| e.to_string())?;
        initialize_statistics(&conn).map_err(|e| e.to_string())?;
        let mut store = Self {
            conn,
            timezone,
            ownership: None,
        };
        store.rebuild_sessions()?;
        store.backfill_pad_calories()?;
        Ok(store)
    }
}
