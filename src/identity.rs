//! Shared artwork and runtime identity; BGRA pixels match the native ICO format.
pub const DEVELOPMENT_TITLE: &str = "DeskSteps Development — Offline";
pub fn title(development: bool) -> &'static str {
    if development {
        DEVELOPMENT_TITLE
    } else {
        "DeskSteps"
    }
}
pub fn pixel(x: u32, y: u32, development: bool) -> [u8; 4] {
    if (6..26).contains(&x) && (23..26).contains(&y)
        || (8..13).contains(&x) && (13..20).contains(&y)
        || (17..22).contains(&x) && (7..14).contains(&y)
    {
        [255, 255, 255, 255]
    } else if development {
        [45, 45, 200, 255]
    } else {
        [180, 100, 25, 255]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn development_changes_only_the_background() {
        for y in 0..32 {
            for x in 0..32 {
                let normal = pixel(x, y, false);
                let dev = pixel(x, y, true);
                if normal == [255; 4] {
                    assert_eq!(dev, normal);
                } else {
                    assert!(dev[2] > dev[0] && dev[2] > dev[1]);
                }
            }
        }
        assert_eq!(title(false), "DeskSteps");
        assert_eq!(title(true), DEVELOPMENT_TITLE);
    }
}
