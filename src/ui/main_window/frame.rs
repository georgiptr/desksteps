//! Pure window geometry: rail, live card, header, body and hit targets.
use crate::ui::{
    kit::{
        input::{Id, Targets},
        layout::{Rect, content_area, rail_collapsed},
    },
    model::{live::LiveCard, page::Page},
};

pub const MINUS: Id = 1;
pub const PLUS: Id = 2;
pub const START: Id = 3;
pub const PAUSE: Id = 4;
pub const NAV: Id = 10;
pub const MIN_WIDTH: f32 = 520.0;
pub const MIN_HEIGHT: f32 = 420.0;
pub const DEFAULT_WIDTH: f32 = 820.0;
pub const DEFAULT_HEIGHT: f32 = 580.0;

pub fn nav_id(page: Page) -> Id {
    NAV + page.index() as Id
}
pub fn nav_page(id: Id) -> Option<Page> {
    id.checked_sub(NAV)
        .and_then(|i| Page::from_index(i as usize))
}
pub fn nav_ids() -> Vec<Id> {
    Page::ALL.iter().map(|p| nav_id(*p)).collect()
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub size: Rect,
    pub collapsed: bool,
    pub rail: Rect,
    pub badge: Option<Rect>,
    /// Expanded rail only.
    pub card: Option<Rect>,
    pub pill: Rect,
    pub speed: Rect,
    pub pending: Rect,
    pub minus: Rect,
    pub plus: Rect,
    pub start: Rect,
    pub pause: Rect,
    pub steps: Rect,
    pub bar: Rect,
    pub nav: [Rect; 4],
    pub footer: Rect,
    pub content: Rect,
    pub title: Rect,
    /// Scrolling page area below the header.
    pub body: Rect,
}
impl Frame {
    pub fn new(width: f32, height: f32, development: bool) -> Self {
        let size = Rect::new(0.0, 0.0, width, height);
        let collapsed = rail_collapsed(width);
        let short = height < 470.0;
        let rail = Rect::new(0.0, 0.0, if collapsed { 64.0 } else { 220.0 }, height);
        let nav_h = if short { 32.0 } else { 40.0 };
        let nav_gap = if short { 0.0 } else { 2.0 };
        let mut f = Self {
            size,
            collapsed,
            rail,
            badge: None,
            card: None,
            pill: Rect::default(),
            speed: Rect::default(),
            pending: Rect::default(),
            minus: Rect::default(),
            plus: Rect::default(),
            start: Rect::default(),
            pause: Rect::default(),
            steps: Rect::default(),
            bar: Rect::default(),
            nav: [Rect::default(); 4],
            footer: Rect::default(),
            content: Rect::default(),
            title: Rect::default(),
            body: Rect::default(),
        };
        let mut y = 8.0;
        if collapsed {
            let x = 8.0;
            let w = 48.0;
            if development {
                f.badge = Some(Rect::new(x, y, w, 18.0));
                y += 24.0;
            }
            f.pill = Rect::new(x, y, w, 20.0);
            y += 26.0;
            f.speed = Rect::new(x, y, w, 28.0);
            y += 32.0;
            f.pause = Rect::new(x, y, w, 32.0);
            y += 44.0;
            for (i, r) in f.nav.iter_mut().enumerate() {
                *r = Rect::new(x, y + i as f32 * (nav_h + nav_gap), w, nav_h);
            }
        } else {
            let x = 8.0;
            let w = rail.w - 16.0;
            if development {
                f.badge = Some(Rect::new(x, y, w, 22.0));
                y += 30.0;
            }
            let card = Rect::new(x, y, w, 168.0);
            f.card = Some(card);
            let inner = card.inset_xy(8.0, 8.0);
            f.pill = Rect::new(inner.x, inner.y, inner.w, 22.0);
            f.speed = Rect::new(inner.x, inner.y + 26.0, inner.w, 40.0);
            f.pending = Rect::new(inner.x, inner.y + 68.0, inner.w, 16.0);
            let gap = 4.0;
            let bw = (inner.w - 3.0 * gap) / 4.0;
            let by = inner.y + 88.0;
            let button = |i: f32| Rect::new(inner.x + i * (bw + gap), by, bw, 32.0);
            f.minus = button(0.0);
            f.plus = button(1.0);
            f.start = button(2.0);
            f.pause = button(3.0);
            f.steps = Rect::new(inner.x, inner.y + 126.0, inner.w, 16.0);
            f.bar = Rect::new(inner.x, inner.y + 144.0, inner.w, 6.0);
            y = card.bottom() + 12.0;
            for (i, r) in f.nav.iter_mut().enumerate() {
                *r = Rect::new(x, y + i as f32 * (nav_h + nav_gap), w, nav_h);
            }
            f.footer = Rect::new(x + 4.0, height - 48.0, w - 8.0, 40.0);
        }
        let mut content = content_area(size);
        if collapsed {
            // Without room in the rail the status footer becomes a strip under the page.
            let (rest, footer) = content.split_bottom(24.0);
            f.footer = Rect::new(
                rest.x + 24.0,
                footer.y,
                (footer.w - 48.0).max(0.0),
                footer.h,
            );
            content = rest;
        }
        f.content = content;
        let (header, body) = content.split_top(56.0);
        f.title = Rect::new(
            header.x + 24.0,
            header.y + 8.0,
            (header.w - 48.0).max(0.0),
            40.0,
        );
        f.body = Rect::new(
            body.x + 24.0,
            body.y,
            (body.w - 48.0).max(0.0),
            (body.h - 12.0).max(0.0),
        );
        if collapsed {
            // Start and the speed buttons move to the page header.
            let y = header.y + 12.0;
            let mut right = header.right() - 24.0;
            for (rect, w) in [
                (&mut f.start, 72.0),
                (&mut f.plus, 56.0),
                (&mut f.minus, 56.0),
            ] {
                right -= w;
                *rect = Rect::new(right, y, w, 32.0);
                right -= 6.0;
            }
            f.title.w = (right - f.title.x).max(0.0);
        }
        f
    }
    /// Width available to page blocks.
    pub fn page_width(&self) -> f32 {
        self.body.w
    }
    /// Targets in tab order. Page targets are in window space after scrolling.
    pub fn targets(&self, card: &LiveCard, page_targets: &[(Id, Rect, bool, bool)]) -> Targets {
        let mut t = Targets::default();
        // Tab order: Pause, navigation and the page come first. Movement controls
        // come last so a first Tab press never lands on Start or the speed buttons.
        t.push(PAUSE, self.pause, card.pause);
        for page in Page::ALL {
            t.push(nav_id(page), self.nav[page.index()], true);
        }
        for (id, rect, enabled, tab_stop) in page_targets {
            if let Some(visible) = intersect(*rect, self.body) {
                t.push_stop(*id, visible, *enabled, *tab_stop);
            }
        }
        t.push(MINUS, self.minus, card.minus);
        t.push(PLUS, self.plus, card.plus);
        t.push(START, self.start, card.start);
        t
    }
}
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let r = a.right().min(b.right());
    let bt = a.bottom().min(b.bottom());
    (r > x && bt > y).then(|| Rect::new(x, y, r - x, bt - y))
}
pub fn tooltip_text(id: Id, card: &LiveCard, collapsed: bool) -> Option<String> {
    match id {
        START => Some(crate::ui::model::live::START_HINT.into()),
        PAUSE => Some("Pause the belt. Available whenever controls are available.".into()),
        MINUS => Some(format!(
            "Decrease selected speed ({} km/h)",
            &card.decrease[3..]
        )),
        PLUS => Some(format!(
            "Increase selected speed ({} km/h)",
            &card.increase[1..]
        )),
        _ if collapsed => nav_page(id).map(|p| p.label().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PAGE_ID: Id = 99;
    use crate::application::View;
    fn inside(outer: Rect, inner: Rect) -> bool {
        inner.x >= outer.x - 0.01
            && inner.y >= outer.y - 0.01
            && inner.right() <= outer.right() + 0.01
            && inner.bottom() <= outer.bottom() + 0.01
    }
    fn overlap(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }
    #[test]
    fn every_element_fits_at_every_supported_size() {
        for dev in [false, true] {
            for (w, h) in [
                (520.0, 420.0),
                (560.0, 600.0),
                (719.0, 420.0),
                (720.0, 420.0),
                (720.0, 469.0),
                (820.0, 580.0),
                (1400.0, 900.0),
                (2600.0, 1400.0),
            ] {
                let f = Frame::new(w, h, dev);
                let window = Rect::new(0.0, 0.0, w, h);
                let mut parts = vec![f.pill, f.speed, f.pause, f.footer, f.title, f.body];
                parts.extend(f.nav);
                parts.extend(f.badge);
                if f.collapsed {
                    parts.extend([f.minus, f.plus, f.start]);
                } else {
                    parts.extend([f.pending, f.minus, f.plus, f.start, f.steps, f.bar]);
                    parts.push(f.card.unwrap());
                }
                for part in parts {
                    assert!(inside(window, part), "{w}x{h} dev={dev}: {part:?}");
                    assert!(part.w > 0.0 && part.h > 0.0, "{w}x{h}: {part:?}");
                }
                for part in f.nav {
                    assert!(inside(f.rail, part), "{w}x{h}");
                }
                assert_eq!(f.collapsed, w < 720.0);
                // Nothing in the rail overlaps the content area.
                assert!(!overlap(f.rail, f.content) || f.rail.right() <= f.content.x + 0.01);
            }
        }
    }
    #[test]
    fn controls_do_not_overlap_each_other() {
        for (w, h) in [(520.0, 420.0), (720.0, 420.0), (820.0, 580.0)] {
            let f = Frame::new(w, h, true);
            let mut rects = vec![f.minus, f.plus, f.start, f.pause];
            rects.extend(f.nav);
            if f.collapsed {
                rects.push(f.title);
            }
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    assert!(!overlap(*a, *b), "{w}x{h}: {a:?} vs {b:?}");
                }
            }
        }
    }
    #[test]
    fn rail_nav_sits_below_the_card_and_above_the_footer() {
        for (w, h) in [(720.0, 420.0), (720.0, 470.0), (820.0, 580.0)] {
            let f = Frame::new(w, h, true);
            assert!(f.nav[0].y >= f.card.unwrap().bottom(), "{h}");
            assert!(f.nav[3].bottom() <= f.footer.y + 0.01, "{h}");
        }
    }
    #[test]
    fn live_card_always_ends_at_the_progress_bar() {
        for development in [false, true] {
            let frame = Frame::new(820.0, 580.0, development);
            assert_eq!(frame.card.unwrap().bottom() - frame.bar.bottom(), 10.0);
        }
    }
    #[test]
    fn collapsed_layout_moves_start_and_speed_to_the_header() {
        let f = Frame::new(520.0, 420.0, false);
        assert!(f.collapsed && f.card.is_none());
        assert_eq!(f.rail.w, 64.0);
        for r in [f.minus, f.plus, f.start] {
            assert!(r.y < f.body.y && r.x >= f.content.x);
        }
        assert!(f.title.right() <= f.minus.x);
        assert!(f.pause.right() <= f.rail.w);
    }
    #[test]
    fn targets_follow_tab_order_and_enable_flags() {
        let f = Frame::new(820.0, 580.0, false);
        let card = LiveCard::from_view(&View::default(), None);
        let t = f.targets(
            &card,
            &[(
                PAGE_ID,
                Rect::new(f.body.x, f.body.y + 10.0, 100.0, 30.0),
                true,
                true,
            )],
        );
        let ids: Vec<_> = t.0.iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            vec![PAUSE, 10, 11, 12, 13, PAGE_ID, MINUS, PLUS, START]
        );
        assert!(!t.enabled(PAUSE) && t.enabled(nav_id(Page::History)));
        assert_eq!(nav_page(11), Some(Page::History));
        assert_eq!(nav_page(4), None);
        assert_eq!(nav_ids().len(), 4);
    }
    #[test]
    fn scrolled_out_page_targets_are_clipped_to_the_body() {
        let f = Frame::new(820.0, 580.0, false);
        let card = LiveCard::from_view(&View::default(), None);
        let above = Rect::new(f.body.x, f.body.y - 50.0, 100.0, 30.0);
        let partly = Rect::new(f.body.x, f.body.y - 10.0, 100.0, 30.0);
        let t = f.targets(
            &card,
            &[
                (PAGE_ID, above, true, true),
                (PAGE_ID + 1, partly, true, true),
            ],
        );
        assert!(t.rect(PAGE_ID).is_none());
        assert_eq!(t.rect(PAGE_ID + 1).unwrap().y, f.body.y);
    }
    #[test]
    fn tooltips_cover_movement_controls_and_collapsed_nav() {
        let card = LiveCard::from_view(
            &View {
                increment: 3,
                ..Default::default()
            },
            None,
        );
        assert!(
            tooltip_text(START, &card, false)
                .unwrap()
                .contains("selected by the pad")
        );
        assert!(tooltip_text(MINUS, &card, false).unwrap().contains("0.3"));
        assert!(tooltip_text(PLUS, &card, false).unwrap().contains("0.3"));
        assert!(tooltip_text(PAUSE, &card, false).is_some());
        assert_eq!(
            tooltip_text(nav_id(Page::Targets), &card, true).as_deref(),
            Some("Targets")
        );
        assert_eq!(tooltip_text(nav_id(Page::Targets), &card, false), None);
    }
}
