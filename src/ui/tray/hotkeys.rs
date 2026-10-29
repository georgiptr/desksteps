//! Tray hotkeys implementation.
use super::*;
pub(super) fn sync_shortcuts(window: HWND, context: &Context) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, RegisterHotKey, UnregisterHotKey,
    };
    let Some(worker) = &context.worker else {
        return;
    };
    let view = worker.view.snapshot();
    let config = if view.development && !view.development_shortcuts {
        Default::default()
    } else {
        view.shortcuts
    };
    if context.shortcuts.borrow().as_ref() == Some(&config) {
        return;
    }
    let mut errors = vec![];
    for i in 0..4 {
        unsafe {
            let _ = UnregisterHotKey(Some(window), 500 + i);
        }
        if context.locked.get() && i < 3 {
            continue;
        }
        match crate::shortcuts::parse(&config.bindings[i as usize]) {
            Ok(Some((modifiers, key))) => {
                if unsafe {
                    RegisterHotKey(
                        Some(window),
                        500 + i,
                        HOT_KEY_MODIFIERS(modifiers | 0x4000),
                        key,
                    )
                }
                .is_err()
                {
                    errors.push(format!(
                        "Shortcut {} is unavailable",
                        config.bindings[i as usize]
                    ));
                }
            }
            Err(error) => errors.push(error),
            _ => {}
        }
    }
    let _ = worker
        .sender
        .try_send(Input::ShortcutStatus(if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        }));
    *context.shortcuts.borrow_mut() = Some(config);
}
