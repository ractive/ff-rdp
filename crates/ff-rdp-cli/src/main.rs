use clap::Parser;

mod cli;
mod commands;
mod dispatch;
mod error;
mod output;
mod output_pipeline;
mod tab_target;

use cli::Cli;
use error::AppError;

fn main() {
    let cli = Cli::parse();
    let result = dispatch::dispatch(&cli);
    match result {
        Ok(()) => {}
        Err(AppError::User(msg)) => {
            eprintln!("error: {msg}");
            std::process::exit(1);
        }
        Err(AppError::Internal(err)) => {
            eprintln!("internal error: {err:#}");
            std::process::exit(2);
        }
        Err(AppError::Exit(code)) => {
            std::process::exit(code);
        }
    }
}
