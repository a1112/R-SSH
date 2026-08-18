use std::process::ExitCode;

use rssh_diagnostics::{LAUNCHER_USAGE, LauncherCliError, LauncherOptions};

fn main() -> ExitCode {
    match LauncherOptions::parse(std::env::args()) {
        Err(LauncherCliError::HelpRequested) => {
            println!("{LAUNCHER_USAGE}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
        Ok(_) => {
            eprintln!("scenario execution is not connected yet");
            ExitCode::from(2)
        }
    }
}
