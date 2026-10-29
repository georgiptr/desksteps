//! Draws a `Scene` with the kit widgets.
use super::{
    frame::{self, Frame},
    pages::{self, Block, Field, Screen},
    scene::Scene,
};
use crate::ui::{
    kit::{
        chart,
        input::{Id, Input, Visual},
        layout::Rect,
        render::{Align, Painter},
        theme::{TextStyle, Theme, Tone},
        widgets::{self, ButtonKind, State, TileText, icons},
    },
    model::{
        day::{self, BarKind, DayView, SessionRow, Timeline},
        page::Page,
    },
};

fn icon(page: Page) -> char {
    match page {
        Page::Today => icons::TODAY,
        Page::History => icons::HISTORY,
        Page::Targets => icons::TARGETS,
        Page::Settings => icons::SETTINGS,
    }
}
pub fn title(scene: &Scene, page: Page, screen: Screen) -> String {
    match (screen, &scene.data.day) {
        (Screen::Today | Screen::HistoryDay, Some(day)) => day.title.clone(),
        (Screen::Today, None) => "Today".into(),
        _ => page.label().into(),
    }
}
pub fn draw(
    p: &Painter,
    th: &Theme,
    scene: &Scene,
    input: &Input,
    page: Page,
    screen: Screen,
    tip: Option<Id>,
) {
    let f = &scene.frame;
    let state = |id| State::of(input, &scene.targets, id);
    // Rail.
    p.fill_rect(f.rail, th.rail);
    p.line(
        (f.rail.right() - 0.5, 0.0),
        (f.rail.right() - 0.5, f.size.h),
        th.card_border,
        1.0,
    );
    if th.development {
        p.fill_rect(Rect::new(0.0, 0.0, f.size.w, 3.0), th.development_red);
    }
    if let Some(badge) = f.badge {
        p.fill_round(badge, badge.h / 2.0, th.development_red);
        p.text(
            badge,
            if f.collapsed {
                "DEV"
            } else {
                "Development — Offline"
            },
            TextStyle::Caption,
            th.development_red.readable_on(),
            Align::Center,
        );
    }
    live_card(p, th, scene, &state);
    for page_item in Page::ALL {
        let id = frame::nav_id(page_item);
        widgets::nav_item(
            p,
            th,
            f.nav[page_item.index()],
            &widgets::NavEntry {
                glyph: icon(page_item),
                label: page_item.label(),
                selected: page_item == page,
                collapsed: f.collapsed,
            },
            state(id),
        );
    }
    footer(p, th, scene);
    // Content.
    p.text(
        f.title,
        &title(scene, page, screen),
        TextStyle::Title,
        th.text,
        Align::Start,
    );
    if f.collapsed {
        widgets::button(
            p,
            th,
            f.minus,
            &scene.card.decrease,
            ButtonKind::Standard,
            state(frame::MINUS),
        );
        widgets::button(
            p,
            th,
            f.plus,
            &scene.card.increase,
            ButtonKind::Standard,
            state(frame::PLUS),
        );
        widgets::button(
            p,
            th,
            f.start,
            "Start",
            ButtonKind::Accent,
            state(frame::START),
        );
    }
    p.clip(f.body);
    for (rect, block) in &scene.blocks {
        let r = Scene::to_window(f, scene.scroll, *rect);
        if r.bottom() < f.body.y || r.y > f.body.bottom() {
            continue;
        }
        draw_block(p, th, scene, r, block, input, &state);
    }
    p.unclip();
    widgets::scrollbar(p, th, f.body, scene.content_height, scene.scroll);
    if let Some(id) = tip
        && let Some(rect) = scene.targets.rect(id)
        && let Some(text) = scene.tooltip(id)
    {
        widgets::tooltip(p, th, rect, &text, f.size);
    }
}
fn badge(kind: BarKind) -> (&'static str, Tone) {
    match kind {
        BarKind::Observed => ("Observed", Tone::Good),
        BarKind::Partial => ("Partial / gap", Tone::Warning),
        BarKind::Empty => ("No data", Tone::Neutral),
    }
}
fn cell(p: &Painter, th: &Theme, r: Rect, col: &pages::Col, text: &str) {
    let rect = Rect::new(r.x + col.x, r.y, col.w - 6.0, r.h);
    p.text(
        rect,
        text,
        TextStyle::Body,
        th.text,
        if col.right { Align::End } else { Align::Start },
    );
}
fn head(p: &Painter, th: &Theme, r: Rect, cols: &[pages::Col]) {
    for col in cols {
        let rect = Rect::new(r.x + col.x, r.y, col.w - 6.0, r.h);
        p.text(
            rect,
            col.field.title(),
            TextStyle::Caption,
            th.text_secondary,
            if col.right { Align::End } else { Align::Start },
        );
    }
    divider(p, th, r, true);
}
fn coverage(p: &Painter, th: &Theme, r: Rect, col: &pages::Col, kind: BarKind) {
    let (text, tone) = badge(kind);
    let w = widgets::pill_width(p, text).min(col.w - 6.0);
    widgets::pill(
        p,
        th,
        Rect::new(r.x + col.x, r.y + (r.h - 22.0) / 2.0, w, 22.0),
        text,
        tone,
    );
}
fn divider(p: &Painter, th: &Theme, r: Rect, on: bool) {
    if on {
        p.line(
            (r.x + 12.0, r.bottom() - 0.5),
            (r.right() - 12.0, r.bottom() - 0.5),
            th.card_border,
            1.0,
        );
    }
}
fn timeline(p: &Painter, th: &Theme, r: Rect, t: &Timeline) {
    let track = pages::timeline_track(r);
    p.fill_round(track, 4.0, th.track);
    let warning = th.tone(Tone::Warning).1;
    for bar in &t.bars {
        let x = track.x + bar.start * track.w;
        let w = ((bar.end - bar.start) * track.w).max(4.0);
        let rect = Rect::new(x.min(track.right() - w), track.y + 3.0, w, track.h - 6.0);
        p.fill_round(rect, 3.0, if bar.gap { warning } else { th.accent });
        if bar.live {
            p.stroke_round(rect.inset(-2.0), 4.0, th.text, 1.5);
        }
    }
    for (frac, label) in day::timeline_ticks(t.from, t.to, track.w) {
        let x = track.x + frac * track.w;
        p.line(
            (x, track.bottom()),
            (x, track.bottom() + 3.0),
            th.text_secondary,
            1.0,
        );
        let (rect, align) = if frac < 0.001 {
            (Rect::new(x, track.bottom() + 3.0, 40.0, 14.0), Align::Start)
        } else if frac > 0.999 {
            (
                Rect::new(x - 40.0, track.bottom() + 3.0, 40.0, 14.0),
                Align::End,
            )
        } else {
            (
                Rect::new(x - 20.0, track.bottom() + 3.0, 40.0, 14.0),
                Align::Center,
            )
        };
        p.text_tabular(rect, &label, TextStyle::Caption, th.text_secondary, align);
    }
    divider(p, th, r, true);
}
fn session(p: &Painter, th: &Theme, r: Rect, row: &SessionRow) {
    if row.live {
        p.fill_round(r.inset(2.0), 4.0, th.card.blend(th.accent, 0.12));
    }
    let z = pages::session_zones(r.w);
    let line = |x: f32, w: f32, y: f32, h: f32| Rect::new(r.x + x, r.y + y, w, h);
    let strong = |slot: (f32, f32), text: &str, align| {
        p.text_tabular(
            line(slot.0, slot.1, 8.0, 22.0),
            text,
            TextStyle::BodyStrong,
            th.text,
            align,
        )
    };
    // One string, so the font's own spaces sit evenly on both sides of the dash.
    strong(
        z.left,
        &format!("{} – {}", row.start, row.end),
        Align::Start,
    );
    let sub = if row.live {
        th.accent
    } else {
        th.text_secondary
    };
    p.text(
        line(z.left.0, z.left.1, 29.0, 16.0),
        &row.duration,
        TextStyle::Caption,
        sub,
        Align::Start,
    );
    if let Some(gap) = &row.gap_line {
        let warning = th.tone(Tone::Warning).1;
        p.text(
            line(z.left.0, z.left.1, 46.0, 16.0),
            gap,
            TextStyle::Caption,
            warning,
            Align::Start,
        );
    }
    let unit = p.text_width("steps", TextStyle::Caption).ceil() + 1.0;
    p.text(
        line(z.right.0, z.right.1, 10.0, 18.0),
        "steps",
        TextStyle::Caption,
        th.text_secondary,
        Align::End,
    );
    p.text_tabular(
        line(z.right.0, z.right.1 - unit - 4.0, 8.0, 22.0),
        &row.steps,
        TextStyle::Title,
        th.text,
        Align::End,
    );
    p.text_tabular(
        line(z.right.0, z.right.1, 29.0, 16.0),
        &row.detail,
        TextStyle::Caption,
        th.text_secondary,
        Align::End,
    );
}
fn draw_block(
    p: &Painter,
    th: &Theme,
    scene: &Scene,
    r: Rect,
    block: &Block,
    input: &Input,
    state: &dyn Fn(Id) -> State,
) {
    let day: Option<&DayView> = scene.data.day.as_ref();
    match block {
        Block::Tile { index, compact } => {
            if let Some(t) = day.map(|d| &d.tiles[*index]) {
                widgets::tile(
                    p,
                    th,
                    r,
                    &TileText {
                        title: t.title,
                        value: &t.value,
                        lines: [&t.target, &t.remaining],
                        progress: t.progress,
                    },
                    *compact,
                );
            }
        }
        Block::Section(title) => widgets::section_title(p, th, r, title),
        Block::Note(text) => p.paragraph(r, text, TextStyle::Caption, th.text_secondary),
        Block::Panel => widgets::card(p, th, r),
        Block::Toggle { id, label, on } => widgets::toggle(p, th, r, label, *on, state(*id)),
        Block::Candidate { id, index, divider } => {
            if let Some((_, label, chosen)) = scene.data.devices.candidates.get(*index) {
                widgets::choice_row(p, th, r, label, *chosen, *divider, state(*id));
            }
        }
        Block::Stepper { down, up, value } => {
            let (a, c) = pages::stepper_buttons(r);
            widgets::icon_button(p, th, a, '\u{E738}', ButtonKind::Standard, state(*down));
            widgets::icon_button(p, th, c, '\u{E710}', ButtonKind::Standard, state(*up));
            p.text(
                Rect::new(a.right(), r.y, c.x - a.right(), r.h),
                value,
                TextStyle::BodyStrong,
                th.text,
                Align::Center,
            );
        }
        Block::Label(text) => p.text(r, text, TextStyle::Body, th.text, Align::Start),
        Block::Error(text) => p.paragraph(r, text, TextStyle::Caption, th.bad),
        Block::Field {
            id,
            label,
            caption,
            error,
        } => {
            p.text(
                Rect::new(r.x, r.y, r.w, 18.0),
                label,
                TextStyle::Body,
                th.text,
                Align::Start,
            );
            let frame = pages::field_frame(r);
            p.fill_round(frame, 4.0, th.control);
            if error.is_some() {
                p.stroke_round(frame, 4.0, th.bad, 1.5);
            } else if input.focus == Some(*id) {
                p.stroke_round(frame, 4.0, th.accent, 2.0);
            } else {
                p.stroke_round(frame, 4.0, th.control_border, 1.0);
            }
            let mut y = frame.bottom() + 2.0;
            if let Some(caption) = caption {
                let h = pages::note_height(caption, r.w);
                p.paragraph(
                    Rect::new(r.x, y, r.w, h),
                    caption,
                    TextStyle::Caption,
                    th.text_secondary,
                );
                y += h;
            }
            if let Some(error) = error {
                p.paragraph(
                    Rect::new(r.x, y, r.w, pages::note_height(error, r.w)),
                    error,
                    TextStyle::Caption,
                    th.bad,
                );
            }
        }
        Block::Row {
            label,
            value,
            divider,
        } => widgets::list_row(p, th, r, label, value, *divider),
        Block::Button { id, label, kind } => widgets::button(p, th, r, label, *kind, state(*id)),
        Block::IconButton { id, glyph } => {
            widgets::icon_button(p, th, r, *glyph, ButtonKind::Standard, state(*id))
        }
        Block::Summary(text) => {
            p.text_tabular(r, text, TextStyle::Caption, th.text_secondary, Align::End)
        }
        Block::Timeline => {
            if let Some(t) = day.and_then(|d| d.timeline.as_ref()) {
                timeline(p, th, r, t);
            }
        }
        Block::Session { index, divider: d } => {
            if let Some(row) = day.and_then(|d| d.sessions.get(*index)) {
                session(p, th, r, row);
                divider(p, th, r, *d);
            }
        }
        Block::Chart => {
            widgets::card(p, th, r);
            if let Some(h) = &scene.data.history {
                let bars: Vec<chart::Bar> = h
                    .bars
                    .iter()
                    .map(|b| chart::Bar {
                        steps: b.steps as f32,
                        goal: b.goal.map(|g| g as f32),
                        style: match b.kind {
                            BarKind::Observed => chart::Style::Normal,
                            BarKind::Partial => chart::Style::Partial,
                            BarKind::Empty => chart::Style::Empty,
                        },
                        label: b.label.clone(),
                    })
                    .collect();
                let hover = input
                    .hover
                    .filter(|id| Scene::is_bar(*id))
                    .map(|id| (id - pages::BAR_BASE) as usize);
                chart::draw(p, th, pages::chart_area(r), &bars, hover);
            }
        }
        Block::DayHead => head(p, th, r, &pages::history_cols(r.w)),
        Block::DayRow {
            id,
            index,
            divider: d,
        } => {
            if let Some(row) = scene.data.history.as_ref().and_then(|h| h.rows.get(*index)) {
                let st = state(*id);
                if st.visual == Visual::Hover || st.focused {
                    p.fill_rect(r.inset_xy(1.0, 0.0), th.nav_hover);
                }
                for col in pages::history_cols(r.w) {
                    let text = match col.field {
                        Field::Date => row.date.as_str(),
                        Field::Steps => row.steps.as_str(),
                        Field::Minutes => row.minutes.as_str(),
                        Field::Kcal => row.kcal.as_str(),
                        Field::Sessions => row.sessions.as_str(),
                        Field::Target => row.target.as_str(),
                        _ => "",
                    };
                    if col.field == Field::Coverage {
                        coverage(p, th, r, &col, row.kind);
                    } else {
                        cell(p, th, r, &col, text);
                    }
                }
                divider(p, th, r, *d);
                if st.focused {
                    widgets::focus_ring(p, th, r.inset(2.0), 2.0);
                }
            }
        }
    }
}
fn live_card(p: &Painter, th: &Theme, scene: &Scene, state: &dyn Fn(Id) -> State) {
    let f: &Frame = &scene.frame;
    let c = &scene.card;
    let (_, tone_color) = th.tone(c.tone);
    if f.collapsed {
        p.fill_circle(
            (f.pill.x + f.pill.w / 2.0, f.pill.y + f.pill.h / 2.0),
            5.0,
            tone_color,
        );
        p.text(f.speed, &c.speed, TextStyle::Title, th.text, Align::Center);
        widgets::icon_button(
            p,
            th,
            f.pause,
            icons::PAUSE,
            ButtonKind::Standard,
            state(frame::PAUSE),
        );
        return;
    }
    widgets::card(p, th, f.card.unwrap());
    let pill_w = widgets::pill_width(p, c.connection).min(f.pill.w);
    widgets::pill(
        p,
        th,
        Rect::new(f.pill.x, f.pill.y, pill_w, f.pill.h),
        c.connection,
        c.tone,
    );
    let number_w = p.text_width(&c.speed, TextStyle::Display).ceil() + 2.0;
    p.text(
        Rect::new(f.speed.x, f.speed.y, number_w, f.speed.h),
        &c.speed,
        TextStyle::Display,
        th.text,
        Align::Start,
    );
    p.text(
        Rect::new(
            f.speed.x + number_w + 6.0,
            f.speed.y + 10.0,
            (f.speed.w - number_w - 6.0).max(0.0),
            f.speed.h - 12.0,
        ),
        c.speed_caption,
        TextStyle::Caption,
        th.text_secondary,
        Align::Start,
    );
    if let Some(pending) = &c.pending {
        p.text(
            f.pending,
            pending,
            TextStyle::Caption,
            th.accent,
            Align::Start,
        );
    }
    widgets::button(
        p,
        th,
        f.minus,
        &c.decrease,
        ButtonKind::Standard,
        state(frame::MINUS),
    );
    widgets::button(
        p,
        th,
        f.plus,
        &c.increase,
        ButtonKind::Standard,
        state(frame::PLUS),
    );
    widgets::button(
        p,
        th,
        f.start,
        "Start",
        ButtonKind::Accent,
        state(frame::START),
    );
    widgets::button(
        p,
        th,
        f.pause,
        "Pause",
        ButtonKind::Standard,
        state(frame::PAUSE),
    );
    p.text(
        f.steps,
        &c.steps,
        TextStyle::Caption,
        th.text_secondary,
        Align::Start,
    );
    if let Some(fraction) = c.target_fraction {
        widgets::progress_bar(p, th, f.bar, fraction, th.accent);
    }
}
fn footer(p: &Painter, th: &Theme, scene: &Scene) {
    let f = &scene.frame;
    let c = &scene.card;
    if let Some(error) = &c.footer_error {
        p.text(f.footer, error, TextStyle::Caption, th.bad, Align::Start);
    }
}
