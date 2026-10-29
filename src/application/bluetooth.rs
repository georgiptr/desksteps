//! Windows Bluetooth adapter: GATT ownership, setup and callbacks.
use super::*;
mod discovery;
pub(super) use discovery::Discovery;
use windows::{
    Devices::Bluetooth::{
        Advertisement::{
            BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
            BluetoothLEAdvertisementWatcherStoppedEventArgs, BluetoothLEScanningMode,
        },
        BluetoothCacheMode, BluetoothConnectionStatus, BluetoothError, BluetoothLEDevice,
        GenericAttributeProfile::{
            GattCharacteristic, GattCharacteristicProperties,
            GattClientCharacteristicConfigurationDescriptorValue as Cccd, GattCommunicationStatus,
            GattDeviceService, GattSession, GattValueChangedEventArgs, GattWriteOption,
        },
    },
    Foundation::TypedEventHandler,
    Storage::Streams::{DataReader, DataWriter},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{GUID, HRESULT, Interface, RuntimeType},
};
use windows_future::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncInfo, IAsyncOperation};

pub(super) fn wait<T: RuntimeType + 'static>(
    op: IAsyncOperation<T>,
    cancel: &Cancellation,
) -> windows::core::Result<T> {
    let (tx, rx) = mpsc::sync_channel(1);
    op.SetCompleted(&AsyncOperationCompletedHandler::<T>::new(move |_, _| {
        let _ = tx.try_send(());
        Ok(())
    }))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            let _ = op.cast::<IAsyncInfo>()?.Cancel();
            return Err(windows::core::Error::new(
                HRESULT(0x800705B4u32 as i32),
                "Bluetooth operation cancelled or timed out",
            ));
        }
        if rx.recv_timeout(Duration::from_millis(250)).is_ok() {
            break;
        }
    }
    op.GetResults()
}
fn guid(value: &str) -> GUID {
    GUID::from_u128(u128::from_str_radix(&value.replace('-', ""), 16).unwrap())
}
fn uuid(value: GUID) -> String {
    format!("{value:?}").trim_matches(['{', '}']).to_lowercase()
}
fn read_buffer(buffer: &windows::Storage::Streams::IBuffer) -> windows::core::Result<Vec<u8>> {
    let reader = DataReader::FromBuffer(buffer)?;
    let mut bytes = vec![0; reader.UnconsumedBufferLength()? as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}
pub(super) fn enqueue(tx: &MessageSender, overflow: &AtomicBool, input: DeviceEvent) {
    if tx.try_send(input).is_err() {
        overflow.store(true, Ordering::Relaxed);
    }
}
pub(super) struct BluetoothLink {
    device: BluetoothLEDevice,
    session: Option<GattSession>,
    services: Vec<GattDeviceService>,
    subscriptions: Vec<(GattCharacteristic, i64)>,
    status_token: Option<i64>,
    pub(super) generation: u64,
    pub(super) control: Option<(GattCharacteristic, Receiver<Vec<u8>>)>,
    pub(super) control_uncertain: bool,
}
#[derive(Debug)]
pub(super) enum ConnectError {
    NameUnavailable(String),
    Other(String),
}
impl From<windows::core::Error> for ConnectError {
    fn from(error: windows::core::Error) -> Self {
        Self::Other(error.to_string())
    }
}
impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NameUnavailable(name) => {
                write!(
                    f,
                    "Windows has not resolved the selected device's name yet: {name:?}"
                )
            }
            Self::Other(message) => f.write_str(message),
        }
    }
}
pub(super) fn verify_device_name(name: &str, target: u64) -> Result<(), ConnectError> {
    let placeholder_address = name
        .strip_prefix("Bluetooth ")
        .and_then(|value| crate::parse_address(value).ok());
    if name.is_empty() || placeholder_address == Some(target) {
        Err(ConnectError::NameUnavailable(name.into()))
    } else if name != protocol::MODEL_NAME {
        Err(ConnectError::Other(format!(
            "Selected device has Windows name {name:?}; expected {}",
            protocol::MODEL_NAME
        )))
    } else {
        Ok(())
    }
}
pub(super) struct ScanGuard(
    pub(super) BluetoothLEAdvertisementWatcher,
    pub(super) i64,
    pub(super) i64,
);
impl Drop for ScanGuard {
    fn drop(&mut self) {
        let _ = self.0.Stop();
        let _ = self.0.RemoveReceived(self.1);
        let _ = self.0.RemoveStopped(self.2);
    }
}
impl Drop for BluetoothLink {
    fn drop(&mut self) {
        for (characteristic, token) in &self.subscriptions {
            let _ = characteristic.RemoveValueChanged(*token);
        }
        if let Some(token) = self.status_token {
            let _ = self.device.RemoveConnectionStatusChanged(token);
        }
        if let Some(session) = &self.session {
            let _ = session.SetMaintainConnection(false);
            let _ = session.Close();
        }
        for service in &self.services {
            let _ = service.Close();
        }
        let _ = self.device.Close();
    }
}
struct Setup<'a> {
    target: u64,
    generation: u64,
    tx: MessageSender,
    overflow: Arc<AtomicBool>,
    origin: Instant,
    controls_enabled: bool,
    cancel: &'a Cancellation,
}
impl BluetoothLink {
    pub(super) fn open(
        target: u64,
        generation: u64,
        tx: MessageSender,
        overflow: Arc<AtomicBool>,
        origin: Instant,
        controls_enabled: bool,
        cancel: &Cancellation,
    ) -> Result<Self, ConnectError> {
        let device = wait(
            BluetoothLEDevice::FromBluetoothAddressAsync(target)?,
            cancel,
        )?;
        let name = device.Name()?.to_string();
        if let Err(error) = verify_device_name(&name, target) {
            let _ = device.Close();
            return Err(error);
        }
        enqueue(
            &tx,
            &overflow,
            DeviceEvent::Found(
                Candidate {
                    address: target,
                    name,
                },
                Instant::now(),
            ),
        );
        Self::open_inner(
            device,
            Setup {
                target,
                generation,
                tx,
                overflow,
                origin,
                controls_enabled,
                cancel,
            },
        )
        .map_err(Into::into)
    }
    fn open_inner(device: BluetoothLEDevice, setup: Setup<'_>) -> windows::core::Result<Self> {
        let Setup {
            target,
            generation,
            tx,
            overflow,
            origin,
            controls_enabled,
            cancel,
        } = setup;
        let mut link = BluetoothLink {
            device,
            session: None,
            services: vec![],
            subscriptions: vec![],
            status_token: None,
            generation,
            control: None,
            control_uncertain: false,
        };
        let session = wait(
            GattSession::FromDeviceIdAsync(&link.device.BluetoothDeviceId()?)?,
            cancel,
        )?;
        session.SetMaintainConnection(true)?;
        link.session = Some(session);
        let tx_lost = tx.clone();
        let overflow_lost = overflow.clone();
        link.status_token =
            Some(link.device.ConnectionStatusChanged(&TypedEventHandler::<
                BluetoothLEDevice,
                windows::core::IInspectable,
            >::new(move |sender, _| {
                if let Some(sender) = sender.as_ref()
                    && sender.ConnectionStatus()? == BluetoothConnectionStatus::Disconnected
                {
                    enqueue(&tx_lost, &overflow_lost, DeviceEvent::Lost(generation));
                }
                Ok(())
            }))?);
        let result = wait(
            link.device
                .GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
            cancel,
        )?;
        if result.Status()? != GattCommunicationStatus::Success {
            return Err(windows::core::Error::from_hresult(HRESULT(
                0x80004005u32 as i32,
            )));
        }
        let mut characteristics = vec![];
        for service in result.Services()? {
            let service_id = uuid(service.Uuid()?);
            link.services.push(service.clone());
            if ![
                "00001826-0000-1000-8000-00805f9b34fb",
                "0000fe00-0000-1000-8000-00805f9b34fb",
                "0000fff0-0000-1000-8000-00805f9b34fb",
                "24e2521c-f63b-48ed-85be-c5330a00fdf7",
                "0000180a-0000-1000-8000-00805f9b34fb",
            ]
            .contains(&service_id.as_str())
            {
                continue;
            }
            let result = wait(
                service.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
                cancel,
            )?;
            if result.Status()? != GattCommunicationStatus::Success {
                continue;
            }
            for characteristic in result.Characteristics()? {
                characteristics.push(characteristic);
            }
        }
        let write = characteristics
            .iter()
            .find(|c| c.Uuid().ok() == Some(guid(protocol::SUPPLEMENT_WRITE)))
            .cloned()
            .ok_or_else(|| {
                windows::core::Error::new(
                    HRESULT(0x80004005u32 as i32),
                    "Missing supplement handshake characteristic",
                )
            })?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let mut required_treadmill = false;
        let mut required_supplement = false;
        for characteristic in &characteristics {
            let name = uuid(characteristic.Uuid()?);
            let props = characteristic.CharacteristicProperties()?;
            if (props & GattCharacteristicProperties::Notify) == GattCharacteristicProperties::None
            {
                continue;
            }
            let notify_tx = tx.clone();
            let notify_overflow = overflow.clone();
            let notify_name = name.clone();
            let notify_reply = reply_tx.clone();
            let token = characteristic.ValueChanged(&TypedEventHandler::<
                GattCharacteristic,
                GattValueChangedEventArgs,
            >::new(move |_, args| {
                if let Some(args) = args.as_ref() {
                    let bytes = match args.CharacteristicValue().and_then(|b| read_buffer(&b)) {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            enqueue(
                                &notify_tx,
                                &notify_overflow,
                                DeviceEvent::Raw {
                                    generation,
                                    device: target,
                                    utc: Utc::now(),
                                    mono: origin.elapsed().as_nanos() as i64,
                                    kind: "notification_error",
                                    uuid: notify_name.clone(),
                                    bytes: vec![],
                                    detail: error.to_string(),
                                },
                            );
                            return Ok(());
                        }
                    };
                    if notify_name == protocol::SUPPLEMENT
                        && let Some(status) = protocol::handshake_status(&bytes)
                    {
                        let _ = notify_reply.try_send(status);
                    }
                    enqueue(
                        &notify_tx,
                        &notify_overflow,
                        DeviceEvent::Raw {
                            generation,
                            device: target,
                            utc: Utc::now(),
                            mono: origin.elapsed().as_nanos() as i64,
                            kind: "notification",
                            uuid: notify_name.clone(),
                            bytes,
                            detail: String::new(),
                        },
                    );
                }
                Ok(())
            }))?;
            link.subscriptions.push((characteristic.clone(), token));
            if name == protocol::TREADMILL {
                // Start from an explicit disabled state, including after a
                // peripheral power cycle. Do not rely on an earlier CCCD value.
                let result = wait(
                    characteristic
                        .WriteClientCharacteristicConfigurationDescriptorWithResultAsync(
                            Cccd::None,
                        )?,
                    cancel,
                )?;
                let status = result.Status()?;
                enqueue(
                    &tx,
                    &overflow,
                    DeviceEvent::Raw {
                        generation,
                        device: target,
                        utc: Utc::now(),
                        mono: origin.elapsed().as_nanos() as i64,
                        kind: "subscription_reset",
                        uuid: name.clone(),
                        bytes: vec![],
                        detail: format!(
                            "handle={} status={status:?}",
                            characteristic.AttributeHandle()?
                        ),
                    },
                );
                if status != GattCommunicationStatus::Success {
                    return Err(windows::core::Error::new(
                        HRESULT(0x80004005u32 as i32),
                        "Could not reset treadmill notification subscription",
                    ));
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            let result = wait(
                characteristic.WriteClientCharacteristicConfigurationDescriptorWithResultAsync(
                    Cccd::Notify,
                )?,
                cancel,
            )?;
            let status = result.Status()?;
            enqueue(
                &tx,
                &overflow,
                DeviceEvent::Raw {
                    generation,
                    device: target,
                    utc: Utc::now(),
                    mono: origin.elapsed().as_nanos() as i64,
                    kind: "subscription_enabled",
                    uuid: name.clone(),
                    bytes: vec![],
                    detail: format!(
                        "handle={} status={status:?}",
                        characteristic.AttributeHandle()?
                    ),
                },
            );
            if status != GattCommunicationStatus::Success {
                if name == protocol::TREADMILL || name == protocol::SUPPLEMENT {
                    return Err(windows::core::Error::new(
                        HRESULT(0x80004005u32 as i32),
                        "Required telemetry subscription failed",
                    ));
                }
            } else {
                required_treadmill |= name == protocol::TREADMILL;
                required_supplement |= name == protocol::SUPPLEMENT;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        if !required_treadmill || !required_supplement {
            return Err(windows::core::Error::new(
                HRESULT(0x80004005u32 as i32),
                "Required treadmill/supplement notifications missing",
            ));
        }
        if (write.CharacteristicProperties()? & GattCharacteristicProperties::WriteWithoutResponse)
            == GattCharacteristicProperties::None
        {
            return Err(windows::core::Error::new(
                HRESULT(0x80004005u32 as i32),
                "Unexpected handshake write capability",
            ));
        }
        // Default capture mode sends only this verified connection frame.
        let nonce = (Utc::now().timestamp_subsec_nanos() % 255) as u8;
        let frame = protocol::handshake(protocol::MODEL_NAME, nonce).unwrap();
        let writer = DataWriter::new()?;
        writer.WriteBytes(&frame)?;
        enqueue(
            &tx,
            &overflow,
            DeviceEvent::Raw {
                generation,
                device: target,
                utc: Utc::now(),
                mono: origin.elapsed().as_nanos() as i64,
                kind: "handshake_write",
                uuid: protocol::SUPPLEMENT_WRITE.into(),
                bytes: frame,
                detail: String::new(),
            },
        );
        if wait(
            write.WriteValueWithOptionAsync(
                &writer.DetachBuffer()?,
                GattWriteOption::WriteWithoutResponse,
            )?,
            cancel,
        )? != GattCommunicationStatus::Success
        {
            return Err(windows::core::Error::new(
                HRESULT(0x80004005u32 as i32),
                "Connection handshake write failed",
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut accepted = false;
        while Instant::now() < deadline && !cancel.is_cancelled() {
            match reply_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(0) => {
                    accepted = true;
                    break;
                }
                Ok(_) => break,
                Err(_) => {}
            }
        }
        if !accepted {
            return Err(windows::core::Error::new(
                HRESULT(0x80004005u32 as i32),
                "No successful supplement handshake reply",
            ));
        }
        if controls_enabled {
            let characteristic = characteristics
                .iter()
                .find(|c| c.Uuid().ok() == Some(guid(control::UUID)))
                .cloned()
                .ok_or_else(|| {
                    windows::core::Error::new(
                        HRESULT(0x80004005u32 as i32),
                        "Missing FTMS Control Point",
                    )
                })?;
            let (reply_tx, reply_rx) = mpsc::sync_channel(16);
            let raw_tx = tx.clone();
            let raw_overflow = overflow.clone();
            let token = characteristic.ValueChanged(&TypedEventHandler::<
                GattCharacteristic,
                GattValueChangedEventArgs,
            >::new(move |_, args| {
                if let Some(args) = args.as_ref() {
                    let bytes = read_buffer(&args.CharacteristicValue()?)?;
                    let _ = reply_tx.try_send(bytes.clone());
                    enqueue(
                        &raw_tx,
                        &raw_overflow,
                        DeviceEvent::Raw {
                            generation,
                            device: target,
                            utc: Utc::now(),
                            mono: origin.elapsed().as_nanos() as i64,
                            kind: "notification",
                            uuid: control::UUID.into(),
                            bytes,
                            detail: String::new(),
                        },
                    );
                }
                Ok(())
            }))?;
            link.subscriptions.push((characteristic.clone(), token));
            let result = wait(
                characteristic.WriteClientCharacteristicConfigurationDescriptorWithResultAsync(
                    Cccd::Indicate,
                )?,
                cancel,
            )?;
            if result.Status()? != GattCommunicationStatus::Success {
                return Err(windows::core::Error::new(
                    HRESULT(0x80004005u32 as i32),
                    "Control Point indication subscription failed",
                ));
            }
            link.control = Some((characteristic, reply_rx));
        }
        // Read identity metadata once, retaining values locally with raw provenance.
        for characteristic in &characteristics {
            let name = uuid(characteristic.Uuid()?);
            if [
                "00002a24-0000-1000-8000-00805f9b34fb",
                "00002a26-0000-1000-8000-00805f9b34fb",
                "00002a28-0000-1000-8000-00805f9b34fb",
            ]
            .contains(&name.as_str())
                && let Ok(result) = characteristic
                    .ReadValueAsync()
                    .and_then(|op| wait(op, cancel))
                && result.Status()? == GattCommunicationStatus::Success
            {
                let bytes = read_buffer(&result.Value()?)?;
                enqueue(
                    &tx,
                    &overflow,
                    DeviceEvent::Raw {
                        generation,
                        device: target,
                        utc: Utc::now(),
                        mono: origin.elapsed().as_nanos() as i64,
                        kind: "metadata",
                        uuid: name,
                        detail: String::from_utf8_lossy(&bytes).into_owned(),
                        bytes,
                    },
                );
            }
        }
        Ok(link)
    }

    pub(super) fn control_write(&self, frame: &[u8]) -> Result<BluetoothWrite, String> {
        let (characteristic, receiver) = self.control.as_ref().ok_or("Controls unavailable")?;
        while receiver.try_recv().is_ok() {}
        let writer = DataWriter::new().map_err(|e| e.to_string())?;
        writer.WriteBytes(frame).map_err(|e| e.to_string())?;
        characteristic
            .WriteValueWithOptionAsync(
                &writer.DetachBuffer().map_err(|e| e.to_string())?,
                GattWriteOption::WriteWithResponse,
            )
            .map(BluetoothWrite)
            .map_err(|e| e.to_string())
    }
}

pub(super) struct Apartment;
impl Apartment {
    pub(super) fn enter() -> Result<Self, String> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED).map_err(|e| e.to_string())?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            RoUninitialize();
        }
    }
}

pub(super) struct BluetoothWrite(IAsyncOperation<GattCommunicationStatus>);
impl super::ports::PendingWrite for BluetoothWrite {
    fn poll(&self) -> Result<bool, String> {
        if self
            .0
            .cast::<IAsyncInfo>()
            .map_err(|e| e.to_string())?
            .Status()
            .map_err(|e| e.to_string())?
            == AsyncStatus::Started
        {
            return Ok(false);
        }
        if self.0.GetResults().map_err(|e| e.to_string())? != GattCommunicationStatus::Success {
            return Err("Control GATT write failed; outcome unknown".into());
        }
        Ok(true)
    }
    fn cancel(&self) {
        if let Ok(info) = self.0.cast::<IAsyncInfo>() {
            let _ = info.Cancel();
        }
    }
}
impl super::ports::ControlChannel for BluetoothLink {
    type Write = BluetoothWrite;
    fn generation(&self) -> u64 {
        self.generation
    }
    fn next_reply(&self) -> Result<Option<Vec<u8>>, String> {
        let (_, rx) = self.control.as_ref().ok_or("Control channel unavailable")?;
        Ok(rx.try_recv().ok())
    }
}
