//! Palettes and type roles. Every colour used by the UI comes from `Theme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl Color {
    pub const fn rgb(value: u32) -> Self {
        Self {
            r: (value >> 16) as u8,
            g: (value >> 8) as u8,
            b: value as u8,
            a: 255,
        }
    }
    /// Linear mix in sRGB space; `t` of 0 gives `self`, 1 gives `other`.
    pub fn blend(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
            a: mix(self.a, other.a),
        }
    }
    #[cfg(test)]
    pub fn luminance(self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }
    /// WCAG contrast ratio between two colours, 1.0 to 21.0.
    pub fn contrast(self, other: Self) -> f32 {
        fn channel(v: u8) -> f32 {
            let c = v as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        let l = |c: Self| 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
        let (a, b) = (l(self), l(other));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    /// Black or white, whichever reads better on this colour.
    pub fn readable_on(self) -> Self {
        let (black, white) = (Self::rgb(0x000000), Self::rgb(0xffffff));
        if self.contrast(black) >= self.contrast(white) {
            black
        } else {
            white
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
    HighContrast,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Good,
    Warning,
    Bad,
    Development,
}
/// System colours supplied by Windows in high-contrast mode.
#[derive(Clone, Copy, Debug)]
pub struct HighContrastColors {
    pub window: Color,
    pub text: Color,
    pub hotlight: Color,
    pub gray_text: Color,
    pub highlight: Color,
    pub highlight_text: Color,
    pub face: Color,
    pub face_text: Color,
}
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub mode: Mode,
    pub development: bool,
    pub background: Color,
    pub rail: Color,
    pub card: Color,
    pub card_border: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,
    pub accent_text: Color,
    pub control: Color,
    pub control_hover: Color,
    pub control_pressed: Color,
    pub control_border: Color,
    pub control_disabled: Color,
    pub nav_hover: Color,
    pub nav_selected: Color,
    /// Text on `nav_selected`; differs from `text` only in high contrast.
    pub on_selected: Color,
    pub track: Color,
    pub focus: Color,
    pub good: Color,
    pub warning: Color,
    pub bad: Color,
    pub development_red: Color,
}
const DEV_LIGHT: Color = Color::rgb(0xc42b1c);
const DEV_DARK: Color = Color::rgb(0xff6a5c);
pub const DEFAULT_ACCENT_LIGHT: Color = Color::rgb(0x0067c0);
pub const DEFAULT_ACCENT_DARK: Color = Color::rgb(0x4cc2ff);
impl Theme {
    /// `accent` is the Windows accent colour when known; development mode replaces it with red.
    pub fn new(mode: Mode, accent: Option<Color>, development: bool) -> Self {
        match mode {
            Mode::HighContrast => Self::new(Mode::Light, accent, development),
            Mode::Light => Self::build(false, accent.unwrap_or(DEFAULT_ACCENT_LIGHT), development),
            Mode::Dark => Self::build(true, accent.unwrap_or(DEFAULT_ACCENT_DARK), development),
        }
    }
    fn build(dark: bool, accent: Color, development: bool) -> Self {
        let dev = if dark { DEV_DARK } else { DEV_LIGHT };
        let accent = if development { dev } else { accent };
        let c = Color::rgb;
        let (text, white, black) = if dark {
            (c(0xffffff), c(0xffffff), c(0x000000))
        } else {
            (c(0x1a1a1a), c(0xffffff), c(0x000000))
        };
        let toward = if dark { white } else { black };
        Self {
            mode: if dark { Mode::Dark } else { Mode::Light },
            development,
            background: if dark { c(0x202020) } else { c(0xf3f3f3) },
            rail: if dark { c(0x1b1b1b) } else { c(0xeaeaea) },
            card: if dark { c(0x2b2b2b) } else { c(0xffffff) },
            card_border: if dark { c(0x3a3a3a) } else { c(0xe0e0e0) },
            text,
            text_secondary: if dark { c(0xc8c8c8) } else { c(0x5c5c5c) },
            text_disabled: if dark { c(0x7a7a7a) } else { c(0xa0a0a0) },
            accent,
            accent_hover: accent.blend(toward, 0.12),
            accent_pressed: accent.blend(toward, 0.25),
            accent_text: accent.readable_on(),
            control: if dark { c(0x333333) } else { c(0xfbfbfb) },
            control_hover: if dark { c(0x3d3d3d) } else { c(0xf3f3f3) },
            control_pressed: if dark { c(0x2a2a2a) } else { c(0xe9e9e9) },
            control_border: if dark { c(0x454545) } else { c(0xd5d5d5) },
            control_disabled: if dark { c(0x2a2a2a) } else { c(0xf0f0f0) },
            nav_hover: if dark { c(0x2a2a2a) } else { c(0xe0e0e0) },
            nav_selected: if dark { c(0x383838) } else { c(0xdadada) },
            on_selected: text,
            track: if dark { c(0x4a4a4a) } else { c(0xd8d8d8) },
            focus: if dark { white } else { black },
            good: if dark { c(0x6ccb5f) } else { c(0x0f7b0f) },
            warning: if dark { c(0xfce100) } else { c(0x9d5d00) },
            bad: if dark { c(0xff99a4) } else { c(0xc42b1c) },
            development_red: dev,
        }
    }
    /// High contrast uses the system palette only, with no custom accent.
    pub fn high_contrast(colors: HighContrastColors, development: bool) -> Self {
        Self {
            mode: Mode::HighContrast,
            development,
            background: colors.window,
            rail: colors.window,
            card: colors.window,
            card_border: colors.text,
            text: colors.text,
            text_secondary: colors.text,
            text_disabled: colors.gray_text,
            accent: colors.highlight,
            accent_hover: colors.hotlight,
            accent_pressed: colors.hotlight,
            accent_text: colors.highlight_text,
            control: colors.face,
            control_hover: colors.window,
            control_pressed: colors.window,
            control_border: colors.face_text,
            control_disabled: colors.face,
            nav_hover: colors.window,
            nav_selected: colors.highlight,
            on_selected: colors.highlight_text,
            track: colors.gray_text,
            focus: colors.hotlight,
            good: colors.text,
            warning: colors.text,
            bad: colors.text,
            development_red: colors.highlight,
        }
    }
    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }
    pub fn high_contrast_mode(&self) -> bool {
        self.mode == Mode::HighContrast
    }
    /// Fill and text colours for a status pill.
    pub fn tone(&self, tone: Tone) -> (Color, Color) {
        let base = match tone {
            Tone::Neutral => return (self.control_hover.blend(self.text, 0.08), self.text),
            Tone::Good => self.good,
            Tone::Warning => self.warning,
            Tone::Bad => self.bad,
            Tone::Development => self.development_red,
        };
        if self.high_contrast_mode() {
            return (self.control, self.text);
        }
        (self.card.blend(base, 0.18), base)
    }
}
/// Type roles from the plan: sizes are logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextStyle {
    Display,
    Title,
    Body,
    BodyStrong,
    Caption,
}
impl TextStyle {
    pub fn size(self) -> f32 {
        match self {
            Self::Display => 28.0,
            Self::Title => 20.0,
            Self::Body | Self::BodyStrong => 14.0,
            Self::Caption => 12.0,
        }
    }
    pub fn weight(self) -> u16 {
        match self {
            Self::Display | Self::Title | Self::BodyStrong => 600,
            _ => 400,
        }
    }
}
pub const RADIUS_CARD: f32 = 8.0;
pub const RADIUS_CONTROL: f32 = 4.0;

