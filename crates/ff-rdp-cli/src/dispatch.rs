use crate::cli::args::{Cli, Command};
use crate::error::AppError;
use crate::output_pipeline::OutputPipeline;

#[allow(clippy::unused_async)] // Will be async once commands are implemented
pub async fn dispatch(cli: &Cli) -> Result<(), AppError> {
    let _pipeline = OutputPipeline::new(cli.jq.clone());

    match &cli.command {
        Command::Tabs => todo!("iteration 2"),
        Command::Navigate { url: _ } => todo!("iteration 2"),
        Command::Eval { script: _ } => todo!("iteration 2"),
        Command::PageText => todo!("iteration 5"),
        Command::Console => todo!("iteration 4"),
        Command::Screenshot { output: _ } => todo!("iteration 7"),
        Command::Reload => todo!("iteration 3"),
        Command::Back => todo!("iteration 3"),
        Command::Forward => todo!("iteration 3"),
    }
}
