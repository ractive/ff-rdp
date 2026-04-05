use crate::cli::args::{Cli, Command};
use crate::error::AppError;
use crate::output_pipeline::OutputPipeline;

#[allow(clippy::unused_async)] // Will be async once commands are implemented
pub async fn dispatch(cli: &Cli) -> Result<(), AppError> {
    let _pipeline = OutputPipeline::new(cli.jq.clone());

    match &cli.command {
        Command::Tabs => Err(AppError::User(
            "tabs: not yet implemented (iteration 2)".into(),
        )),
        Command::Navigate { url: _ } => Err(AppError::User(
            "navigate: not yet implemented (iteration 2)".into(),
        )),
        Command::Eval { script: _ } => Err(AppError::User(
            "eval: not yet implemented (iteration 2)".into(),
        )),
        Command::PageText => Err(AppError::User(
            "page-text: not yet implemented (iteration 5)".into(),
        )),
        Command::Console => Err(AppError::User(
            "console: not yet implemented (iteration 4)".into(),
        )),
        Command::Screenshot { output: _ } => Err(AppError::User(
            "screenshot: not yet implemented (iteration 7)".into(),
        )),
        Command::Reload => Err(AppError::User(
            "reload: not yet implemented (iteration 3)".into(),
        )),
        Command::Back => Err(AppError::User(
            "back: not yet implemented (iteration 3)".into(),
        )),
        Command::Forward => Err(AppError::User(
            "forward: not yet implemented (iteration 3)".into(),
        )),
    }
}
