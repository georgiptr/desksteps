//! Minimal transport contracts. A completed write is not device acceptance.
pub(super) trait PendingWrite {
    /// True only after successful GATT completion; false means still pending.
    fn poll(&self) -> Result<bool, String>;
    fn cancel(&self);
}
pub(super) trait ControlChannel {
    type Write: PendingWrite;
    fn generation(&self) -> u64;
    fn next_reply(&self) -> Result<Option<Vec<u8>>, String>;
}
