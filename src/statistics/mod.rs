//! Domain statistics. SQL and Store implementations live exclusively in storage.
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
mod estimates;
mod model;
pub(crate) mod recovery;
pub(crate) mod sessions;
pub(crate) use estimates::*;
pub(crate) use model::*;
pub const MODEL: &str = "Compendium-2024-flat-treadmill-v1";
#[cfg(test)]
mod tests;
