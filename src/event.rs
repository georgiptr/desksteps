//! Raw evidence passed to recording and projections. No platform or database dependencies.
use chrono::{DateTime, Utc};
pub struct Event<'a> {
    pub key: &'a str,
    pub device: &'a str,
    pub connection: &'a str,
    pub utc: DateTime<Utc>,
    pub monotonic_ns: Option<i64>,
    pub kind: &'a str,
    pub uuid: &'a str,
    pub raw: &'a [u8],
    pub detail: &'a str,
}
