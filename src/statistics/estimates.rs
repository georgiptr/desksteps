//! Published estimate arithmetic and target progress.
pub fn progress(value: f64, goal: Option<f64>) -> Option<f64> {
    goal.filter(|g| g.is_finite() && *g > 0.0)
        .map(|g| value * 100.0 / g)
}
/// Published flat-treadmill bands. Small gaps between the rounded bands use the
/// lower adjacent category; speeds above the walking table are not extrapolated.
pub fn met(speed: f64) -> Option<f64> {
    if !speed.is_finite() || speed <= 0.0 || speed > 8.851392 {
        return None;
    }
    Some(if speed < 1.609344 {
        2.1
    } else if speed < 1.9312128 {
        2.3
    } else if speed < 3.218688 {
        2.8
    } else if speed < 4.02336 {
        3.0
    } else if speed < 4.828032 {
        3.5
    } else if speed < 5.632704 {
        3.8
    } else if speed < 6.437376 {
        4.8
    } else if speed < 7.242048 {
        5.8
    } else if speed < 8.04672 {
        6.8
    } else {
        8.3
    })
}
