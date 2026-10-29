//! DeskSteps application. See docs/ARCHITECTURE.md for ownership and boundaries.
mod accounting;
mod appearance;
mod application;
mod bootstrap;
mod cli;
mod control;
mod database_ownership;
mod device;
mod event;
mod identity;
mod protocol;
mod replay;
mod runtime;
mod shortcuts;
mod statistics;
mod storage;
mod ui;
mod window_behavior;
mod windows_startup;
use bootstrap::Options;
use device::{address, parse_address};
pub fn launch() {
    if let Err(error) = bootstrap::run() {
        if std::env::args().any(|arg| arg == "--replay" || arg == "--smoke-seconds") {
            eprintln!("{error}");
        } else {
            ui::dialogs::error_dialog(&error);
        }
        std::process::exit(1);
    }
}
