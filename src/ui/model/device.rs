//! Device choices and enable rules shared by the tray menu and the Settings page.
use crate::{address, application::View};

/// Discovered candidates, plus the saved device when it was not discovered this time.
pub fn choices(view: &View) -> Vec<(u64, String)> {
    let mut choices = view
        .candidates
        .iter()
        .map(|candidate| {
            let name = if candidate.name.trim().is_empty() {
                view.selected_name
                    .as_deref()
                    .filter(|_| view.selected == Some(candidate.address))
                    .unwrap_or("Saved device")
            } else {
                &candidate.name
            };
            (
                candidate.address,
                format!("{name} ({})", address(candidate.address)),
            )
        })
        .collect::<Vec<_>>();
    if let Some(selected) = view.selected
        && !choices.iter().any(|(candidate, _)| *candidate == selected)
    {
        choices.push((
            selected,
            format!(
                "{} ({})",
                view.selected_name.as_deref().unwrap_or("Saved device"),
                address(selected)
            ),
        ));
    }
    choices
}
/// Connection commands are available unless this is a Bluetooth-free preview.
pub fn available(view: &View) -> bool {
    !view.no_bluetooth || view.development
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::Candidate;
    #[test]
    fn saved_device_is_added_only_when_not_discovered() {
        let found = |a| Candidate {
            address: a,
            name: "Pad".into(),
        };
        let v = View {
            candidates: vec![found(1), found(2)],
            selected: Some(2),
            ..Default::default()
        };
        assert_eq!(choices(&v).len(), 2);
        let v = View {
            selected: Some(3),
            ..v
        };
        let c = choices(&v);
        assert_eq!(c.len(), 3);
        assert!(c[2].1.starts_with("Saved device ("));
        assert!(c[0].1.starts_with("Pad ("));
        assert!(choices(&View::default()).is_empty());
    }
    #[test]
    fn connection_commands_need_bluetooth_or_development() {
        assert!(available(&View::default()));
        assert!(!available(&View {
            no_bluetooth: true,
            ..Default::default()
        }));
        assert!(available(&View {
            no_bluetooth: true,
            development: true,
            ..Default::default()
        }));
    }
    #[test]
    fn remembered_name_survives_missing_discovery_and_nameless_advertisements() {
        let mut view = View {
            selected: Some(1),
            selected_name: Some("Remembered Pad".into()),
            ..Default::default()
        };
        assert_eq!(choices(&view)[0].1, "Remembered Pad (00:00:00:00:00:01)");
        view.candidates.push(Candidate {
            address: 1,
            name: String::new(),
        });
        assert_eq!(choices(&view)[0].1, "Remembered Pad (00:00:00:00:00:01)");
        view.candidates[0].name = "New Pad Name".into();
        assert_eq!(choices(&view)[0].1, "New Pad Name (00:00:00:00:00:01)");
    }
}
