mod config;
#[allow(dead_code)]
mod herdr;
#[allow(dead_code)]
mod lock;
#[allow(dead_code)]
mod tmux;

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some(cmd @ ("toggle" | "shell" | "tab-focused")) => {
            let _config = config::Config::load();
            eprintln!("herdr-tabshell: {cmd} not implemented yet");
            ExitCode::from(2)
        }
        _ => {
            eprintln!("usage: herdr-tabshell <toggle|shell|tab-focused>");
            ExitCode::from(64)
        }
    }
}
