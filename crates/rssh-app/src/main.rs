mod cli;
mod local;
mod terminal_input;
mod terminal_runtime;
mod window;

use std::{env, process::ExitCode};

use cli::AppCommand;
use rssh_pty::PtyExitStatus;

fn main() -> ExitCode {
    match run() {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli::parse_args(env::args()).map_err(io_error)? {
        AppCommand::Local(options) => local::run(&options).map(|status| pty_exit_code(&status)),
        AppCommand::Window(options) => {
            window::run(&options)?;
            Ok(ExitCode::SUCCESS)
        }
        AppCommand::Help => {
            print!("{}", cli::help_text());
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn pty_exit_code(status: &PtyExitStatus) -> ExitCode {
    ExitCode::from(pty_status_code(status))
}

fn pty_status_code(status: &PtyExitStatus) -> u8 {
    if status.success() {
        return 0;
    }

    u8::try_from(status.exit_code()).unwrap_or(1)
}

fn io_error(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use rssh_pty::PtyExitStatus;

    use super::pty_status_code;

    #[test]
    fn maps_pty_success_to_process_success() {
        assert_eq!(pty_status_code(&PtyExitStatus::from_exit_code(0)), 0);
    }

    #[test]
    fn maps_pty_failure_to_process_exit_code() {
        assert_eq!(pty_status_code(&PtyExitStatus::from_exit_code(7)), 7);
    }

    #[test]
    fn maps_large_pty_failure_to_generic_process_failure() {
        assert_eq!(pty_status_code(&PtyExitStatus::from_exit_code(300)), 1);
    }
}
