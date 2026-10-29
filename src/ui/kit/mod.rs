//! Shared Direct2D drawing and input toolkit. `layout`, `input` and the palette
//! types in `theme` are pure; `render` is the only module that owns GPU objects.
pub(crate) mod chart;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod render;
pub(crate) mod theme;
pub(crate) mod widgets;
