//! Startup orchestration; argument parsing is delegated to cli.
use crate::runtime;
use crate::ui::{dialogs, tray};
use chrono_tz::Tz;
use std::path::PathBuf;
#[derive(Debug)]
pub struct Options {
    pub(crate) database_ownership: Option<crate::database_ownership::DatabaseOwnership>,
    pub(crate) database: PathBuf,
    pub(crate) timezone: Tz,
    pub(crate) device: Option<u64>,
    pub(crate) no_bluetooth: bool,
    pub(crate) smoke_seconds: Option<u32>,
    pub(crate) controls_enabled: bool,
    pub(crate) development: bool,
}

pub(crate) fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match crate::cli::parse(&args)? {
        crate::cli::Invocation::Info(message) => {
            dialogs::information(message);
            return Ok(());
        }
        crate::cli::Invocation::Replay {
            path,
            database,
            timezone,
        } => return crate::replay::replay_file(&path, &database, timezone),
        crate::cli::Invocation::Launch(args) => args,
    };
    let crate::cli::LaunchArgs {
        database,
        timezone,
        device,
        no_bluetooth,
        smoke_seconds,
        controls_enabled,
        portable,
        development,
    } = args;
    let database = match database {
        _ if development => runtime::development_database_path(
            &std::env::current_exe().map_err(|e| e.to_string())?,
        )?,
        Some(path) => path,
        None => runtime::database_path(
            &std::env::current_exe().map_err(|e| e.to_string())?,
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            portable,
        )?,
    };
    runtime::check_writable(&database)?;
    // Acquiring file ownership creates a missing database, so capture this first.
    let first_run = !database.try_exists().map_err(|e| e.to_string())?;
    // Reject another writer before reading schema/settings or creating UI.
    let database_ownership = crate::database_ownership::DatabaseOwnership::acquire(&database)?;
    let saved = runtime::saved_timezone(&database)?;
    let window_behavior = runtime::window_behavior(&database)?;
    let timezone = match (timezone, saved) {
        (Some(zone), _) | (None, Some(zone)) => zone,
        _ if no_bluetooth => chrono_tz::UTC,
        _ => {
            let zone = runtime::detected_timezone()?;
            if !dialogs::confirm(&format!(
                "Use {zone} for daily walking statistics?\n\nThis timezone stays with this database even if Windows later changes timezone. To choose another, cancel and launch with --timezone IANA_NAME."
            )) {
                return Ok(());
            }
            zone
        }
    };
    tray::run(
        Options {
            database_ownership: Some(database_ownership),
            database,
            timezone,
            device,
            no_bluetooth,
            smoke_seconds,
            controls_enabled,
            development,
        },
        first_run,
        window_behavior,
    )
}
