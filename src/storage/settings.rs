//! Persisted application and dated exercise preferences.
use super::*;
use crate::statistics::{Preferences, date};
impl Store {
    pub fn device_name(&self, device: u64) -> Result<Option<String>, String> {
        self.setting(&format!("device_name:{}", crate::address(device)))
    }
    /// Save the default and its known name together, keyed by address so that
    /// selecting a different device can never reuse another device's name.
    pub fn save_device(&mut self, device: u64, name: Option<&str>) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO settings VALUES('device',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [crate::address(device)]).map_err(|e| e.to_string())?;
        if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
            tx.execute("INSERT INTO settings VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![format!("device_name:{}", crate::address(device)), name]).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn.execute("INSERT INTO settings VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn preferences(&self, day: &str) -> Result<Preferences, String> {
        date(day)?;
        self.conn.query_row("SELECT day,weight,steps,minutes,calories FROM ui_preferences WHERE day<=?1 ORDER BY day DESC LIMIT 1", [day], |r| Ok(Preferences {effective:r.get(0)?,weight:r.get(1)?,steps:r.get(2)?,minutes:r.get(3)?,calories:r.get(4)?})).optional().map(|p|p.unwrap_or(Preferences{effective:day.into(),..Default::default()})).map_err(|e|e.to_string())
    }
    pub fn save_preferences(&self, p: &Preferences) -> Result<(), String> {
        p.validate()?;
        self.conn.execute("INSERT INTO ui_preferences VALUES(?1,?2,?3,?4,?5) ON CONFLICT(day) DO UPDATE SET weight=excluded.weight,steps=excluded.steps,minutes=excluded.minutes,calories=excluded.calories",params![p.effective,p.weight,p.steps,p.minutes,p.calories]).map_err(|e|e.to_string())?;
        Ok(())
    }
}
