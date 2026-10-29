#![allow(non_upper_case_globals)] // UI Automation constants keep their documented names in patterns.
//! COM side of UI Automation. Providers only read a published snapshot, so calls from
//! automation threads never touch window state; actions are posted to the window.
use super::{ACCESS_FOCUS, ACCESS_INVOKE, Item, Kind};
use std::sync::{Arc, Mutex};
use windows::{
    Win32::{
        Foundation::{E_FAIL, HWND, LPARAM, LRESULT, POINT, WPARAM},
        Graphics::Gdi::{ClientToScreen, ScreenToClient},
        System::{
            Com::SAFEARRAY,
            Ole::{SafeArrayCreateVector, SafeArrayPutElement},
            Variant::{VARIANT, VT_I4},
        },
        UI::{
            Accessibility::*,
            WindowsAndMessaging::{PostMessageW, WM_APP},
        },
    },
    core::{Error, IUnknown, IUnknownImpl, Interface, Result, implement},
};

/// Shown for elements that disappeared between calls.
const ELEMENT_NOT_AVAILABLE: i32 = 0x8004_0201u32 as i32;
const ELEMENT_NOT_ENABLED: i32 = 0x8004_0200u32 as i32;
const _: u32 = WM_APP;

#[derive(Default)]
struct Published {
    items: Vec<Item>,
    /// Device pixels per logical pixel.
    scale: f32,
}
pub struct Shared {
    hwnd: isize,
    state: Mutex<Published>,
}
impl Shared {
    pub fn new(hwnd: HWND) -> Arc<Self> {
        Arc::new(Self {
            hwnd: hwnd.0 as isize,
            state: Mutex::new(Published {
                items: Vec::new(),
                scale: 1.0,
            }),
        })
    }
    pub fn publish(&self, items: Vec<Item>, scale: f32) {
        *self.state.lock().unwrap() = Published { items, scale };
    }
    fn window(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }
    fn item(&self, key: u32) -> Option<(Item, f32)> {
        let s = self.state.lock().unwrap();
        s.items
            .iter()
            .find(|i| i.key == key)
            .cloned()
            .map(|i| (i, s.scale))
    }
    /// The root provider handed to UI Automation for WM_GETOBJECT.
    pub fn root(self: &Arc<Self>) -> IRawElementProviderSimple {
        Node {
            shared: self.clone(),
            key: None,
        }
        .into()
    }
    #[cfg(test)]
    pub fn items(&self) -> Vec<Item> {
        self.state.lock().unwrap().items.clone()
    }
}
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot,
    IInvokeProvider,
    IToggleProvider
)]
pub struct Node {
    shared: Arc<Shared>,
    /// `None` is the window itself.
    key: Option<u32>,
}
impl Node {
    fn child(&self, key: u32) -> IRawElementProviderFragment {
        Node {
            shared: self.shared.clone(),
            key: Some(key),
        }
        .into()
    }
    fn me(&self) -> Result<(Item, f32)> {
        self.key
            .and_then(|k| self.shared.item(k))
            .ok_or_else(|| Error::from_hresult(windows::core::HRESULT(ELEMENT_NOT_AVAILABLE)))
    }
    fn index_of(&self, key: u32) -> Option<(usize, usize)> {
        let s = self.shared.state.lock().unwrap();
        s.items
            .iter()
            .position(|i| i.key == key)
            .map(|p| (p, s.items.len()))
    }
    fn key_at(&self, index: usize) -> Option<u32> {
        self.shared
            .state
            .lock()
            .unwrap()
            .items
            .get(index)
            .map(|i| i.key)
    }
    fn post(&self, message: u32, target: usize) {
        unsafe {
            let _ = PostMessageW(
                Some(self.shared.window()),
                message,
                WPARAM(target),
                LPARAM(0),
            );
        }
    }
}
fn none<T>() -> Result<T> {
    // S_OK with a null result: "no such element".
    Err(Error::empty())
}
impl IRawElementProviderSimple_Impl for Node_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }
    fn GetPatternProvider(&self, pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        let Ok((item, _)) = self.me() else {
            return none();
        };
        let invokable =
            item.target.is_some() && matches!(item.kind, Kind::Button | Kind::Tab | Kind::ListItem);
        if (pattern == UIA_InvokePatternId && invokable)
            || (pattern == UIA_TogglePatternId && item.kind == Kind::Toggle)
        {
            let me = self.to_object();
            return if pattern == UIA_InvokePatternId {
                me.to_interface::<IInvokeProvider>().cast()
            } else {
                me.to_interface::<IToggleProvider>().cast()
            };
        }
        none()
    }
    fn GetPropertyValue(&self, property: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let Ok((item, _)) = self.me() else {
            // The window itself: let the host HWND provider answer.
            return Ok(VARIANT::default());
        };
        Ok(match property {
            UIA_ControlTypePropertyId => VARIANT::from(
                match item.kind {
                    Kind::Button => UIA_ButtonControlTypeId,
                    Kind::Tab => UIA_TabItemControlTypeId,
                    Kind::Text => UIA_TextControlTypeId,
                    Kind::ListItem => UIA_ListItemControlTypeId,
                    Kind::Toggle => UIA_ButtonControlTypeId,
                }
                .0,
            ),
            UIA_NamePropertyId => VARIANT::from(item.name.as_str()),
            UIA_AutomationIdPropertyId => VARIANT::from(format!("DeskSteps.{}", item.key).as_str()),
            UIA_HelpTextPropertyId => match &item.help {
                Some(h) => VARIANT::from(h.as_str()),
                None => VARIANT::default(),
            },
            UIA_IsEnabledPropertyId => VARIANT::from(item.enabled),
            UIA_IsKeyboardFocusablePropertyId => {
                VARIANT::from(item.target.is_some() && item.enabled)
            }
            UIA_HasKeyboardFocusPropertyId => VARIANT::from(item.focused),
            UIA_IsOffscreenPropertyId => VARIANT::from(item.offscreen),
            UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId => VARIANT::from(true),
            _ => VARIANT::default(),
        })
    }
    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        if self.key.is_none() {
            unsafe { UiaHostProviderFromHwnd(self.shared.window()) }
        } else {
            none()
        }
    }
}
impl IRawElementProviderFragment_Impl for Node_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let Some(key) = self.key else {
            // The window: its children are the flat element list.
            let n = self.shared.state.lock().unwrap().items.len();
            return match direction {
                NavigateDirection_FirstChild => self
                    .key_at(0)
                    .map(|k| self.child(k))
                    .ok_or_else(Error::empty),
                NavigateDirection_LastChild => n
                    .checked_sub(1)
                    .and_then(|i| self.key_at(i))
                    .map(|k| self.child(k))
                    .ok_or_else(Error::empty),
                _ => none(),
            };
        };
        let Some((at, n)) = self.index_of(key) else {
            return Err(Error::from_hresult(windows::core::HRESULT(
                ELEMENT_NOT_AVAILABLE,
            )));
        };
        match direction {
            NavigateDirection_Parent => Ok(Node {
                shared: self.shared.clone(),
                key: None,
            }
            .into()),
            NavigateDirection_NextSibling if at + 1 < n => self
                .key_at(at + 1)
                .map(|k| self.child(k))
                .ok_or_else(Error::empty),
            NavigateDirection_PreviousSibling if at > 0 => self
                .key_at(at - 1)
                .map(|k| self.child(k))
                .ok_or_else(Error::empty),
            _ => none(),
        }
    }
    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        let Some(key) = self.key else {
            return Ok(std::ptr::null_mut());
        };
        unsafe {
            let array = SafeArrayCreateVector(VT_I4, 0, 2);
            if array.is_null() {
                return Err(Error::from_hresult(E_FAIL));
            }
            for (i, value) in [UiaAppendRuntimeId as i32, key as i32].iter().enumerate() {
                let index = i as i32;
                SafeArrayPutElement(array, &index, (value as *const i32).cast())?;
            }
            Ok(array)
        }
    }
    fn BoundingRectangle(&self) -> Result<UiaRect> {
        let (item, scale) = self.me()?;
        let mut origin = POINT {
            x: (item.rect[0] * scale).round() as i32,
            y: (item.rect[1] * scale).round() as i32,
        };
        unsafe {
            let _ = ClientToScreen(self.shared.window(), &mut origin);
        }
        Ok(UiaRect {
            left: origin.x as f64,
            top: origin.y as f64,
            width: (item.rect[2] * scale).round() as f64,
            height: (item.rect[3] * scale).round() as f64,
        })
    }
    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }
    fn SetFocus(&self) -> Result<()> {
        let (item, _) = self.me()?;
        match item.target.filter(|_| item.enabled) {
            Some(t) => {
                self.post(ACCESS_FOCUS, t as usize);
                Ok(())
            }
            None => Err(Error::from_hresult(windows::core::HRESULT(
                ELEMENT_NOT_ENABLED,
            ))),
        }
    }
    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(Node {
            shared: self.shared.clone(),
            key: None,
        }
        .into())
    }
}
impl IRawElementProviderFragmentRoot_Impl for Node_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let mut p = POINT {
            x: x as i32,
            y: y as i32,
        };
        unsafe {
            let _ = ScreenToClient(self.shared.window(), &mut p);
        }
        let hit = {
            let s = self.shared.state.lock().unwrap();
            let (lx, ly) = (p.x as f32 / s.scale, p.y as f32 / s.scale);
            s.items
                .iter()
                .rev()
                .find(|i| {
                    !i.offscreen
                        && lx >= i.rect[0]
                        && ly >= i.rect[1]
                        && lx < i.rect[0] + i.rect[2]
                        && ly < i.rect[1] + i.rect[3]
                })
                .map(|i| i.key)
        };
        hit.map(|k| self.child(k)).ok_or_else(Error::empty)
    }
    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        let key = self
            .shared
            .state
            .lock()
            .unwrap()
            .items
            .iter()
            .find(|i| i.focused)
            .map(|i| i.key);
        key.map(|k| self.child(k)).ok_or_else(Error::empty)
    }
}
impl IInvokeProvider_Impl for Node_Impl {
    fn Invoke(&self) -> Result<()> {
        let (item, _) = self.me()?;
        match item.target.filter(|_| item.enabled) {
            Some(t) => {
                self.post(ACCESS_INVOKE, t as usize);
                Ok(())
            }
            None => Err(Error::from_hresult(windows::core::HRESULT(
                ELEMENT_NOT_ENABLED,
            ))),
        }
    }
}
impl IToggleProvider_Impl for Node_Impl {
    fn Toggle(&self) -> Result<()> {
        IInvokeProvider_Impl::Invoke(self)
    }
    fn ToggleState(&self) -> Result<ToggleState> {
        let (item, _) = self.me()?;
        Ok(if item.on == Some(true) {
            ToggleState_On
        } else {
            ToggleState_Off
        })
    }
}
/// Answers WM_GETOBJECT for the UI Automation root object.
pub unsafe fn get_object(
    shared: &Arc<Shared>,
    window: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    if lparam.0 as i32 != UiaRootObjectId {
        return None;
    }
    let root = shared.root();
    Some(unsafe { UiaReturnRawElementProvider(window, wparam, lparam, &root) })
}
/// Drops UI Automation's references to this window's providers.
pub unsafe fn disconnect(window: HWND) {
    unsafe {
        let _ = UiaReturnRawElementProvider(
            window,
            WPARAM(0),
            LPARAM(0),
            None::<&IRawElementProviderSimple>,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    fn item(key: u32, name: &str, kind: Kind, target: Option<u32>, enabled: bool) -> Item {
        Item {
            key,
            target,
            name: name.into(),
            help: Some("help".into()),
            kind,
            rect: [10.0, 20.0, 30.0, 40.0],
            enabled,
            focused: key == 2,
            offscreen: false,
            on: (kind == Kind::Toggle).then_some(true),
        }
    }
    fn text(v: &VARIANT) -> String {
        windows::core::BSTR::try_from(v)
            .map(|b| b.to_string())
            .unwrap_or_default()
    }
    fn number(v: &VARIANT) -> i32 {
        i32::try_from(v).unwrap()
    }
    fn flag(v: &VARIANT) -> bool {
        bool::try_from(v).unwrap()
    }
    #[test]
    fn native_ui_access_provider_exposes_names_state_navigation_and_posts_invocations() {
        unsafe {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                windows::core::w!("STATIC"),
                windows::core::w!("Fixture"),
                WS_OVERLAPPED,
                0,
                0,
                400,
                300,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let shared = Shared::new(window);
            shared.publish(
                vec![
                    item(1, "Start or resume", Kind::Button, Some(3), false),
                    item(2, "Pause", Kind::Button, Some(4), true),
                    item(3, "Connect automatically", Kind::Toggle, Some(70), true),
                    item(100_001, "Steps: 700", Kind::Text, None, true),
                ],
                2.0,
            );
            let root = shared.root();
            let fragment: IRawElementProviderFragment = root.cast().unwrap();
            // Children of the window, in order.
            let first = fragment.Navigate(NavigateDirection_FirstChild).unwrap();
            let first_simple: IRawElementProviderSimple = first.cast().unwrap();
            assert_eq!(
                text(&first_simple.GetPropertyValue(UIA_NamePropertyId).unwrap()),
                "Start or resume"
            );
            assert!(!flag(
                &first_simple
                    .GetPropertyValue(UIA_IsEnabledPropertyId)
                    .unwrap()
            ));
            assert_eq!(
                number(
                    &first_simple
                        .GetPropertyValue(UIA_ControlTypePropertyId)
                        .unwrap()
                ),
                50000
            );
            let second = first.Navigate(NavigateDirection_NextSibling).unwrap();
            let second_simple: IRawElementProviderSimple = second.cast().unwrap();
            assert_eq!(
                text(&second_simple.GetPropertyValue(UIA_NamePropertyId).unwrap()),
                "Pause"
            );
            assert!(flag(
                &second_simple
                    .GetPropertyValue(UIA_HasKeyboardFocusPropertyId)
                    .unwrap()
            ));
            assert_eq!(
                text(
                    &second_simple
                        .GetPropertyValue(UIA_HelpTextPropertyId)
                        .unwrap()
                ),
                "help"
            );
            assert!(first.Navigate(NavigateDirection_PreviousSibling).is_err());
            let last = fragment.Navigate(NavigateDirection_LastChild).unwrap();
            let last_simple: IRawElementProviderSimple = last.cast().unwrap();
            assert_eq!(
                number(
                    &last_simple
                        .GetPropertyValue(UIA_ControlTypePropertyId)
                        .unwrap()
                ),
                50020
            );
            assert!(last.Navigate(NavigateDirection_NextSibling).is_err());
            // Parent is the window; the bounding rectangle is in device pixels.
            let parent = second.Navigate(NavigateDirection_Parent).unwrap();
            assert!(parent.GetRuntimeId().unwrap().is_null());
            let rect = second.BoundingRectangle().unwrap();
            assert_eq!((rect.width, rect.height), (60.0, 80.0));
            // Runtime ids are stable and distinct.
            assert!(!first.GetRuntimeId().unwrap().is_null());
            // Invoke is offered for enabled buttons only, and posts the activation instead of
            // acting on an automation thread.
            let disabled: IRawElementProviderSimple = first.cast().unwrap();
            let invoke = disabled.GetPatternProvider(UIA_InvokePatternId).unwrap();
            assert!(
                invoke.cast::<IInvokeProvider>().unwrap().Invoke().is_err(),
                "disabled Start cannot be invoked"
            );
            let pause_invoke = second_simple
                .GetPatternProvider(UIA_InvokePatternId)
                .unwrap()
                .cast::<IInvokeProvider>()
                .unwrap();
            pause_invoke.Invoke().unwrap();
            let mut message = MSG::default();
            assert!(
                PeekMessageW(
                    &mut message,
                    Some(window),
                    ACCESS_INVOKE,
                    ACCESS_INVOKE,
                    PM_REMOVE
                )
                .as_bool()
            );
            assert_eq!(message.wParam.0, 4);
            assert!(
                second_simple
                    .GetPatternProvider(UIA_TogglePatternId)
                    .is_err()
            );
            // The switch supports Toggle and reports its state.
            let toggle = fragment
                .Navigate(NavigateDirection_FirstChild)
                .unwrap()
                .Navigate(NavigateDirection_NextSibling)
                .unwrap()
                .Navigate(NavigateDirection_NextSibling)
                .unwrap();
            let toggle_simple: IRawElementProviderSimple = toggle.cast().unwrap();
            let state = toggle_simple
                .GetPatternProvider(UIA_TogglePatternId)
                .unwrap()
                .cast::<IToggleProvider>()
                .unwrap();
            assert_eq!(state.ToggleState().unwrap(), ToggleState_On);
            // Focus requests are posted too.
            second.SetFocus().unwrap();
            assert!(
                PeekMessageW(
                    &mut message,
                    Some(window),
                    ACCESS_FOCUS,
                    ACCESS_FOCUS,
                    PM_REMOVE
                )
                .as_bool()
            );
            // Elements that vanish report themselves unavailable.
            shared.publish(Vec::new(), 2.0);
            assert!(
                second_simple.GetPropertyValue(UIA_NamePropertyId).is_err()
                    || text(&second_simple.GetPropertyValue(UIA_NamePropertyId).unwrap())
                        .is_empty()
            );
            assert!(fragment.Navigate(NavigateDirection_FirstChild).is_err());
            assert!(shared.items().is_empty());
            let _ = DestroyWindow(window);
        }
    }
}
