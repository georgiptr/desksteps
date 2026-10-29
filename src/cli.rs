//! Argument parsing is pure: no paths are opened and no dialogs or Bluetooth are created.
use crate::parse_address;
use chrono_tz::Tz;
use std::path::PathBuf;
#[derive(Debug)]
pub(crate) struct LaunchArgs {
    pub database: Option<PathBuf>,
    pub timezone: Option<Tz>,
    pub device: Option<u64>,
    pub no_bluetooth: bool,
    pub smoke_seconds: Option<u32>,
    pub controls_enabled: bool,
    pub portable: bool,
    pub development: bool,
}
#[derive(Debug)]
pub(crate) enum Invocation {
    Info(&'static str),
    Replay {
        path: PathBuf,
        database: PathBuf,
        timezone: Tz,
    },
    Launch(LaunchArgs),
}
pub(crate) fn parse(args: &[String]) -> Result<Invocation, String> {
    let mut database = None;
    let mut replay = None;
    let mut timezone = None;
    let mut device = None;
    let mut no_bluetooth = false;
    let mut smoke_seconds = None;
    let mut controls_enabled = true;
    let mut portable = false;
    let mut development = false;
    let mut i = 0;
    while i < args.len() {
        let name = &args[i];
        if name == "--development" {
            development = true;
            no_bluetooth = true;
        } else if name == "--no-bluetooth" {
            no_bluetooth = true;
        } else if name == "--no-controls" {
            controls_enabled = false;
        } else if name == "--portable" {
            portable = true;
        } else if name == "--version" || name == "--help" {
            return Ok(Invocation::Info(if name == "--version" {
                concat!("DeskSteps ", env!("CARGO_PKG_VERSION"))
            } else {
                "DeskSteps: WalkingPad Z1\n\ncargo run: normal app (Bluetooth enabled)\ncargo dev / --development: offline development simulator, isolated storage\n--portable: store data beside the executable\n--database PATH: explicit data location (normal/preview only)\n--timezone IANA_NAME: timezone for a new database\n--no-bluetooth: offline preview\n--no-controls: record only\n--replay FILE --database PATH: offline replay\n\nFirst launch opens the main window. Settings controls later launch visibility and close behavior. Exit is always in the tray menu."
            }));
        } else {
            i += 1;
            let value = args
                .get(i)
                .ok_or_else(|| format!("Missing value for {name}"))?;
            match name.as_str() {
                "--database" => database = Some(PathBuf::from(value)),
                "--replay" => replay = Some(PathBuf::from(value)),
                "--timezone" => {
                    timezone =
                        Some(value.parse::<Tz>().map_err(|_| {
                            "Use an IANA timezone such as Europe/Helsinki".to_string()
                        })?);
                }
                "--device" => device = Some(parse_address(value)?),
                "--smoke-seconds" => {
                    smoke_seconds = Some(
                        value
                            .parse::<u32>()
                            .map_err(|e| e.to_string())?
                            .clamp(1, 60),
                    );
                    no_bluetooth = true;
                }
                _ => return Err(format!("Unknown option: {name}")),
            }
        }
        i += 1;
    }
    if development && (database.is_some() || replay.is_some() || portable || device.is_some()) {
        return Err("Development mode uses isolated development storage and a synthetic pad; --database, --replay, --portable and --device cannot override it".into());
    }
    if let Some(path) = replay {
        let db =
            database.ok_or("Replay requires --database pointing to a separate test database")?;
        return Ok(Invocation::Replay {
            path,
            database: db,
            timezone: timezone.unwrap_or(chrono_tz::UTC),
        });
    }
    if smoke_seconds.is_some() && database.is_none() && !development {
        return Err("Smoke tests require an explicit test --database".into());
    }
    Ok(Invocation::Launch(LaunchArgs {
        database,
        timezone,
        device,
        no_bluetooth,
        smoke_seconds,
        controls_enabled,
        portable,
        development,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn arguments(value: &[&str]) -> Vec<String> {
        value.iter().map(|v| v.to_string()).collect()
    }
    fn launch(value: &[&str]) -> LaunchArgs {
        match parse(&arguments(value)).unwrap() {
            Invocation::Launch(args) => args,
            _ => panic!("Expected launch"),
        }
    }
    #[test]
    fn defaults_preserve_normal_launch_without_a_device_override() {
        let a = launch(&[]);
        assert!(a.controls_enabled);
        assert!(!a.no_bluetooth);
        assert!(!a.development);
        assert!(!a.portable);
        assert!(a.database.is_none());
        assert!(a.device.is_none());
        assert!(a.timezone.is_none());
        assert!(a.smoke_seconds.is_none());
    }
    #[test]
    fn storage_timezone_and_device_arguments_are_typed() {
        let a = launch(&[
            "--portable",
            "--database",
            "synthetic.sqlite",
            "--timezone",
            "Europe/Helsinki",
            "--device",
            "11:22:33:44:55:66",
            "--no-controls",
            "--no-bluetooth",
        ]);
        assert!(a.portable && a.no_bluetooth && !a.controls_enabled);
        assert_eq!(a.device, Some(0x112233445566));
        assert_eq!(a.timezone, Some(chrono_tz::Europe::Helsinki));
        assert_eq!(a.database, Some(PathBuf::from("synthetic.sqlite")));
    }
    #[test]
    fn smoke_requires_explicit_database_and_always_disables_bluetooth() {
        assert!(
            parse(&arguments(&["--smoke-seconds", "1"]))
                .unwrap_err()
                .contains("explicit")
        );
        for (value, expected) in [("0", 1), ("1", 1), ("60", 60), ("999", 60)] {
            let a = launch(&["--smoke-seconds", value, "--database", "test.sqlite"]);
            assert_eq!(a.smoke_seconds, Some(expected));
            assert!(a.no_bluetooth);
        }
    }
    #[test]
    fn replay_requires_a_database_and_defaults_to_utc() {
        assert!(
            parse(&arguments(&["--replay", "test.jsonl"]))
                .unwrap_err()
                .contains("Replay requires")
        );
        match parse(&arguments(&[
            "--replay",
            "test.jsonl",
            "--database",
            "test.sqlite",
        ]))
        .unwrap()
        {
            Invocation::Replay {
                path,
                database,
                timezone,
            } => {
                assert_eq!(path, PathBuf::from("test.jsonl"));
                assert_eq!(database, PathBuf::from("test.sqlite"));
                assert_eq!(timezone, chrono_tz::UTC);
            }
            _ => panic!("Expected replay"),
        }
    }
    #[test]
    fn malformed_arguments_fail_before_any_io() {
        for args in [
            vec!["--database"],
            vec!["--timezone", "Invalid"],
            vec!["--device", "invalid"],
            vec!["--smoke-seconds", "invalid"],
            vec!["--unknown", "value"],
        ] {
            assert!(parse(&arguments(&args)).is_err(), "{args:?}");
        }
    }
    #[test]
    fn help_and_version_stop_parsing_at_the_same_boundary() {
        assert!(
            matches!(parse(&arguments(&["--help","--unknown"])).unwrap(),Invocation::Info(text) if text.contains("--portable"))
        );
        assert!(matches!(
            parse(&arguments(&["--version"])).unwrap(),
            Invocation::Info(text) if text == concat!("DeskSteps ", env!("CARGO_PKG_VERSION"))
        ));
        assert_eq!(
            launch(&["--database", "first", "--database", "second"]).database,
            Some(PathBuf::from("second"))
        );
    }
    #[test]
    fn development_forces_offline_and_rejects_storage_and_device_overrides_in_any_order() {
        let a = launch(&["--development"]);
        assert!(a.development && a.no_bluetooth && a.controls_enabled);
        assert!(a.database.is_none());
        for other in [
            vec!["--database", "personal.sqlite"],
            vec!["--device", "11:22:33:44:55:66"],
            vec!["--portable"],
            vec!["--replay", "synthetic.jsonl"],
        ] {
            for first in [true, false] {
                let mut args = other.clone();
                if first {
                    args.insert(0, "--development");
                } else {
                    args.push("--development");
                }
                assert!(
                    parse(&arguments(&args))
                        .unwrap_err()
                        .contains("cannot override")
                );
            }
        }
        assert!(launch(&["--development", "--smoke-seconds", "1"]).no_bluetooth);
    }
}
