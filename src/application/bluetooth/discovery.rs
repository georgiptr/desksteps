//! Windows advertisement watcher and callback ownership.
use super::*;
pub(in crate::application) struct Discovery {
    watcher: Option<BluetoothLEAdvertisementWatcher>,
    _guard: Option<ScanGuard>,
}
impl Discovery {
    pub(in crate::application) fn new(
        enabled: bool,
        tx: MessageSender,
        overflow: Arc<AtomicBool>,
        shared_target: Arc<Mutex<Option<u64>>>,
    ) -> Result<Self, String> {
        let mut watcher = None;
        let mut watcher_token = None;
        let mut stopped_token = None;
        if enabled {
            let w = BluetoothLEAdvertisementWatcher::new().map_err(|e| e.to_string())?;
            w.SetScanningMode(BluetoothLEScanningMode::Passive)
                .map_err(|e| e.to_string())?;
            let ad_tx = tx.clone();
            let ad_target = shared_target.clone();
            let ad_overflow = overflow.clone();
            watcher_token = Some(
                w.Received(&TypedEventHandler::<
                    BluetoothLEAdvertisementWatcher,
                    BluetoothLEAdvertisementReceivedEventArgs,
                >::new(move |_, args| {
                    if let Some(args) = args.as_ref() {
                        let addr = args.BluetoothAddress()?;
                        let name = args.Advertisement()?.LocalName()?.to_string();
                        if name == protocol::MODEL_NAME || *ad_target.lock().unwrap() == Some(addr)
                        {
                            enqueue(
                                &ad_tx,
                                &ad_overflow,
                                DeviceEvent::Found(
                                    Candidate {
                                        address: addr,
                                        name,
                                    },
                                    Instant::now(),
                                ),
                            );
                        }
                    }
                    Ok(())
                }))
                .map_err(|e| e.to_string())?,
            );
            let stopped_tx = tx.clone();
            let stopped_overflow = overflow.clone();
            stopped_token = Some(
                w.Stopped(&TypedEventHandler::<
                    BluetoothLEAdvertisementWatcher,
                    BluetoothLEAdvertisementWatcherStoppedEventArgs,
                >::new(move |_, args| {
                    if let Some(args) = args.as_ref() {
                        let error = args.Error()?;
                        enqueue(
                            &stopped_tx,
                            &stopped_overflow,
                            DeviceEvent::ScanStopped {
                                error: format!("{error:?}"),
                                interrupted: error != BluetoothError::Success,
                            },
                        );
                    }
                    Ok(())
                }))
                .map_err(|e| e.to_string())?,
            );
            watcher = Some(w);
        }
        let guard = watcher
            .as_ref()
            .zip(watcher_token)
            .zip(stopped_token)
            .map(|((w, token), stopped)| ScanGuard(w.clone(), token, stopped));
        Ok(Self {
            watcher,
            _guard: guard,
        })
    }
    pub(in crate::application) fn available(&self) -> bool {
        self.watcher.is_some()
    }
    pub(in crate::application) fn start(&self) -> Result<(), String> {
        self.watcher
            .as_ref()
            .ok_or("Bluetooth disabled")?
            .Start()
            .map_err(|e| e.to_string())
    }
    pub(in crate::application) fn stop(&self) {
        if let Some(w) = &self.watcher {
            let _ = w.Stop();
        }
    }
    pub(in crate::application) fn active(&self, active: bool) -> Result<(), String> {
        if let Some(w) = &self.watcher {
            w.SetScanningMode(if active {
                BluetoothLEScanningMode::Active
            } else {
                BluetoothLEScanningMode::Passive
            })
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
