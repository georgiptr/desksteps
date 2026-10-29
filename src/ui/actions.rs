//! Shared availability checks and command submission for every native surface.
use crate::{
    application::{CommandSender, SnapshotReader, UserCommand as Input},
    control::Command,
};
pub(crate) fn request(sender: &CommandSender, view: &SnapshotReader, command: Command) {
    if let Err(error) = request_checked(sender, view, command) {
        crate::ui::dialogs::error_dialog(&error);
    }
}
fn request_checked(
    sender: &CommandSender,
    view: &SnapshotReader,
    command: Command,
) -> Result<(), String> {
    let v = view.snapshot();
    if !v.connected
        || !v.controls_enabled
        || (v.no_bluetooth && !v.development)
        || (command != Command::Pause && v.control_uncertain)
        || (command.is_speed() && !v.speed_available)
        || (command == Command::StartResume && v.control_busy)
    {
        return Ok(());
    }
    sender.try_send(Input::Control {
        command,
        generation: v.generation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{View, testing::channels};
    fn available() -> View {
        View {
            connected: true,
            controls_enabled: true,
            speed_available: true,
            generation: 7,
            ..Default::default()
        }
    }
    #[test]
    fn every_surface_blocks_speed_during_start_pause_or_uncertainty() {
        for uncertain in [false, true] {
            let initial = View {
                control_busy: !uncertain,
                control_uncertain: uncertain,
                speed_available: false,
                ..available()
            };
            let (sender, view, _, rx) = channels(1, initial);
            request_checked(&sender, &view, Command::Increase).unwrap();
            drop(sender);
            assert!(rx.recv().is_err());
        }
    }
    #[test]
    fn speed_clicks_are_submitted_while_a_speed_write_is_busy() {
        let (sender, view, _, rx) = channels(
            4,
            View {
                control_busy: true,
                adjustable_speed: Some(3.1),
                fresh_speed: Some(3.0),
                ..available()
            },
        );
        for command in [Command::Increase, Command::Increase, Command::Decrease] {
            request_checked(&sender, &view, command).unwrap();
            assert!(
                matches!(rx.recv().unwrap(), Input::Control { command: received, generation: 7 } if received == command)
            );
        }
        request_checked(&sender, &view, Command::StartResume).unwrap();
        drop(sender);
        assert!(rx.recv().is_err());
    }
    #[test]
    fn pause_remains_available_while_an_exchange_is_busy_and_uncertain() {
        let (sender, view, _, rx) = channels(
            1,
            View {
                control_busy: true,
                control_uncertain: true,
                ..available()
            },
        );
        request_checked(&sender, &view, Command::Pause).unwrap();
        assert!(matches!(
            rx.recv().unwrap(),
            Input::Control {
                command: Command::Pause,
                generation: 7
            }
        ));
    }
    #[test]
    fn start_submits_once_with_the_current_generation_for_unknown_or_known_state() {
        for development in [false, true] {
            for fresh_speed in [None, Some(0.0)] {
                let (sender, view, _, rx) = channels(
                    2,
                    View {
                        development,
                        no_bluetooth: development,
                        fresh_speed,
                        ..available()
                    },
                );
                request(&sender, &view, Command::StartResume);
                drop(sender);
                assert!(matches!(
                    rx.recv().unwrap(),
                    Input::Control {
                        command: Command::StartResume,
                        generation: 7
                    }
                ));
                assert!(rx.recv().is_err());
            }
        }
    }
    #[test]
    fn unavailable_controls_do_not_enqueue() {
        for initial in [
            View {
                connected: false,
                ..available()
            },
            View {
                controls_enabled: false,
                ..available()
            },
            View {
                no_bluetooth: true,
                development: false,
                ..available()
            },
            View {
                control_busy: true,
                ..available()
            },
            View {
                control_uncertain: true,
                ..available()
            },
        ] {
            let (sender, view, _, rx) = channels(1, initial);
            request_checked(&sender, &view, Command::StartResume).unwrap();
            drop(sender);
            assert!(rx.recv().is_err());
        }
    }
    #[test]
    fn submission_failure_is_returned_without_retry() {
        let (sender, view, _, rx) = channels(1, available());
        sender.try_send(Input::Increment(1)).unwrap();
        assert!(
            request_checked(&sender, &view, Command::Pause)
                .unwrap_err()
                .contains("busy")
        );
        assert!(matches!(rx.recv().unwrap(), Input::Increment(1)));
    }
}
