//! SQLite ownership. Recording owns one transaction across all source and derived writes.
use crate::protocol::Sample;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{path::Path, time::Duration};

pub struct Store {
    pub(crate) conn: Connection,
    pub(crate) timezone: Tz,
    pub(crate) ownership: Option<crate::database_ownership::DatabaseOwnership>,
}

pub(crate) use crate::event::Event;
#[derive(Debug, Default, Clone, Serialize)]
pub struct Totals {
    pub day: String,
    pub recorded_steps: i64,
    pub uncertain_events: i64,
    pub samples: i64,
}

mod pad_calories;
mod queries;
mod recording;
mod recovery;
pub(crate) mod schema;
mod sessions;
mod settings;
impl Store {
    pub fn open(path: &Path, timezone: Tz) -> Result<Self, String> {
        Self::open_owned(path, timezone, None)
    }
    pub(crate) fn open_owned(
        path: &Path,
        timezone: Tz,
        ownership: Option<crate::database_ownership::DatabaseOwnership>,
    ) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let ownership = match ownership {
            Some(owner) => owner,
            None => crate::database_ownership::DatabaseOwnership::acquire(path)?,
        };
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        let mut store = Self::initialize(conn, timezone)?;
        store.ownership = Some(ownership);
        Ok(store)
    }
}
#[cfg(test)]
mod tests;
