use crate::cli::args::{Cli, Command};
use crate::commands;
use crate::commands::nav_action::NavAction;
use crate::error::AppError;

pub fn dispatch(cli: &Cli) -> Result<(), AppError> {
    match &cli.command {
        Command::Tabs => commands::tabs::run(cli),
        Command::Navigate { url } => commands::navigate::run(cli, url),
        Command::Eval { script } => commands::eval::run(cli, script),
        Command::Reload => commands::nav_action::run(cli, NavAction::Reload),
        Command::Back => commands::nav_action::run(cli, NavAction::Back),
        Command::Forward => commands::nav_action::run(cli, NavAction::Forward),
        Command::PageText => Err(AppError::User(
            "page-text: not yet implemented (iteration 5)".into(),
        )),
        Command::Console => Err(AppError::User(
            "console: not yet implemented (iteration 4)".into(),
        )),
        Command::Screenshot { output: _ } => Err(AppError::User(
            "screenshot: not yet implemented (iteration 7)".into(),
        )),
    }
}
