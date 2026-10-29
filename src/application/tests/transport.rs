//! Scripted transport for deterministic tests. No Windows or Bluetooth calls.
use super::ports::{ControlChannel, PendingWrite};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
#[derive(Clone)]
pub(super) struct Write {
    pub done: Rc<Cell<bool>>,
    pub failed: bool,
    pub cancelled: Rc<Cell<bool>>,
}
impl Write {
    pub fn completed() -> Self {
        Self {
            done: Rc::new(Cell::new(true)),
            failed: false,
            cancelled: Rc::new(Cell::new(false)),
        }
    }
}
impl PendingWrite for Write {
    fn poll(&self) -> Result<bool, String> {
        if self.failed {
            Err("synthetic write failure".into())
        } else {
            Ok(self.done.get())
        }
    }
    fn cancel(&self) {
        self.cancelled.set(true);
    }
}
pub(super) struct Channel {
    pub generation: u64,
    pub replies: RefCell<VecDeque<Vec<u8>>>,
}
impl Channel {
    pub fn new() -> Self {
        Self {
            generation: 7,
            replies: RefCell::new(VecDeque::new()),
        }
    }
    pub fn reply(&self, bytes: &[u8]) {
        self.replies.borrow_mut().push_back(bytes.to_vec());
    }
}
impl ControlChannel for Channel {
    type Write = Write;
    fn generation(&self) -> u64 {
        self.generation
    }
    fn next_reply(&self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.replies.borrow_mut().pop_front())
    }
}
