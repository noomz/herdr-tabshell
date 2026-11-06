mod command;
mod config;
mod herdr;
mod lock;
mod tmux;

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("toggle") => command_exit(command::run_toggle()),
        Some("shell") => command_exit(command::run_shell()),
        Some("tab-focused") => {
            eprintln!("herdr-tabshell: tab-focused not implemented yet");
            ExitCode::from(2)
        }
        _ => {
            eprintln!("usage: herdr-tabshell <toggle|shell|tab-focused>");
            ExitCode::from(64)
        }
    }
}

fn command_exit(result: std::io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "herdr-tabshell: {}",
                error.to_string().replace(['\n', '\r'], " ")
            );
            ExitCode::FAILURE
        }
    }
}
