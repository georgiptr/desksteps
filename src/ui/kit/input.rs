//! Hit testing, hover/press tracking and keyboard focus. Pure: no Windows types.
use super::layout::Rect;

pub type Id = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub id: Id,
    pub rect: Rect,
    pub enabled: bool,
    /// Whether Tab stops here; list rows and chart bars are reached with arrows or the mouse.
    pub tab_stop: bool,
}
/// Targets in tab order. Later targets win hit tests where rectangles overlap.
#[derive(Clone, Debug, Default)]
pub struct Targets(pub Vec<Target>);
impl Targets {
    pub fn push(&mut self, id: Id, rect: Rect, enabled: bool) {
        self.push_stop(id, rect, enabled, true);
    }
    pub fn push_stop(&mut self, id: Id, rect: Rect, enabled: bool, tab_stop: bool) {
        self.0.push(Target {
            id,
            rect,
            enabled,
            tab_stop,
        });
    }
    pub fn rect(&self, id: Id) -> Option<Rect> {
        self.0.iter().find(|t| t.id == id).map(|t| t.rect)
    }
    pub fn enabled(&self, id: Id) -> bool {
        self.0.iter().any(|t| t.id == id && t.enabled)
    }
    pub fn hit(&self, x: f32, y: f32) -> Option<Id> {
        self.0
            .iter()
            .rev()
            .find(|t| t.rect.contains(x, y))
            .map(|t| t.id)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visual {
    Normal,
    Hover,
    Pressed,
    Disabled,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub hover: Option<Id>,
    pub pressed: Option<Id>,
    pub focus: Option<Id>,
    /// The focus ring only shows after keyboard use.
    pub focus_visible: bool,
}
impl Input {
    /// Returns true when the hover target changed.
    pub fn mouse_move(&mut self, targets: &Targets, x: f32, y: f32) -> bool {
        // Disabled targets still hover so their tooltips can explain why.
        let hover = targets.hit(x, y);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }
    pub fn mouse_leave(&mut self) -> bool {
        let changed = self.hover.is_some() || self.pressed.is_some();
        self.hover = None;
        self.pressed = None;
        changed
    }
    pub fn mouse_down(&mut self, targets: &Targets, x: f32, y: f32) {
        self.focus_visible = false;
        self.pressed = targets.hit(x, y).filter(|id| targets.enabled(*id));
        if self.pressed.is_some() {
            self.focus = self.pressed;
        }
    }
    /// Returns the activated target: released over the same enabled target it was pressed on.
    pub fn mouse_up(&mut self, targets: &Targets, x: f32, y: f32) -> Option<Id> {
        let pressed = self.pressed.take()?;
        (targets.hit(x, y) == Some(pressed) && targets.enabled(pressed)).then_some(pressed)
    }
    /// Moves focus to the next (or previous) tab stop, wrapping at the ends. A focused
    /// non-stop target, such as a list row, continues from its own position.
    pub fn tab(&mut self, targets: &Targets, backwards: bool) -> Option<Id> {
        self.focus_visible = true;
        let all: Vec<&Target> = targets.0.iter().filter(|t| t.enabled).collect();
        let n = all.len();
        let at = self.focus.and_then(|f| all.iter().position(|t| t.id == f));
        self.focus = (1..=n)
            .map(|step| match (at, backwards) {
                (Some(i), false) => (i + step) % n,
                (Some(i), true) => (i + n - step % n) % n,
                (None, false) => step - 1,
                (None, true) => n - step,
            })
            .map(|i| all[i])
            .find(|t| t.tab_stop)
            .map(|t| t.id);
        self.focus
    }
    /// Moves focus to a neighbour within `group` (arrow keys in a list or rail).
    pub fn step_within(&mut self, targets: &Targets, group: &[Id], delta: i32) -> Option<Id> {
        self.focus_visible = true;
        let members: Vec<Id> = group
            .iter()
            .copied()
            .filter(|id| targets.enabled(*id))
            .collect();
        let at = self
            .focus
            .and_then(|f| members.iter().position(|id| *id == f))?;
        let next = (at as i32 + delta).clamp(0, members.len() as i32 - 1) as usize;
        self.focus = Some(members[next]);
        self.focus
    }
    /// Enter or Space activates the focused target only while its focus ring is visible,
    /// that is after the user's own Tab or arrow navigation, and only if it is enabled.
    /// A target focused by a mouse press never reacts to the keyboard.
    pub fn key_activate(&self, targets: &Targets) -> Option<Id> {
        if !self.focus_visible {
            return None;
        }
        self.focus.filter(|id| targets.enabled(*id))
    }
    pub fn visual(&self, targets: &Targets, id: Id) -> Visual {
        if !targets.enabled(id) {
            Visual::Disabled
        } else if self.pressed == Some(id) && self.hover == Some(id) {
            Visual::Pressed
        } else if self.hover == Some(id) && self.pressed.is_none_or(|p| p == id) {
            Visual::Hover
        } else {
            Visual::Normal
        }
    }
    pub fn focused(&self, id: Id) -> bool {
        self.focus_visible && self.focus == Some(id)
    }
    /// Drops focus/press state for targets that no longer exist or are disabled.
    pub fn retain_valid(&mut self, targets: &Targets) {
        if self.focus.is_some_and(|id| !targets.enabled(id)) {
            self.focus = None;
        }
        if self.pressed.is_some_and(|id| !targets.enabled(id)) {
            self.pressed = None;
        }
        if self.hover.is_some_and(|id| targets.rect(id).is_none()) {
            self.hover = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn targets() -> Targets {
        let mut t = Targets::default();
        t.push(1, Rect::new(0.0, 0.0, 10.0, 10.0), true);
        t.push(2, Rect::new(20.0, 0.0, 10.0, 10.0), false);
        t.push(3, Rect::new(40.0, 0.0, 10.0, 10.0), true);
        t
    }
    #[test]
    fn hit_testing_prefers_later_targets() {
        let mut t = targets();
        t.push(9, Rect::new(0.0, 0.0, 5.0, 5.0), true);
        assert_eq!(t.hit(1.0, 1.0), Some(9));
        assert_eq!(t.hit(8.0, 8.0), Some(1));
        assert_eq!(t.hit(15.0, 5.0), None);
        assert_eq!(t.rect(3), Some(Rect::new(40.0, 0.0, 10.0, 10.0)));
        assert!(!t.enabled(2) && t.enabled(3) && !t.enabled(77));
    }
    #[test]
    fn press_activates_only_when_released_on_the_same_enabled_target() {
        let t = targets();
        let mut i = Input::default();
        i.mouse_down(&t, 1.0, 1.0);
        assert_eq!(i.pressed, Some(1));
        assert_eq!(i.mouse_up(&t, 45.0, 5.0), None);
        i.mouse_down(&t, 1.0, 1.0);
        assert_eq!(i.mouse_up(&t, 2.0, 2.0), Some(1));
        i.mouse_down(&t, 25.0, 5.0);
        assert_eq!(i.pressed, None);
        assert_eq!(i.mouse_up(&t, 25.0, 5.0), None);
    }
    #[test]
    fn hover_and_visual_states() {
        let t = targets();
        let mut i = Input::default();
        assert!(i.mouse_move(&t, 1.0, 1.0));
        assert!(!i.mouse_move(&t, 2.0, 2.0));
        assert_eq!(i.visual(&t, 1), Visual::Hover);
        assert_eq!(i.visual(&t, 2), Visual::Disabled);
        i.mouse_down(&t, 1.0, 1.0);
        assert_eq!(i.visual(&t, 1), Visual::Pressed);
        i.mouse_move(&t, 45.0, 5.0);
        assert_eq!(i.visual(&t, 1), Visual::Normal);
        assert_eq!(i.visual(&t, 3), Visual::Normal);
        assert!(i.mouse_leave());
        assert!(!i.mouse_leave());
    }
    #[test]
    fn tab_skips_disabled_targets_and_wraps() {
        let t = targets();
        let mut i = Input::default();
        assert_eq!(i.tab(&t, false), Some(1));
        assert!(i.focus_visible && i.focused(1));
        assert_eq!(i.tab(&t, false), Some(3));
        assert_eq!(i.tab(&t, false), Some(1));
        assert_eq!(i.tab(&t, true), Some(3));
        let mut j = Input::default();
        assert_eq!(j.tab(&t, true), Some(3));
        assert_eq!(j.tab(&Targets::default(), false), None);
    }
    #[test]
    fn tab_skips_non_stops_and_continues_from_a_focused_row() {
        let mut t = Targets::default();
        t.push(1, Rect::new(0.0, 0.0, 10.0, 10.0), true);
        t.push_stop(7, Rect::new(0.0, 20.0, 10.0, 10.0), true, false);
        t.push_stop(8, Rect::new(0.0, 40.0, 10.0, 10.0), true, false);
        t.push(2, Rect::new(0.0, 60.0, 10.0, 10.0), true);
        let mut i = Input::default();
        assert_eq!(i.tab(&t, false), Some(1));
        assert_eq!(i.tab(&t, false), Some(2));
        i.focus = Some(8);
        assert_eq!(i.tab(&t, false), Some(2));
        i.focus = Some(7);
        assert_eq!(i.tab(&t, true), Some(1));
        let mut none = Targets::default();
        none.push_stop(5, Rect::new(0.0, 0.0, 1.0, 1.0), true, false);
        assert_eq!(Input::default().tab(&none, false), None);
    }
    #[test]
    fn arrows_stay_inside_the_group() {
        let t = targets();
        let mut i = Input {
            focus: Some(1),
            ..Default::default()
        };
        assert_eq!(i.step_within(&t, &[1, 2, 3], 1), Some(3));
        assert_eq!(i.step_within(&t, &[1, 2, 3], 1), Some(3));
        assert_eq!(i.step_within(&t, &[1, 2, 3], -1), Some(1));
        let mut outside = Input::default();
        assert_eq!(outside.step_within(&t, &[1, 3], 1), None);
    }
    #[test]
    fn keyboard_activation_requires_an_enabled_focus() {
        let t = targets();
        let mut i = Input::default();
        assert_eq!(i.key_activate(&t), None);
        i.focus = Some(3);
        assert_eq!(i.key_activate(&t), None, "no visible ring, no activation");
        i.focus_visible = true;
        assert_eq!(i.key_activate(&t), Some(3));
        i.focus = Some(2);
        assert_eq!(i.key_activate(&t), None);
        // A mouse press moves focus but hides the ring, so Enter cannot repeat the click.
        i.mouse_down(&t, 41.0, 1.0);
        assert_eq!((i.focus, i.focus_visible), (Some(3), false));
        assert_eq!(i.key_activate(&t), None);
    }
    #[test]
    fn mouse_down_hides_focus_ring_and_stale_state_is_dropped() {
        let t = targets();
        let mut i = Input::default();
        i.tab(&t, false);
        i.mouse_down(&t, 41.0, 1.0);
        assert!(!i.focus_visible);
        assert_eq!(i.focus, Some(3));
        let mut smaller = Targets::default();
        smaller.push(1, Rect::new(0.0, 0.0, 10.0, 10.0), true);
        i.hover = Some(3);
        i.retain_valid(&smaller);
        assert_eq!((i.focus, i.pressed, i.hover), (None, None, None));
    }
}
