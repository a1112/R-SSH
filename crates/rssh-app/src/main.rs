mod bench;
mod cli;
mod config_lifecycle;
mod diagnostics;
mod local;
mod profiles;
mod scp;
mod self_test;
mod sftp;
mod ssh;
mod terminal_input;
mod terminal_modes;
mod terminal_runtime;
mod version;
mod visible_output;
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
    run_command(cli::parse_args(env::args()).map_err(io_error)?)
}

fn run_command(command: AppCommand) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match command {
        AppCommand::Bench(options) => {
            bench::print_bench(&options)?;
            Ok(ExitCode::SUCCESS)
        }
        AppCommand::Doctor(options) => {
            diagnostics::print_doctor(&options)?;
            Ok(ExitCode::SUCCESS)
        }
        AppCommand::Local(options) => local::run(&options).map(|status| pty_exit_code(&status)),
        AppCommand::Profile(options) => run_command(profiles::load_command(&options)?),
        AppCommand::ProfileCheck(options) => {
            profiles::print_profile_check(&options)?;
            Ok(ExitCode::SUCCESS)
        }
        AppCommand::ProfileInit(options) => {
            profiles::print_profile_init(&options)?;
            Ok(ExitCode::SUCCESS)
        }
        AppCommand::ProfileList(options) => {
            profiles::print_profile_list(&options)?;
            Ok(ExitCode::SUCCESS)
        }
        AppCommand::ProfileShow(options) => {
            profiles::print_profile_show(&options)?;
            Ok(ExitCode::SUCCESS)
        }
        AppCommand::Scp(options) => scp::run(&options).map(|status| pty_exit_code(&status)),
        AppCommand::SelfTest(options) => {
            self_test::print_self_test(&options)?;
            Ok(ExitCode::SUCCESS)
        }
        AppCommand::Sftp(options) => sftp::run(&options).map(|status| pty_exit_code(&status)),
        AppCommand::Ssh(options) => ssh::run(&options).map(|status| pty_exit_code(&status)),
        AppCommand::Version(options) => {
            version::print_version(&options)?;
            Ok(ExitCode::SUCCESS)
        }
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
