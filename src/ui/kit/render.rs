//! Direct2D window render target, DirectWrite text and device-loss recovery.
//! All drawing coordinates are logical pixels; the target applies the window DPI.
use super::{
    layout::Rect,
    theme::{Color, TextStyle},
};
use std::{
    cell::Cell,
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::DXGI_FORMAT_UNKNOWN,
        },
        UI::WindowsAndMessaging::GetClientRect,
    },
    core::{BOOL, PCWSTR},
};

const RECREATE_TARGET: i32 = 0x8899000Cu32 as i32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    size: u32,
    weight: u16,
    align: u8,
    wrap: bool,
    icon: bool,
}
struct Target {
    rt: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
}
thread_local! {
    // All app windows draw on their UI thread. A weak cache shares the factory
    // without retaining its device resources after the last window closes.
    static FACTORY: RefCell<Weak<ID2D1Factory>> = const { RefCell::new(Weak::new()) };
}
fn shared_factory() -> windows::core::Result<Rc<ID2D1Factory>> {
    FACTORY.with(|slot| {
        if let Some(factory) = slot.borrow().upgrade() {
            return Ok(factory);
        }
        let factory =
            Rc::new(unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? });
        *slot.borrow_mut() = Rc::downgrade(&factory);
        Ok(factory)
    })
}
pub struct Renderer {
    hwnd: HWND,
    factory: Rc<ID2D1Factory>,
    write: IDWriteFactory,
    target: RefCell<Option<Target>>,
    dpi: Cell<f32>,
    formats: RefCell<HashMap<Key, IDWriteTextFormat>>,
    body_font: Vec<u16>,
    display_font: Vec<u16>,
    icon_font: Vec<u16>,
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn d2d(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: c.a as f32 / 255.0,
    }
}
fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}
impl Renderer {
    pub fn new(hwnd: HWND, dpi: u32) -> windows::core::Result<Self> {
        unsafe {
            let factory = shared_factory()?;
            let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let mut collection = None;
            write.GetSystemFontCollection(&mut collection, false)?;
            let installed = |name: &str| {
                let (mut index, mut exists) = (0u32, BOOL(0));
                collection.as_ref().is_some_and(|c| {
                    let name = wide(name);
                    c.FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists)
                        .is_ok()
                        && exists.as_bool()
                })
            };
            let pick = |names: &[&str]| {
                wide(
                    names
                        .iter()
                        .find(|n| installed(n))
                        .unwrap_or(names.last().unwrap()),
                )
            };
            Ok(Self {
                hwnd,
                factory,
                body_font: pick(&["Segoe UI Variable Text", "Segoe UI"]),
                display_font: pick(&["Segoe UI Variable Display", "Segoe UI"]),
                icon_font: pick(&["Segoe Fluent Icons", "Segoe MDL2 Assets"]),
                write,
                target: RefCell::new(None),
                dpi: Cell::new(dpi.max(96) as f32),
                formats: RefCell::new(HashMap::new()),
            })
        }
    }
    pub fn set_dpi(&self, dpi: u32) {
        self.dpi.set(dpi.max(96) as f32);
        if let Some(t) = self.target.borrow().as_ref() {
            unsafe { t.rt.SetDpi(self.dpi.get(), self.dpi.get()) };
        }
    }
    /// Resizes the swap surface to the window's client size in physical pixels.
    pub fn resize(&self, width: u32, height: u32) {
        let failed = self.target.borrow().as_ref().is_some_and(|t| unsafe {
            t.rt.Resize(&D2D_SIZE_U {
                width: width.max(1),
                height: height.max(1),
            })
            .is_err()
        });
        if failed {
            self.discard_target();
        }
    }
    pub fn discard_target(&self) {
        self.target.borrow_mut().take();
    }
    pub fn has_target(&self) -> bool {
        self.target.borrow().is_some()
    }
    fn create_target(&self) -> windows::core::Result<Target> {
        unsafe {
            let mut client = RECT::default();
            GetClientRect(self.hwnd, &mut client)?;
            let dpi = self.dpi.get();
            let rt = self.factory.CreateHwndRenderTarget(
                &D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_UNKNOWN,
                        alphaMode: D2D1_ALPHA_MODE_UNKNOWN,
                    },
                    dpiX: dpi,
                    dpiY: dpi,
                    usage: D2D1_RENDER_TARGET_USAGE_NONE,
                    minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
                },
                &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd: self.hwnd,
                    pixelSize: D2D_SIZE_U {
                        width: (client.right - client.left).max(1) as u32,
                        height: (client.bottom - client.top).max(1) as u32,
                    },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                },
            )?;
            let brush = rt.CreateSolidColorBrush(&d2d(Color::rgb(0)), None)?;
            Ok(Target { rt, brush })
        }
    }
    /// Draws one frame. A lost device discards the target; the caller repaints once.
    pub fn paint(
        &self,
        clear: Color,
        draw: impl FnOnce(&Painter),
    ) -> Result<(), windows::core::Error> {
        if !self.has_target() {
            let target = self.create_target()?;
            *self.target.borrow_mut() = Some(target);
        }
        let result = {
            let guard = self.target.borrow();
            let target = guard.as_ref().unwrap();
            unsafe {
                target.rt.BeginDraw();
                target.rt.Clear(Some(&d2d(clear)));
                draw(&Painter {
                    renderer: self,
                    rt: &target.rt,
                    brush: &target.brush,
                });
                target.rt.EndDraw(None, None)
            }
        };
        if let Err(error) = &result
            && error.code().0 == RECREATE_TARGET
        {
            self.discard_target();
        }
        result
    }
    fn format(&self, key: Key) -> Option<IDWriteTextFormat> {
        if let Some(format) = self.formats.borrow().get(&key) {
            return Some(format.clone());
        }
        let family = if key.icon {
            &self.icon_font
        } else if key.size >= 190 {
            &self.display_font
        } else {
            &self.body_font
        };
        unsafe {
            let format = self
                .write
                .CreateTextFormat(
                    PCWSTR(family.as_ptr()),
                    None::<&IDWriteFontCollection>,
                    DWRITE_FONT_WEIGHT(key.weight as i32),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    key.size as f32 / 10.0,
                    windows::core::w!("en-us"),
                )
                .ok()?;
            let _ = format.SetTextAlignment(match key.align {
                0 => DWRITE_TEXT_ALIGNMENT_LEADING,
                1 => DWRITE_TEXT_ALIGNMENT_CENTER,
                _ => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            let _ = format.SetParagraphAlignment(if key.wrap {
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR
            } else {
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER
            });
            let _ = format.SetWordWrapping(if key.wrap {
                DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            });
            if !key.wrap
                && !key.icon
                && let Ok(sign) = self.write.CreateEllipsisTrimmingSign(&format)
            {
                let _ = format.SetTrimming(
                    &DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    },
                    &sign,
                );
            }
            self.formats.borrow_mut().insert(key, format.clone());
            Some(format)
        }
    }
    fn key(style: TextStyle, align: Align, wrap: bool) -> Key {
        Key {
            size: (style.size() * 10.0) as u32,
            weight: style.weight(),
            align: align as u8,
            wrap,
            icon: false,
        }
    }
    /// Natural single-line width of `text` in logical pixels.
    pub fn text_width(&self, text: &str, style: TextStyle) -> f32 {
        let Some(format) = self.format(Self::key(style, Align::Start, false)) else {
            return text.chars().count() as f32 * style.size() * 0.55;
        };
        let text: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let mut metrics = DWRITE_TEXT_METRICS::default();
            self.write
                .CreateTextLayout(&text, &format, 4096.0, 200.0)
                .ok()
                .and_then(|l| l.GetMetrics(&mut metrics).ok())
                .map_or(text.len() as f32 * style.size() * 0.55, |_| metrics.width)
        }
    }
}
/// Drawing surface handed to widgets for the duration of one frame.
pub struct Painter<'a> {
    renderer: &'a Renderer,
    rt: &'a ID2D1HwndRenderTarget,
    brush: &'a ID2D1SolidColorBrush,
}
impl Painter<'_> {
    fn set(&self, c: Color) {
        unsafe { self.brush.SetColor(&d2d(c)) };
    }
    pub fn text_width(&self, text: &str, style: TextStyle) -> f32 {
        self.renderer.text_width(text, style)
    }
    pub fn fill_rect(&self, r: Rect, c: Color) {
        self.set(c);
        unsafe { self.rt.FillRectangle(&rect_f(r), self.brush) };
    }
    pub fn fill_round(&self, r: Rect, radius: f32, c: Color) {
        self.set(c);
        let r = D2D1_ROUNDED_RECT {
            rect: rect_f(r),
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.rt.FillRoundedRectangle(&r, self.brush) };
    }
    /// Strokes inside `r`: the line is centred half a stroke inward so edges stay crisp.
    pub fn stroke_round(&self, r: Rect, radius: f32, c: Color, width: f32) {
        self.set(c);
        let r = D2D1_ROUNDED_RECT {
            rect: rect_f(r.inset(width / 2.0)),
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.rt.DrawRoundedRectangle(&r, self.brush, width, None) };
    }
    /// Horizontal or vertical hairline drawn as a thin filled rectangle.
    pub fn line(&self, from: (f32, f32), to: (f32, f32), c: Color, width: f32) {
        let half = width / 2.0;
        self.fill_rect(
            Rect::new(
                from.0.min(to.0) - half,
                from.1.min(to.1) - half,
                (to.0 - from.0).abs() + width,
                (to.1 - from.1).abs() + width,
            ),
            c,
        );
    }
    pub fn fill_circle(&self, center: (f32, f32), radius: f32, c: Color) {
        self.fill_round(
            Rect::new(
                center.0 - radius,
                center.1 - radius,
                radius * 2.0,
                radius * 2.0,
            ),
            radius,
            c,
        );
    }
    pub fn clip(&self, r: Rect) {
        unsafe {
            self.rt
                .PushAxisAlignedClip(&rect_f(r), D2D1_ANTIALIAS_MODE_ALIASED)
        };
    }
    pub fn unclip(&self) {
        unsafe { self.rt.PopAxisAlignedClip() };
    }
    fn draw(&self, r: Rect, text: &str, format: &IDWriteTextFormat, c: Color) {
        if text.is_empty() || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        self.set(c);
        let text: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.rt.DrawText(
                &text,
                format,
                &rect_f(r),
                self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }
    /// One line, vertically centred, trimmed with an ellipsis.
    pub fn text(&self, r: Rect, text: &str, style: TextStyle, c: Color, align: Align) {
        if let Some(f) = self.renderer.format(Renderer::key(style, align, false)) {
            self.draw(r, text, &f, c);
        }
    }
    /// Like `text`, with OpenType tabular figures so digits line up between rows.
    pub fn text_tabular(&self, r: Rect, text: &str, style: TextStyle, c: Color, align: Align) {
        if text.is_empty() || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let Some(f) = self.renderer.format(Renderer::key(style, align, false)) else {
            return;
        };
        let units: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let Ok(layout) = self.renderer.write.CreateTextLayout(&units, &f, r.w, r.h) else {
                return;
            };
            if let Ok(typography) = self.renderer.write.CreateTypography()
                && typography
                    .AddFontFeature(DWRITE_FONT_FEATURE {
                        nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                        parameter: 1,
                    })
                    .is_ok()
            {
                let _ = layout.SetTypography(
                    &typography,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: units.len() as u32,
                    },
                );
            }
            self.set(c);
            let origin = windows_numerics::Vector2 { X: r.x, Y: r.y };
            self.rt
                .DrawTextLayout(origin, &layout, self.brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
        }
    }
    /// Wrapped paragraph anchored at the top of `r`.
    pub fn paragraph(&self, r: Rect, text: &str, style: TextStyle, c: Color) {
        if let Some(f) = self
            .renderer
            .format(Renderer::key(style, Align::Start, true))
        {
            self.draw(r, text, &f, c);
        }
    }
    /// One glyph of the Segoe Fluent Icons (or MDL2) font centred in `r`.
    pub fn icon(&self, r: Rect, glyph: char, size: f32, c: Color) {
        let key = Key {
            size: (size * 10.0) as u32,
            weight: 400,
            align: Align::Center as u8,
            wrap: false,
            icon: true,
        };
        if let Some(f) = self.renderer.format(key) {
            self.draw(r, &glyph.to_string(), &f, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_ui_shared_renderers_recover_independent_targets_and_release_the_last_owner() {
        use windows::{Win32::UI::WindowsAndMessaging::*, core::w};
        unsafe {
            let window = || {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("Synthetic renderer"),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    320,
                    200,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
            };
            let a = window();
            let b = window();
            let first = Renderer::new(a, 96).unwrap();
            let second = Renderer::new(b, 96).unwrap();
            assert!(Rc::ptr_eq(&first.factory, &second.factory));
            let draw = |r: &Renderer| {
                r.paint(Color::rgb(0xffffff), |p| {
                    p.text(
                        Rect::new(5.0, 5.0, 200.0, 30.0),
                        "Synthetic rendering",
                        TextStyle::Body,
                        Color::rgb(0),
                        Align::Start,
                    )
                })
                .unwrap()
            };
            draw(&first);
            draw(&second);
            first.discard_target();
            assert!(!first.has_target() && second.has_target());
            draw(&first);
            first.resize(400, 240);
            draw(&second);
            let weak = Rc::downgrade(&first.factory);
            drop(first);
            draw(&second);
            assert!(weak.upgrade().is_some());
            drop(second);
            assert!(weak.upgrade().is_none());
            DestroyWindow(a).unwrap();
            DestroyWindow(b).unwrap();
        }
    }

    #[test]
    fn window_factories_share_only_within_the_ui_thread_and_release_the_last_owner() {
        let first = shared_factory().unwrap();
        let second = shared_factory().unwrap();
        assert!(Rc::ptr_eq(&first, &second));
        let address = Rc::as_ptr(&first) as usize;
        let other = std::thread::spawn(|| {
            let factory = shared_factory().unwrap();
            Rc::as_ptr(&factory) as usize
        })
        .join()
        .unwrap();
        assert_ne!(address, other);
        let weak = Rc::downgrade(&first);
        drop(first);
        assert!(weak.upgrade().is_some());
        drop(second);
        assert!(
            weak.upgrade().is_none(),
            "the thread-local cache must not pin device resources"
        );
        assert!(FACTORY.with(|slot| slot.borrow().upgrade().is_none()));
        assert!(
            shared_factory().is_ok(),
            "reopening a window creates a usable factory"
        );
    }
}
