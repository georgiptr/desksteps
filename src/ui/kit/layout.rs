//! Pure layout arithmetic in logical pixels. No Windows types.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    pub fn inset(&self, d: f32) -> Self {
        self.inset_xy(d, d)
    }
    pub fn inset_xy(&self, dx: f32, dy: f32) -> Self {
        Self::new(
            self.x + dx,
            self.y + dy,
            (self.w - 2.0 * dx).max(0.0),
            (self.h - 2.0 * dy).max(0.0),
        )
    }
    pub fn offset(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }
    /// Splits off a left part of at most `w`; returns (left, rest).
    pub fn split_left(&self, w: f32) -> (Self, Self) {
        let w = w.clamp(0.0, self.w);
        (
            Self::new(self.x, self.y, w, self.h),
            Self::new(self.x + w, self.y, self.w - w, self.h),
        )
    }
    pub fn split_right(&self, w: f32) -> (Self, Self) {
        let w = w.clamp(0.0, self.w);
        (
            Self::new(self.x, self.y, self.w - w, self.h),
            Self::new(self.right() - w, self.y, w, self.h),
        )
    }
    /// Splits off a top part of at most `h`; returns (top, rest).
    pub fn split_top(&self, h: f32) -> (Self, Self) {
        let h = h.clamp(0.0, self.h);
        (
            Self::new(self.x, self.y, self.w, h),
            Self::new(self.x, self.y + h, self.w, self.h - h),
        )
    }
    pub fn split_bottom(&self, h: f32) -> (Self, Self) {
        let h = h.clamp(0.0, self.h);
        (
            Self::new(self.x, self.y, self.w, self.h - h),
            Self::new(self.x, self.bottom() - h, self.w, h),
        )
    }
    /// A `w` x `h` rectangle centred inside this one.
    pub fn centered(&self, w: f32, h: f32) -> Self {
        Self::new(
            self.x + (self.w - w) / 2.0,
            self.y + (self.h - h) / 2.0,
            w,
            h,
        )
    }
}
/// `n` equal columns separated by `gap`.
pub fn columns(area: Rect, n: usize, gap: f32) -> Vec<Rect> {
    if n == 0 {
        return vec![];
    }
    let w = ((area.w - gap * (n - 1) as f32) / n as f32).max(0.0);
    (0..n)
        .map(|i| Rect::new(area.x + i as f32 * (w + gap), area.y, w, area.h))
        .collect()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Breakpoint {
    Narrow,
    Medium,
    Wide,
}
/// Content-area breakpoints from the redesign plan: <560, 560-900, >900.
pub fn breakpoint(content_width: f32) -> Breakpoint {
    if content_width < 560.0 {
        Breakpoint::Narrow
    } else if content_width <= 900.0 {
        Breakpoint::Medium
    } else {
        Breakpoint::Wide
    }
}
pub const RAIL_EXPANDED: f32 = 220.0;
pub const RAIL_COLLAPSED: f32 = 64.0;
pub const RAIL_BREAKPOINT: f32 = 720.0;
pub const CONTENT_MAX: f32 = 1040.0;
pub fn rail_collapsed(window_width: f32) -> bool {
    window_width < RAIL_BREAKPOINT
}
pub fn rail_width(window_width: f32) -> f32 {
    if rail_collapsed(window_width) {
        RAIL_COLLAPSED
    } else {
        RAIL_EXPANDED
    }
}
/// Content area right of the rail, centred when wider than `CONTENT_MAX`.
pub fn content_area(window: Rect) -> Rect {
    let (_, rest) = window.split_left(rail_width(window.w));
    if rest.w > CONTENT_MAX {
        rest.centered(CONTENT_MAX, rest.h)
    } else {
        rest
    }
}
/// Vertical scroll state for content of some height inside a shorter view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scroll {
    pub offset: f32,
}
impl Scroll {
    pub fn max(content: f32, view: f32) -> f32 {
        (content - view).max(0.0)
    }
    pub fn clamp(&mut self, content: f32, view: f32) {
        self.offset = self.offset.clamp(0.0, Self::max(content, view));
    }
    pub fn by(&mut self, delta: f32, content: f32, view: f32) {
        self.offset += delta;
        self.clamp(content, view);
    }
    /// Scrolls the minimum distance that makes `item` (in content space) visible.
    pub fn reveal(&mut self, item: Rect, content: f32, view: f32) {
        if item.y < self.offset {
            self.offset = item.y;
        } else if item.bottom() > self.offset + view {
            self.offset = item.bottom() - view;
        }
        self.clamp(content, view);
    }
}
pub fn to_logical(physical: i32, dpi: u32) -> f32 {
    physical as f32 * 96.0 / dpi.max(1) as f32
}
pub fn to_physical(logical: f32, dpi: u32) -> i32 {
    (logical * dpi.max(1) as f32 / 96.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_preserve_total_and_clamp() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let (a, b) = r.split_left(30.0);
        assert_eq!((a.w, b.x, b.w), (30.0, 40.0, 70.0));
        let (a, b) = r.split_left(500.0);
        assert_eq!((a.w, b.w), (100.0, 0.0));
        let (a, b) = r.split_right(30.0);
        assert_eq!((a.w, b.x, b.w), (70.0, 80.0, 30.0));
        let (a, b) = r.split_top(20.0);
        assert_eq!((a.h, b.y, b.h), (20.0, 40.0, 30.0));
        let (a, b) = r.split_bottom(20.0);
        assert_eq!((a.h, b.y, b.h), (30.0, 50.0, 20.0));
        assert_eq!(r.split_top(-5.0).0.h, 0.0);
    }
    #[test]
    fn inset_never_goes_negative_and_contains_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(r.inset(8.0).w, 0.0);
        assert_eq!(r.inset_xy(2.0, 1.0), Rect::new(2.0, 1.0, 6.0, 8.0));
        assert!(r.contains(0.0, 0.0) && r.contains(9.9, 9.9));
        assert!(!r.contains(10.0, 5.0) && !r.contains(-0.1, 5.0));
        assert_eq!(r.centered(4.0, 2.0), Rect::new(3.0, 4.0, 4.0, 2.0));
        assert_eq!(r.offset(1.0, 2.0), Rect::new(1.0, 2.0, 10.0, 10.0));
    }
    #[test]
    fn columns_fill_width_with_gaps() {
        let cols = columns(Rect::new(0.0, 0.0, 320.0, 40.0), 3, 10.0);
        assert_eq!(cols.len(), 3);
        assert!(cols.iter().all(|c| (c.w - 100.0).abs() < 1e-4));
        assert_eq!(cols[2].right(), 320.0);
        assert!(columns(Rect::default(), 0, 4.0).is_empty());
        assert_eq!(columns(Rect::new(0.0, 0.0, 5.0, 5.0), 2, 10.0)[0].w, 0.0);
    }
    #[test]
    fn breakpoints_match_plan() {
        assert_eq!(breakpoint(559.9), Breakpoint::Narrow);
        assert_eq!(breakpoint(560.0), Breakpoint::Medium);
        assert_eq!(breakpoint(900.0), Breakpoint::Medium);
        assert_eq!(breakpoint(900.1), Breakpoint::Wide);
    }
    #[test]
    fn rail_collapses_below_720() {
        assert!(rail_collapsed(719.0) && !rail_collapsed(720.0));
        assert_eq!(rail_width(520.0), 64.0);
        assert_eq!(rail_width(820.0), 220.0);
    }
    #[test]
    fn content_is_capped_and_centred_on_wide_windows() {
        let c = content_area(Rect::new(0.0, 0.0, 820.0, 580.0));
        assert_eq!((c.x, c.w), (220.0, 600.0));
        let c = content_area(Rect::new(0.0, 0.0, 2000.0, 600.0));
        assert_eq!(c.w, 1040.0);
        assert_eq!(c.x, 220.0 + (1780.0 - 1040.0) / 2.0);
    }
    #[test]
    fn scroll_clamps_to_the_content() {
        let mut s = Scroll::default();
        s.by(500.0, 300.0, 200.0);
        assert_eq!(s.offset, 100.0);
        s.by(-500.0, 300.0, 200.0);
        assert_eq!(s.offset, 0.0);
        s.by(10.0, 100.0, 200.0);
        assert_eq!(s.offset, 0.0);
        s.reveal(Rect::new(0.0, 250.0, 10.0, 40.0), 400.0, 200.0);
        assert_eq!(s.offset, 90.0);
        s.reveal(Rect::new(0.0, 10.0, 10.0, 40.0), 400.0, 200.0);
        assert_eq!(s.offset, 10.0);
    }
    #[test]
    fn dpi_conversion_round_trips_at_common_scales() {
        for dpi in [96, 144, 216] {
            assert_eq!(to_physical(to_logical(1000, dpi), dpi), 1000);
        }
        assert_eq!(to_physical(820.0, 216), 1845);
        assert_eq!(to_logical(10, 0), 10.0 * 96.0);
    }
}
