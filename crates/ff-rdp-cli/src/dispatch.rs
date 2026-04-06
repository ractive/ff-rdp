use crate::cli::args::{Cli, Command};
use crate::commands;
use crate::commands::nav_action::NavAction;
use crate::error::AppError;

pub fn dispatch(cli: &Cli) -> Result<(), AppError> {
    match &cli.command {
        Command::Tabs => commands::tabs::run(cli),
        Command::Navigate { url, with_network } => {
            if *with_network {
                commands::navigate::run_with_network(cli, url)
            } else {
                commands::navigate::run(cli, url)
            }
        }
        Command::Eval { script } => commands::eval::run(cli, script),
        Command::Reload => commands::nav_action::run(cli, NavAction::Reload),
        Command::Back => commands::nav_action::run(cli, NavAction::Back),
        Command::Forward => commands::nav_action::run(cli, NavAction::Forward),
        Command::PageText => commands::page_text::run(cli),
        Command::Dom {
            selector,
            outer_html: _,
            inner_html,
            text,
            attrs,
        } => {
            let mode = if *inner_html {
                commands::dom::OutputMode::InnerHtml
            } else if *text {
                commands::dom::OutputMode::Text
            } else if *attrs {
                commands::dom::OutputMode::Attrs
            } else {
                // default (including explicit --outer-html)
                commands::dom::OutputMode::OuterHtml
            };
            commands::dom::run(cli, selector, mode)
        }
        Command::Console { level, pattern } => {
            commands::console::run(cli, level.as_deref(), pattern.as_deref())
        }
        Command::Network {
            filter,
            method,
            cached,
        } => {
            if *cached {
                commands::network::run_cached(cli, filter.as_deref(), method.as_deref())
            } else {
                commands::network::run(cli, filter.as_deref(), method.as_deref())
            }
        }
        Command::Screenshot { output: _ } => Err(AppError::User(
            "screenshot: not yet implemented (iteration 7)".into(),
        )),
    }
}
