use crate::cli::args::{Cli, Command, PerfCommand};
use crate::commands;
use crate::commands::nav_action::NavAction;
use crate::daemon::server;
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
        Command::Network { filter, method } => {
            commands::network::run(cli, filter.as_deref(), method.as_deref())
        }
        Command::Perf {
            perf_command,
            entry_type,
            filter,
        } => match perf_command {
            Some(PerfCommand::Vitals) => commands::perf::run_vitals(cli),
            None => commands::perf::run(cli, entry_type, filter.as_deref()),
        },
        Command::Click { selector } => commands::click::run(cli, selector),
        Command::Type {
            selector,
            text,
            clear,
        } => commands::type_text::run(cli, selector, text, *clear),
        Command::Wait {
            selector,
            text,
            eval,
            wait_timeout,
        } => commands::wait::run(
            cli,
            &commands::wait::WaitOptions {
                selector: selector.as_deref(),
                text: text.as_deref(),
                eval: eval.as_deref(),
                wait_timeout: *wait_timeout,
            },
        ),
        Command::Cookies { name } => commands::cookies::run(cli, name.as_deref()),
        Command::Storage { storage_type, key } => {
            commands::storage::run(cli, storage_type, key.as_deref())
        }
        Command::Inspect { actor_id, depth } => commands::inspect::run(cli, actor_id, *depth),
        Command::Sources { filter, pattern } => {
            commands::sources::run(cli, filter.as_deref(), pattern.as_deref())
        }
        Command::Screenshot { output } => commands::screenshot::run(cli, output.as_deref()),
        Command::Launch {
            headless,
            profile,
            temp_profile,
            debug_port,
        } => commands::launch::run(
            cli,
            *headless,
            profile.as_deref(),
            *temp_profile,
            *debug_port,
        ),
        Command::Daemon => {
            server::run_daemon(&cli.host, cli.port, cli.daemon_timeout).map_err(AppError::Internal)
        }
    }
}
