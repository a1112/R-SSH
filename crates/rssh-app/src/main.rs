mod cli;
mod local;

use std::{env, process::ExitCode};

use cli::AppCommand;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match cli::parse_args(env::args()).map_err(io_error)? {
        AppCommand::Local(options) => local::run(&options),
        AppCommand::Help => {
            print!("{}", cli::help_text());
            Ok(())
        }
    }
}

fn io_error(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}
