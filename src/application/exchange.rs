//! Serialized exchange adapter. Protocol acceptance remains distinct from GATT success.
use super::ports::{ControlChannel, PendingWrite};
use super::*;
pub(super) struct Exchange<W: PendingWrite> {
    pub(super) transaction: control::Transaction,
    pub(super) write: Option<W>,
    pub(super) written: bool,
}
impl<W: PendingWrite> Exchange<W> {
    pub(super) fn advance(
        &mut self,
        link: &impl ControlChannel<Write = W>,
        now: i64,
    ) -> Result<Option<Vec<u8>>, String> {
        if self.transaction.generation() != link.generation() {
            return Err("Connection changed; control discarded".into());
        }
        if self.transaction.expired(now) {
            return Err(
                "Control confirmation timed out; outcome unknown; no automatic retry".into(),
            );
        }
        if self.transaction.stage() == control::Stage::Settling {
            return Ok(None);
        }
        if let Some(write) = &self.write {
            if !write.poll()? {
                return Ok(None);
            }
            self.write = None;
            self.written = true;
            if self.transaction.command().is_speed()
                && self.transaction.stage() == control::Stage::Action
            {
                self.transaction.speed_written(now);
                return Ok(None);
            }
        }
        if self.written {
            while let Some(bytes) = link.next_reply()? {
                if self.transaction.reply(&bytes, now)? {
                    if self.transaction.stage() == control::Stage::Action {
                        self.written = false;
                        return Ok(Some(self.transaction.bytes().to_vec()));
                    }
                    break;
                }
            }
        }
        Ok(None)
    }
}
impl<W: PendingWrite> Drop for Exchange<W> {
    fn drop(&mut self) {
        if let Some(write) = &self.write {
            write.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::test_transport::{Channel, Write};
    fn pending() -> Exchange<Write> {
        Exchange {
            transaction: control::Transaction::new(control::Command::Increase, Some(3.0), 1, 7, 0)
                .unwrap(),
            write: Some(Write::completed()),
            written: false,
        }
    }
    #[test]
    fn gatt_completion_alone_does_not_accept_or_send_the_action() {
        let mut e = pending();
        let c = Channel::new();
        assert_eq!(e.advance(&c, 1).unwrap(), None);
        assert_eq!(e.transaction.stage(), control::Stage::Access);
        assert!(!e.transaction.confirmed(7, 2, 3.1));
    }
    #[test]
    fn delayed_write_does_not_consume_an_early_reply() {
        let mut e = pending();
        let c = Channel::new();
        c.reply(&[0x80, 0, 1]);
        e.write.as_ref().unwrap().done.set(false);
        assert_eq!(e.advance(&c, 1).unwrap(), None);
        assert_eq!(c.replies.borrow().len(), 1);
        e.write.as_ref().unwrap().done.set(true);
        assert_eq!(e.advance(&c, 2).unwrap(), Some(vec![2, 54, 1]));
    }
    #[test]
    fn access_then_action_then_fresh_motion_are_three_distinct_steps() {
        let mut e = pending();
        let c = Channel::new();
        c.reply(&[0x80, 0, 1]);
        assert!(e.advance(&c, 1).unwrap().is_some());
        assert!(!e.transaction.confirmed(7, 2, 3.1));
        e.write = Some(Write::completed());
        c.reply(&[0x80, 2, 1, 54, 1]);
        assert_eq!(e.advance(&c, 3).unwrap(), None);
        assert_eq!(e.transaction.stage(), control::Stage::Settling);
        assert!(!e.transaction.confirmed(7, 3, 3.1));
        assert!(e.transaction.confirmed(7, 4, 3.1));
        assert_eq!(e.advance(&c, 4).unwrap(), None);
    }
    #[test]
    fn unrelated_replies_are_ignored_without_ending_the_exchange() {
        let mut e = pending();
        let c = Channel::new();
        c.reply(&[0x80, 9, 1]);
        c.reply(&[0x80, 0, 1]);
        assert!(e.advance(&c, 1).unwrap().is_some());
    }
    #[test]
    fn access_rejection_is_an_error_but_speed_does_not_wait_for_an_echo() {
        let mut e = pending();
        let c = Channel::new();
        c.reply(&[0x80, 0, 2]);
        assert!(e.advance(&c, 1).unwrap_err().contains("rejected"));
        let mut e = pending();
        c.reply(&[0x80, 0, 1]);
        e.advance(&c, 1).unwrap();
        e.write = Some(Write::completed());
        c.reply(&[0x80, 2, 1, 0, 0]);
        assert_eq!(e.advance(&c, 2).unwrap(), None);
        assert_eq!(e.transaction.stage(), control::Stage::Settling);
        assert_eq!(c.replies.borrow().len(), 1); // retained for background diagnosis
    }
    #[test]
    fn speed_write_finishes_without_any_indication_and_waits_only_for_gatt() {
        let mut e = pending();
        e.transaction.use_access(0);
        let c = Channel::new();
        e.write.as_ref().unwrap().done.set(false);
        assert_eq!(e.advance(&c, 1).unwrap(), None);
        assert_eq!(e.transaction.stage(), control::Stage::Action);
        e.write.as_ref().unwrap().done.set(true);
        assert_eq!(e.advance(&c, 2).unwrap(), None);
        assert_eq!(e.transaction.stage(), control::Stage::Settling);
        assert!(e.transaction.confirmed(7, 3, 3.1));
    }
    #[test]
    fn changed_generation_deadline_and_write_failure_do_not_retry() {
        let mut e = pending();
        let mut c = Channel::new();
        c.generation = 8;
        assert!(e.advance(&c, 1).unwrap_err().contains("changed"));
        c.generation = 7;
        assert!(
            e.advance(&c, control::REPLY_NS)
                .unwrap_err()
                .contains("no automatic retry")
        );
        let mut e = pending();
        e.write.as_mut().unwrap().failed = true;
        assert!(e.advance(&c, 1).unwrap_err().contains("write failure"));
        assert_eq!(e.transaction.stage(), control::Stage::Access);
    }
    #[test]
    fn dropping_an_inflight_exchange_cancels_only_its_own_write() {
        let e = pending();
        let cancelled = e.write.as_ref().unwrap().cancelled.clone();
        drop(e);
        assert!(cancelled.get());
    }
}