/// Development builds only: `DESKSTEPS_DEV_THEME=dark` or `light` overrides the Windows
/// app theme so both palettes can be checked without changing system settings.
pub fn forced_mode(value: &str) -> Option<Mode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "dark" => Some(Mode::Dark),
        "light" => Some(Mode::Light),
        _ => None,
    }
}
/// Reads the Windows app theme, accent colour and high-contrast state.
pub fn detect(development: bool, preference: crate::appearance::ThemePreference) -> Theme {
    use windows::Win32::{
        Graphics::Gdi::*,
        UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW},
    };
    unsafe {
        let mut hc = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        if SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some((&mut hc as *mut HIGHCONTRASTW).cast()),
            Default::default(),
        )
        .is_ok()
            && hc.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
        {
            let sys = |index| {
                let v = GetSysColor(index);
                Color::rgb(((v & 0xff) << 16) | (v & 0xff00) | ((v >> 16) & 0xff))
            };
            return Theme::high_contrast(
                HighContrastColors {
                    window: sys(COLOR_WINDOW),
                    text: sys(COLOR_WINDOWTEXT),
                    hotlight: sys(COLOR_HOTLIGHT),
                    gray_text: sys(COLOR_GRAYTEXT),
                    highlight: sys(COLOR_HIGHLIGHT),
                    highlight_text: sys(COLOR_HIGHLIGHTTEXT),
                    face: sys(COLOR_BTNFACE),
                    face_text: sys(COLOR_BTNTEXT),
                },
                development,
            );
        }
    }
    let forced = development
        .then(|| std::env::var("DESKSTEPS_DEV_THEME").ok())
        .flatten()
        .and_then(|v| forced_mode(&v));
    Theme::new(
        preferred_mode(
            preference,
            forced.unwrap_or(if apps_use_light_theme() {
                Mode::Light
            } else {
                Mode::Dark
            }),
        ),
        accent_color(),
        development,
    )
}
fn preferred_mode(preference: crate::appearance::ThemePreference, automatic: Mode) -> Mode {
    use crate::appearance::ThemePreference;
    match preference {
        ThemePreference::Automatic => automatic,
        ThemePreference::Light => Mode::Light,
        ThemePreference::Dark => Mode::Dark,
    }
}
fn apps_use_light_theme() -> bool {
    use windows::{
        Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        core::w,
    };
    let mut value = 1u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    status.0 != 0 || value != 0
}
fn accent_color() -> Option<Color> {
    use windows::core::BOOL;
    let mut argb = 0u32;
    let mut opaque = BOOL(0);
    unsafe {
        windows::Win32::Graphics::Dwm::DwmGetColorizationColor(&mut argb, &mut opaque).ok()?;
    }
    Some(Color::rgb(argb & 0x00ff_ffff))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colors_blend_and_choose_readable_text() {
        let a = Color::rgb(0x000000);
        let b = Color::rgb(0xffffff);
        assert_eq!(a.blend(b, 0.5), Color::rgb(0x808080));
        assert_eq!(a.blend(b, 2.0), b);
        assert_eq!(a.blend(b, -1.0), a);
        assert_eq!(Color::rgb(0xffff00).readable_on(), Color::rgb(0));
        assert_eq!(Color::rgb(0x0000aa).readable_on(), Color::rgb(0xffffff));
    }
    #[test]
    fn light_and_dark_differ_and_follow_the_accent() {
        let accent = Color::rgb(0x123456);
        let light = Theme::new(Mode::Light, Some(accent), false);
        let dark = Theme::new(Mode::Dark, Some(accent), false);
        assert!(light.background.luminance() > dark.background.luminance());
        assert!(light.text.luminance() < dark.text.luminance());
        assert_eq!(light.accent, accent);
        assert_eq!(dark.accent, accent);
        assert!(dark.is_dark() && !light.is_dark());
    }
    #[test]
    fn development_replaces_the_accent_with_red_in_both_modes() {
        for mode in [Mode::Light, Mode::Dark] {
            let t = Theme::new(mode, Some(Color::rgb(0x00aa00)), true);
            assert!(t.development);
            assert!(t.accent.r > t.accent.g && t.accent.r > t.accent.b);
            assert_eq!(t.accent, t.development_red);
        }
    }
    #[test]
    fn high_contrast_uses_only_system_colours() {
        let sys = HighContrastColors {
            window: Color::rgb(0x000000),
            text: Color::rgb(0xffffff),
            hotlight: Color::rgb(0xffff00),
            gray_text: Color::rgb(0x3fff23),
            highlight: Color::rgb(0x1aebff),
            highlight_text: Color::rgb(0x000000),
            face: Color::rgb(0x000000),
            face_text: Color::rgb(0xffffff),
        };
        let t = Theme::high_contrast(sys, true);
        assert!(t.high_contrast_mode());
        assert_eq!(t.accent, sys.highlight);
        assert_eq!(t.background, sys.window);
        assert_eq!(t.tone(Tone::Good).1, sys.text);
        // Even development mode keeps the system palette.
        assert_eq!(t.development_red, sys.highlight);
        assert_eq!(
            Theme::new(Mode::HighContrast, None, false).mode,
            Mode::Light
        );
    }
    #[test]
    fn tones_have_distinct_text_colours() {
        let t = Theme::new(Mode::Light, None, false);
        let colours: Vec<_> = [Tone::Good, Tone::Warning, Tone::Bad, Tone::Development]
            .iter()
            .map(|x| t.tone(*x).1)
            .collect();
        assert_eq!(colours[0], t.good);
        assert_ne!(colours[0], colours[1]);
        assert_eq!(t.tone(Tone::Neutral).1, t.text);
    }
    #[test]
    fn contrast_ratio_matches_wcag_reference_values() {
        let black = Color::rgb(0);
        let white = Color::rgb(0xffffff);
        assert!((black.contrast(white) - 21.0).abs() < 0.01);
        assert!((white.contrast(white) - 1.0).abs() < 1e-4);
        assert!((Color::rgb(0x767676).contrast(white) - 4.54).abs() < 0.05);
    }
    #[test]
    fn forced_theme_names_are_recognised() {
        assert_eq!(forced_mode(" Dark "), Some(Mode::Dark));
        assert_eq!(forced_mode("LIGHT"), Some(Mode::Light));
        assert_eq!(forced_mode("sepia"), None);
        assert_eq!(forced_mode(""), None);
    }
    #[test]
    fn explicit_preference_survives_system_theme_changes() {
        use crate::appearance::ThemePreference;
        for system in [Mode::Light, Mode::Dark] {
            assert_eq!(preferred_mode(ThemePreference::Automatic, system), system);
            assert_eq!(preferred_mode(ThemePreference::Light, system), Mode::Light);
            assert_eq!(preferred_mode(ThemePreference::Dark, system), Mode::Dark);
        }
    }
    fn palettes() -> Vec<(String, Theme)> {
        let sys = HighContrastColors {
            window: Color::rgb(0),
            text: Color::rgb(0xffffff),
            hotlight: Color::rgb(0xffff00),
            gray_text: Color::rgb(0x3fff23),
            highlight: Color::rgb(0x1aebff),
            highlight_text: Color::rgb(0),
            face: Color::rgb(0),
            face_text: Color::rgb(0xffffff),
        };
        let mut all = vec![(
            "high contrast".to_string(),
            Theme::high_contrast(sys, false),
        )];
        for (mode, name) in [(Mode::Light, "light"), (Mode::Dark, "dark")] {
            for dev in [false, true] {
                for accent in [
                    None,
                    Some(Color::rgb(0x00a000)),
                    Some(Color::rgb(0xffd000)),
                    Some(Color::rgb(0x202080)),
                ] {
                    all.push((
                        format!("{name} dev={dev} accent={accent:?}"),
                        Theme::new(mode, accent, dev),
                    ));
                }
            }
        }
        all
    }
    #[test]
    fn text_is_readable_on_every_surface_in_every_palette() {
        for (name, t) in palettes() {
            for (text, surface, label) in [
                (t.text, t.background, "text/background"),
                (t.text, t.rail, "text/rail"),
                (t.text, t.card, "text/card"),
                (t.text, t.control, "text/control"),
                (t.text, t.control_hover, "text/control hover"),
                (t.text, t.nav_hover, "text/nav hover"),
                (t.on_selected, t.nav_selected, "text/nav selected"),
                (t.text_secondary, t.background, "secondary/background"),
                (t.text_secondary, t.rail, "secondary/rail"),
                (t.text_secondary, t.card, "secondary/card"),
                (t.bad, t.background, "error/background"),
                (t.bad, t.card, "error/card"),
                (t.accent_text, t.accent, "on accent"),
                (t.accent_text, t.accent_hover, "on accent hover"),
                (t.accent_text, t.accent_pressed, "on accent pressed"),
            ] {
                let ratio = text.contrast(surface);
                // Custom accents the user might pick may only reach 3:1 for large bold labels.
                let need = if label.starts_with("on accent ") {
                    3.0
                } else {
                    4.5
                };
                assert!(ratio >= need, "{name}: {label} {ratio:.2}");
            }
            for tone in [
                Tone::Neutral,
                Tone::Good,
                Tone::Warning,
                Tone::Bad,
                Tone::Development,
            ] {
                let (fill, _) = t.tone(tone);
                assert!(
                    t.text.contrast(fill) >= 4.5,
                    "{name}: pill text on {tone:?} fill"
                );
            }
            // Shapes that carry meaning need 3:1 against what they sit on.
            assert!(t.focus.contrast(t.background) >= 3.0, "{name}: focus ring");
            assert!(
                t.focus.contrast(t.card) >= 3.0,
                "{name}: focus ring on card"
            );
            assert!(
                t.track.contrast(t.card) >= 1.3,
                "{name}: progress track visible"
            );
            assert!(
                t.card_border.contrast(t.background) >= 1.05 || t.high_contrast_mode(),
                "{name}: card edge"
            );
        }
    }
    #[test]
    fn disabled_text_stays_distinguishable_from_enabled_text() {
        for (name, t) in palettes() {
            assert!(
                t.text.contrast(t.control_disabled) > t.text_disabled.contrast(t.control_disabled),
                "{name}"
            );
            assert!(
                t.text_disabled.contrast(t.control_disabled) >= 1.4,
                "{name}: disabled label visible"
            );
        }
    }
    #[test]
    fn type_roles_match_plan() {
        assert_eq!(
            (TextStyle::Display.size(), TextStyle::Display.weight()),
            (28.0, 600)
        );
        assert_eq!(
            (TextStyle::Title.size(), TextStyle::Title.weight()),
            (20.0, 600)
        );
        assert_eq!(
            (TextStyle::Body.size(), TextStyle::Body.weight()),
            (14.0, 400)
        );
        assert_eq!(TextStyle::BodyStrong.weight(), 600);
        assert_eq!(
            (TextStyle::Caption.size(), TextStyle::Caption.weight()),
            (12.0, 400)
        );
    }
}
