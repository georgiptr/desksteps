//! One link interface for real GATT and the development pad.
use super::ports::{ControlChannel, PendingWrite};
use super::*;
pub(super) enum WindowsWrite {
    Bluetooth(BluetoothWrite),
    Synthetic,
}
impl PendingWrite for WindowsWrite {
    fn poll(&self) -> Result<bool, String> {
        match self {
            Self::Bluetooth(w) => w.poll(),
            Self::Synthetic => Ok(true),
        }
    }
    fn cancel(&self) {
        if let Self::Bluetooth(w) = self {
            w.cancel();
        }
    }
}
enum Backend {
    Bluetooth(BluetoothLink),
    Synthetic(Arc<Mutex<simulator::Simulator>>),
}
pub(super) struct Link {
    pub generation: u64,
    pub control_uncertain: bool,
    backend: Backend,
}
impl Link {
    pub fn synthetic(pad: Arc<Mutex<simulator::Simulator>>, generation: u64) -> Self {
        pad.lock().unwrap().reconnect();
        Self {
            generation,
            control_uncertain: false,
            backend: Backend::Synthetic(pad),
        }
    }
    pub fn open(
        address: u64,
        generation: u64,
        tx: MessageSender,
        overflow: Arc<AtomicBool>,
        origin: Instant,
        controls: bool,
        cancel: &Cancellation,
    ) -> Result<Self, ConnectError> {
        BluetoothLink::open(address, generation, tx, overflow, origin, controls, cancel).map(
            |link| Self {
                generation,
                control_uncertain: link.control_uncertain,
                backend: Backend::Bluetooth(link),
            },
        )
    }
    pub fn control_write(&self, frame: &[u8]) -> Result<WindowsWrite, String> {
        match &self.backend {
            Backend::Bluetooth(link) => link.control_write(frame).map(WindowsWrite::Bluetooth),
            Backend::Synthetic(pad) => {
                pad.lock().unwrap().write(frame, Instant::now())?;
                Ok(WindowsWrite::Synthetic)
            }
        }
    }
}
impl ControlChannel for Link {
    type Write = WindowsWrite;
    fn generation(&self) -> u64 {
        self.generation
    }
    fn next_reply(&self) -> Result<Option<Vec<u8>>, String> {
        match &self.backend {
            Backend::Bluetooth(link) => link.next_reply(),
            Backend::Synthetic(pad) => Ok(pad.lock().unwrap().reply(Instant::now())),
        }
    }
}
