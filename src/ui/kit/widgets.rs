//! Stateless widget drawing. Widgets read their appearance from `Theme` and `State`.
use super::{
    input::{Id, Input, Targets, Visual},
    layout::Rect,
    render::{Align, Painter},
    theme::{Color, RADIUS_CARD, RADIUS_CONTROL, TextStyle, Theme, Tone},
};

/// Segoe Fluent Icons code points (the same values exist in Segoe MDL2 Assets).
pub mod icons {
    pub const TODAY: char = '\u{E80F}';
    pub const HISTORY: char = '\u{E81C}';
    pub const TARGETS: char = '\u{E7C1}';
    pub const SETTINGS: char = '\u{E713}';
    pub const PAUSE: char = '\u{E769}';
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub visual: Visual,
    pub focused: bool,
}
impl State {
    pub fn of(input: &Input, targets: &Targets, id: Id) -> Self {
        Self {
            visual: input.visual(targets, id),
            focused: input.focused(id),
        }
    }
    pub fn enabled(&self) -> bool {
        self.visual != Visual::Disabled
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Standard,
    Accent,
    Subtle,
    /// The chosen option of a group: tinted fill and an accent outline, not a primary button.
    Selected,
}
pub fn focus_ring(p: &Painter, th: &Theme, r: Rect, radius: f32) {
    p.stroke_round(r.inset(-2.0), radius + 2.0, th.focus, 2.0);
}
pub fn card(p: &Painter, th: &Theme, r: Rect) {
    p.fill_round(r, RADIUS_CARD, th.card);
    p.stroke_round(r, RADIUS_CARD, th.card_border, 1.0);
}
fn button_colors(th: &Theme, kind: ButtonKind, s: State) -> (Option<Color>, Option<Color>, Color) {
    let (fill, border, text) = match (kind, s.visual) {
        (_, Visual::Disabled) => (
            (kind != ButtonKind::Subtle).then_some(th.control_disabled),
            None,
            th.text_disabled,
        ),
        (ButtonKind::Accent, Visual::Normal) => (Some(th.accent), None, th.accent_text),
        (ButtonKind::Accent, Visual::Hover) => (Some(th.accent_hover), None, th.accent_text),
        (ButtonKind::Accent, Visual::Pressed) => (Some(th.accent_pressed), None, th.accent_text),
        (ButtonKind::Standard, Visual::Normal) => {
            (Some(th.control), Some(th.control_border), th.text)
        }
        (ButtonKind::Standard, Visual::Hover) => {
            (Some(th.control_hover), Some(th.control_border), th.text)
        }
        (ButtonKind::Standard, Visual::Pressed) => (
            Some(th.control_pressed),
            Some(th.control_border),
            th.text_secondary,
        ),
        (ButtonKind::Selected, Visual::Pressed) => (Some(th.nav_hover), Some(th.accent), th.text),
        (ButtonKind::Selected, _) => (Some(th.nav_selected), Some(th.accent), th.on_selected),
        (ButtonKind::Subtle, Visual::Normal) => (None, None, th.text),
        (ButtonKind::Subtle, Visual::Hover) => (Some(th.nav_hover), None, th.text),
        (ButtonKind::Subtle, Visual::Pressed) => (Some(th.nav_selected), None, th.text_secondary),
    };
    (fill, border, text)
}
fn button_frame(p: &Painter, th: &Theme, r: Rect, kind: ButtonKind, s: State) -> Color {
    let (fill, border, text) = button_colors(th, kind, s);
    if let Some(fill) = fill {
        p.fill_round(r, RADIUS_CONTROL, fill);
    }
    if let Some(border) = border {
        p.stroke_round(r, RADIUS_CONTROL, border, 1.0);
    }
    if s.focused {
        focus_ring(p, th, r, RADIUS_CONTROL);
    }
    text
}
pub fn button(p: &Painter, th: &Theme, r: Rect, label: &str, kind: ButtonKind, s: State) {
    let text = button_frame(p, th, r, kind, s);
    let style = if kind == ButtonKind::Accent {
        TextStyle::BodyStrong
    } else {
        TextStyle::Body
    };
    p.text(r.inset_xy(2.0, 0.0), label, style, text, Align::Center);
}
pub fn icon_button(p: &Painter, th: &Theme, r: Rect, glyph: char, kind: ButtonKind, s: State) {
    let text = button_frame(p, th, r, kind, s);
    p.icon(r, glyph, 16.0, text);
}
pub struct NavEntry<'a> {
    pub glyph: char,
    pub label: &'a str,
    pub selected: bool,
    pub collapsed: bool,
}
/// Navigation rail entry; collapsed entries show only the icon.
pub fn nav_item(p: &Painter, th: &Theme, r: Rect, entry: &NavEntry, s: State) {
    let NavEntry {
        glyph,
        label,
        selected,
        collapsed,
    } = *entry;
    let fill = if selected {
        Some(th.nav_selected)
    } else if s.visual == Visual::Hover || s.visual == Visual::Pressed {
        Some(th.nav_hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        p.fill_round(r, RADIUS_CONTROL, fill);
    }
    if selected {
        p.fill_round(
            Rect::new(r.x, r.y + (r.h - 16.0) / 2.0, 3.0, 16.0),
            1.5,
            th.accent,
        );
    }
    let color = if !s.enabled() {
        th.text_disabled
    } else if selected {
        th.on_selected
    } else {
        th.text
    };
    if collapsed {
        p.icon(r, glyph, 18.0, color);
    } else {
        p.icon(Rect::new(r.x + 4.0, r.y, 36.0, r.h), glyph, 16.0, color);
        p.text(
            Rect::new(r.x + 44.0, r.y, r.w - 48.0, r.h),
            label,
            if selected {
                TextStyle::BodyStrong
            } else {
                TextStyle::Body
            },
            color,
            Align::Start,
        );
    }
    if s.focused {
        focus_ring(p, th, r, RADIUS_CONTROL);
    }
}
pub fn progress_bar(p: &Painter, th: &Theme, r: Rect, fraction: f32, color: Color) {
    let radius = r.h / 2.0;
    p.fill_round(r, radius, th.track);
    let f = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if f > 0.0 {
        p.fill_round(Rect::new(r.x, r.y, (r.w * f).max(r.h), r.h), radius, color);
    }
}
pub fn pill_width(p: &Painter, text: &str) -> f32 {
    p.text_width(text, TextStyle::Caption) + 26.0
}
pub fn pill(p: &Painter, th: &Theme, r: Rect, text: &str, tone: Tone) {
    let (fill, color) = th.tone(tone);
    p.fill_round(r, r.h / 2.0, fill);
    if th.high_contrast_mode() {
        p.stroke_round(r, r.h / 2.0, th.card_border, 1.0);
    }
    p.fill_circle((r.x + 10.0, r.y + r.h / 2.0), 3.0, color);
    p.text(
        Rect::new(r.x + 19.0, r.y, r.w - 23.0, r.h),
        text,
        TextStyle::Caption,
        th.text,
        Align::Start,
    );
}
pub struct TileText<'a> {
    pub title: &'a str,
    pub value: &'a str,
    /// Target line, then remaining line, including in compact tiles.
    pub lines: [&'a str; 2],
    pub progress: Option<f32>,
}
/// Summary tile. Compact tiles are one row (title, value, detail) with the bar underneath.
pub fn tile(p: &Painter, th: &Theme, r: Rect, t: &TileText, compact: bool) {
    card(p, th, r);
    let inner = r.inset_xy(12.0, 0.0);
    if compact {
        p.text(
            Rect::new(inner.x, r.y + 6.0, 106.0, 30.0),
            t.title,
            TextStyle::Caption,
            th.text_secondary,
            Align::Start,
        );
        p.text(
            Rect::new(inner.x + 106.0, r.y + 6.0, 62.0, 30.0),
            t.value,
            TextStyle::Title,
            th.text,
            Align::Start,
        );
        let detail = Rect::new(inner.x + 168.0, r.y + 6.0, (inner.w - 168.0).max(0.0), 30.0);
        if t.lines[1].is_empty() {
            p.text(
                detail,
                t.lines[0],
                TextStyle::Caption,
                th.text_secondary,
                Align::End,
            );
        } else {
            for (i, line) in t.lines.iter().enumerate() {
                p.text(
                    Rect::new(detail.x, r.y + 7.0 + i as f32 * 16.0, detail.w, 16.0),
                    line,
                    TextStyle::Caption,
                    th.text_secondary,
                    Align::End,
                );
            }
        }
        if let Some(f) = t.progress {
            progress_bar(
                p,
                th,
                Rect::new(inner.x, r.y + 42.0, inner.w, 6.0),
                f,
                th.accent,
            );
        }
        return;
    }
    p.text(
        Rect::new(inner.x, r.y + 10.0, inner.w, 16.0),
        t.title,
        TextStyle::Caption,
        th.text_secondary,
        Align::Start,
    );
    p.text(
        Rect::new(inner.x, r.y + 28.0, inner.w, 38.0),
        t.value,
        TextStyle::Display,
        th.text,
        Align::Start,
    );
    for (i, line) in t.lines.iter().enumerate() {
        p.text(
            Rect::new(inner.x, r.y + 68.0 + 16.0 * i as f32, inner.w, 16.0),
            line,
            TextStyle::Caption,
            th.text_secondary,
            Align::Start,
        );
    }
    if let Some(f) = t.progress {
        progress_bar(
            p,
            th,
            Rect::new(inner.x, r.bottom() - 16.0, inner.w, 6.0),
            f,
            th.accent,
        );
    }
}
/// Label on the left, value on the right, hairline below.
pub fn list_row(p: &Painter, th: &Theme, r: Rect, label: &str, value: &str, divider: bool) {
    let (left, right) = r
        .inset_xy(12.0, 0.0)
        .split_right((r.w * 0.55).min(r.w - 24.0));
    p.text(left, label, TextStyle::Body, th.text, Align::Start);
    p.text(right, value, TextStyle::Body, th.text_secondary, Align::End);
    if divider {
        p.line(
            (r.x + 12.0, r.bottom() - 0.5),
            (r.right() - 12.0, r.bottom() - 0.5),
            th.card_border,
            1.0,
        );
    }
}
pub fn section_title(p: &Painter, th: &Theme, r: Rect, text: &str) {
    p.text(r, text, TextStyle::BodyStrong, th.text, Align::Start);
}
/// Thin overlay scrollbar; draws nothing when the content fits.
pub fn scrollbar(p: &Painter, th: &Theme, view: Rect, content: f32, offset: f32) {
    if content <= view.h + 0.5 {
        return;
    }
    let ratio = view.h / content;
    let thumb = (view.h * ratio).max(24.0).min(view.h);
    let travel = view.h - thumb;
    let y = view.y + travel * (offset / (content - view.h).max(1.0)).clamp(0.0, 1.0);
    p.fill_round(
        Rect::new(view.right() + 10.0, y, 3.0, thumb),
        1.5,
        th.text_disabled,
    );
}
/// Tooltip box for `anchor`, kept inside `bounds`; wraps text wider than 280.
pub fn tooltip(p: &Painter, th: &Theme, anchor: Rect, text: &str, bounds: Rect) {
    let natural = p.text_width(text, TextStyle::Caption);
    let w = (natural + 18.0).min(280.0).min(bounds.w - 8.0);
    let lines = (natural / (w - 18.0).max(1.0)).ceil().max(1.0);
    let h = lines * 16.0 + 10.0;
    let x = (anchor.x + anchor.w / 2.0 - w / 2.0)
        .clamp(bounds.x + 4.0, (bounds.right() - w - 4.0).max(bounds.x));
    let below = anchor.bottom() + 6.0;
    let y = if below + h <= bounds.bottom() {
        below
    } else {
        (anchor.y - h - 6.0).max(bounds.y)
    };
    let r = Rect::new(x, y, w, h);
    p.fill_round(r, RADIUS_CONTROL, th.card);
    p.stroke_round(r, RADIUS_CONTROL, th.control_border, 1.0);
    p.paragraph(r.inset_xy(9.0, 5.0), text, TextStyle::Caption, th.text);
}

/// Toggle switch with its label to the right: track 40 x 20, knob slides, accent when on.
pub fn toggle(p: &Painter, th: &Theme, r: Rect, label: &str, on: bool, s: State) {
    let track = Rect::new(r.x, r.y + (r.h - 20.0) / 2.0, 40.0, 20.0);
    let enabled = s.enabled();
    let (fill, border, knob) = match (on, enabled) {
        (true, true) => (Some(th.accent), th.accent, th.accent_text),
        (true, false) => (
            Some(th.control_disabled),
            th.text_disabled,
            th.text_disabled,
        ),
        (false, true) => (None, th.text_secondary, th.text_secondary),
        (false, false) => (None, th.text_disabled, th.text_disabled),
    };
    if let Some(fill) = fill {
        p.fill_round(track, 10.0, fill);
    }
    p.stroke_round(track, 10.0, border, 1.0);
    let x = if on {
        track.right() - 10.0
    } else {
        track.x + 10.0
    };
    p.fill_circle((x, track.y + 10.0), 5.0, knob);
    if s.visual == Visual::Hover || s.visual == Visual::Pressed {
        p.stroke_round(track.inset(-2.0), 12.0, th.card_border, 1.0);
    }
    p.text(
        Rect::new(track.right() + 10.0, r.y, (r.w - 50.0).max(0.0), r.h),
        label,
        TextStyle::Body,
        if enabled { th.text } else { th.text_disabled },
        Align::Start,
    );
    if s.focused {
        focus_ring(p, th, r, RADIUS_CONTROL);
    }
}
/// A selectable list row: label on the left, a check mark on the right for the chosen one.
pub fn choice_row(
    p: &Painter,
    th: &Theme,
    r: Rect,
    label: &str,
    chosen: bool,
    divider: bool,
    s: State,
) {
    if s.visual == Visual::Hover || s.visual == Visual::Pressed {
        p.fill_rect(r.inset_xy(1.0, 0.0), th.nav_hover);
    }
    let color = if s.enabled() {
        th.text
    } else {
        th.text_disabled
    };
    p.text(
        Rect::new(r.x + 12.0, r.y, (r.w - 52.0).max(0.0), r.h),
        label,
        if chosen {
            TextStyle::BodyStrong
        } else {
            TextStyle::Body
        },
        color,
        Align::Start,
    );
    if chosen {
        p.icon(
            Rect::new(r.right() - 36.0, r.y, 24.0, r.h),
            '\u{E73E}',
            16.0,
            if s.enabled() {
                th.accent
            } else {
                th.text_disabled
            },
        );
    }
    if divider {
        p.line(
            (r.x + 12.0, r.bottom() - 0.5),
            (r.right() - 12.0, r.bottom() - 0.5),
            th.card_border,
            1.0,
        );
    }
    if s.focused {
        focus_ring(p, th, r.inset(2.0), 2.0);
    }
}
