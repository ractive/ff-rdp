use clap::Parser;

mod cli;
mod dispatch;
mod error;
#[allow(dead_code)] // Skeleton — will be called once commands are implemented
mod output;
#[allow(dead_code)]
mod output_pipeline;

use cli::Cli;
use error::AppError;

fn main() {
    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let result = rt.block_on(dispatch::dispatch(&cli));

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
