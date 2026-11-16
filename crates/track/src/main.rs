//! `axonhand-track`: see the library documentation and [`axonhand_track::cli::USAGE`].
//!
//! Exit status 0 on success, 2 when the video cannot be used (the reason is on standard error
//! after the marker the server looks for, written for a clinician), 1 for a fault in the models
//! or the software, 64 for wrong arguments.

use std::path::PathBuf;
use std::process::ExitCode;

use axonhand_track::cli::{run, Args, USAGE};

/// Marks the line of standard error that is meant for the person.
const MESSAGE_PREFIX: &str = "axonhand-message: ";

fn main() -> ExitCode {
    let default_models =
        std::env::var_os("AXONHAND_MODELS").map_or_else(|| PathBuf::from("models"), PathBuf::from);
    let args = match Args::parse(std::env::args().skip(1), default_models) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(64);
        }
    };
    match run(&args, std::io::stdin().lock()) {
        Ok(summary) => {
            for line in summary.lines() {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(error) if error.is_for_the_person() => {
            eprintln!("{MESSAGE_PREFIX}{error}");
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
