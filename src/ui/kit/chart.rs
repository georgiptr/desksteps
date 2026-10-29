//! Daily step bar chart: pure geometry plus drawing. Axis labels stay inside the area.
use super::{
    layout::Rect,
    render::{Align, Painter},
    theme::{TextStyle, Theme},
};

pub const AXIS_WIDTH: f32 = 48.0;
pub const LABEL_HEIGHT: f32 = 22.0;
const TOP_PAD: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Normal,
    Partial,
    Empty,
}
#[derive(Clone, Debug)]
pub struct Bar {
    pub steps: f32,
    pub goal: Option<f32>,
    pub style: Style,
    pub label: String,
}
/// Bars are drawn inside this rectangle.
pub fn plot(area: Rect) -> Rect {
    Rect::new(
        area.x + AXIS_WIDTH,
        area.y + TOP_PAD,
        (area.w - AXIS_WIDTH).max(0.0),
        (area.h - TOP_PAD - LABEL_HEIGHT).max(0.0),
    )
}
/// Full-height columns, one per bar, used for drawing and hit testing.
pub fn slots(area: Rect, n: usize) -> Vec<Rect> {
    let plot = plot(area);
    let w = plot.w / n.max(1) as f32;
    (0..n)
        .map(|i| Rect::new(plot.x + i as f32 * w, plot.y, w, plot.h))
        .collect()
}
fn label_width(text: &str) -> f32 {
    text.chars().count() as f32 * 7.0 + 4.0
}
/// Labels to draw, newest first: every n-th bar counted back from the last, centred under
/// its bar but clamped into the plot so the final label is never clipped, and dropped
/// when it would touch a label already placed.
pub fn label_layout(area: Rect, labels: &[String]) -> Vec<(usize, Rect)> {
    let n = labels.len();
    let plot = plot(area);
    let slot = plot.w / n.max(1) as f32;
    let widest = labels.iter().map(|l| label_width(l)).fold(0.0, f32::max);
    let every = ((widest + 6.0) / slot.max(1.0)).ceil().max(1.0) as usize;
    let mut placed: Vec<(usize, Rect)> = Vec::new();
    for i in (0..n).rev() {
        if !(n - 1 - i).is_multiple_of(every) {
            continue;
        }
        let w = label_width(&labels[i]);
        let x = (plot.x + (i as f32 + 0.5) * slot - w / 2.0)
            .min(plot.right() - w)
            .max(plot.x);
        let rect = Rect::new(x, plot.bottom() + 4.0, w, LABEL_HEIGHT - 6.0);
        if placed
            .iter()
            .all(|(_, r)| rect.right() + 4.0 <= r.x || r.right() + 4.0 <= rect.x)
        {
            placed.push((i, rect));
        }
    }
    placed
}
pub fn axis_max(bars: &[Bar]) -> f32 {
    bars.iter()
        .map(|b| b.steps.max(b.goal.unwrap_or(0.0)))
        .fold(1.0, f32::max)
}
pub fn bar_rect(slot: Rect, steps: f32, max: f32) -> Rect {
    let gap = if slot.w > 8.0 { 3.0 } else { 1.0 };
    let h = (slot.h * steps / max).clamp(0.0, slot.h);
    Rect::new(
        slot.x + gap,
        slot.bottom() - h,
        (slot.w - 2.0 * gap).max(1.0),
        h,
    )
}
pub fn draw(p: &Painter, th: &Theme, area: Rect, bars: &[Bar], hover: Option<usize>) {
    let plot = plot(area);
    let max = axis_max(bars);
    p.text(
        Rect::new(area.x, plot.y - 8.0, AXIS_WIDTH - 6.0, 16.0),
        &crate::ui::model::live::group_digits(max.round() as i64),
        TextStyle::Caption,
        th.text_secondary,
        Align::End,
    );
    p.text(
        Rect::new(area.x, plot.bottom() - 8.0, AXIS_WIDTH - 6.0, 16.0),
        "0",
        TextStyle::Caption,
        th.text_secondary,
        Align::End,
    );
    p.line(
        (plot.x, plot.bottom()),
        (plot.right(), plot.bottom()),
        th.card_border,
        1.0,
    );
    let grey = th.text_secondary.blend(th.background, 0.45);
    for (i, (bar, slot)) in bars.iter().zip(slots(area, bars.len())).enumerate() {
        if hover == Some(i) {
            p.fill_round(slot, 3.0, th.nav_hover);
        }
        let r = bar_rect(slot, bar.steps, max);
        match bar.style {
            Style::Empty => p.fill_rect(Rect::new(r.x, plot.bottom() - 2.0, r.w, 2.0), th.track),
            style => p.fill_round(
                Rect::new(r.x, r.y.min(plot.bottom() - 2.0), r.w, r.h.max(2.0)),
                if r.w > 6.0 { 2.0 } else { 0.0 },
                if style == Style::Partial {
                    grey
                } else {
                    th.accent
                },
            ),
        }
        if let Some(goal) = bar.goal {
            let y = plot.bottom() - plot.h * goal / max;
            p.fill_rect(Rect::new(slot.x, y - 0.5, slot.w, 1.0), th.text);
        }
    }
    let labels: Vec<String> = bars.iter().map(|b| b.label.clone()).collect();
    for (i, rect) in label_layout(area, &labels) {
        p.text(
            rect,
            &bars[i].label,
            TextStyle::Caption,
            th.text_secondary,
            Align::Center,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn labels(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("10-{:02}", i % 28 + 1)).collect()
    }
    #[test]
    fn slots_tile_the_plot_without_gaps() {
        let area = Rect::new(10.0, 20.0, 348.0, 200.0);
        let s = slots(area, 30);
        let p = plot(area);
        assert_eq!(s.len(), 30);
        assert!((s[0].x - p.x).abs() < 1e-4 && (s[29].right() - p.right()).abs() < 1e-3);
        assert!(s.iter().all(|r| r.y == p.y && r.h == p.h));
        assert!(slots(area, 0).is_empty());
    }
    #[test]
    fn labels_stay_inside_the_plot_and_never_overlap_at_any_range_or_width() {
        for n in [1, 7, 30, 90] {
            for width in [260.0, 348.0, 480.0, 700.0, 1000.0] {
                let area = Rect::new(0.0, 0.0, width, 200.0);
                let plot = plot(area);
                let placed = label_layout(area, &labels(n));
                assert!(
                    placed.iter().any(|(i, _)| *i == n - 1),
                    "latest label n={n} w={width}"
                );
                for (k, (_, r)) in placed.iter().enumerate() {
                    assert!(
                        r.x >= plot.x - 0.01 && r.right() <= plot.right() + 0.01,
                        "n={n} w={width} {r:?}"
                    );
                    for (_, other) in &placed[k + 1..] {
                        assert!(
                            r.right() + 4.0 <= other.x || other.right() + 4.0 <= r.x,
                            "n={n} w={width}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn dense_ranges_label_fewer_bars_than_sparse_ones() {
        let area = Rect::new(0.0, 0.0, 400.0, 200.0);
        assert_eq!(label_layout(area, &labels(7)).len(), 7);
        assert!(label_layout(area, &labels(90)).len() < 15);
        assert!(label_layout(area, &[]).is_empty());
    }
    #[test]
    fn axis_covers_goals_and_bars_are_clamped() {
        let bar = |steps, goal| Bar {
            steps,
            goal,
            style: Style::Normal,
            label: String::new(),
        };
        assert_eq!(axis_max(&[]), 1.0);
        assert_eq!(axis_max(&[bar(5.0, Some(8.0)), bar(6.0, None)]), 8.0);
        let slot = Rect::new(0.0, 0.0, 20.0, 100.0);
        assert_eq!(bar_rect(slot, 50.0, 100.0).h, 50.0);
        assert_eq!(bar_rect(slot, 500.0, 100.0).h, 100.0);
        assert_eq!(bar_rect(slot, -3.0, 100.0).h, 0.0);
        assert!(bar_rect(Rect::new(0.0, 0.0, 4.0, 10.0), 1.0, 1.0).w >= 1.0);
    }
}
