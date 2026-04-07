use crate::cli::args::{A11yCommand, Cli, Command, DomCommand, PerfCommand};
use crate::commands;
use crate::commands::nav_action::NavAction;
use crate::daemon::server;
use crate::error::AppError;

pub fn dispatch(cli: &Cli) -> Result<(), AppError> {
    match &cli.command {
        Command::Tabs => commands::tabs::run(cli),
        Command::Navigate {
            url,
            with_network,
            wait_text,
            wait_selector,
            wait_timeout,
        } => {
            let wait_opts = commands::navigate::WaitAfterNav {
                wait_text: wait_text.as_deref(),
                wait_selector: wait_selector.as_deref(),
                wait_timeout: *wait_timeout,
            };
            if *with_network {
                commands::navigate::run_with_network(cli, url, &wait_opts)
            } else {
                commands::navigate::run(cli, url, &wait_opts)
            }
        }
        Command::Eval { script } => commands::eval::run(cli, script),
        Command::Reload => commands::nav_action::run(cli, NavAction::Reload),
        Command::Back => commands::nav_action::run(cli, NavAction::Back),
        Command::Forward => commands::nav_action::run(cli, NavAction::Forward),
        Command::PageText => commands::page_text::run(cli),
        Command::Dom {
            dom_command,
            selector,
            outer_html: _,
            inner_html,
            text,
            attrs,
            count,
        } => match dom_command {
            Some(DomCommand::Stats) => commands::dom::run_stats(cli),
            Some(DomCommand::Tree {
                selector,
                depth,
                max_chars,
            }) => commands::dom_tree::run(cli, selector.as_deref(), *depth, *max_chars),
            None => {
                let sel = selector.as_deref().ok_or_else(|| {
                    AppError::User("dom requires a CSS selector argument".to_string())
                })?;
                if *count {
                    commands::dom::run_count(cli, sel)
                } else {
                    let mode = if *inner_html {
                        commands::dom::OutputMode::InnerHtml
                    } else if *text {
                        commands::dom::OutputMode::Text
                    } else if *attrs {
                        commands::dom::OutputMode::Attrs
                    } else {
                        commands::dom::OutputMode::OuterHtml
                    };
                    commands::dom::run(cli, sel, mode)
                }
            }
        },
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
            group_by,
        } => match perf_command {
            Some(PerfCommand::Vitals) => commands::perf::run_vitals(cli),
            Some(PerfCommand::Summary) => commands::perf::run_summary(cli),
            Some(PerfCommand::Audit) => commands::perf::run_audit(cli),
            Some(PerfCommand::Compare { urls, label }) => {
                commands::perf_compare::run(cli, urls, label.as_ref().map(Vec::as_slice))
            }
            None => {
                if group_by.as_deref() == Some("domain") {
                    commands::perf::run_group_by_domain(cli, entry_type, filter.as_deref())
                } else if let Some(val) = group_by.as_deref() {
                    Err(AppError::User(format!(
                        "unsupported --group-by value {val:?}; supported: \"domain\""
                    )))
                } else {
                    commands::perf::run(cli, entry_type, filter.as_deref())
                }
            }
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
        Command::A11y {
            a11y_command,
            depth,
            max_chars,
            selector,
            interactive,
        } => match a11y_command {
            Some(A11yCommand::Contrast {
                selector: contrast_selector,
                fail_only,
            }) => commands::a11y_contrast::run(cli, contrast_selector.as_deref(), *fail_only),
            None => commands::a11y::run(cli, *depth, *max_chars, selector.as_deref(), *interactive),
        },
        Command::Cookies { name } => commands::cookies::run(cli, name.as_deref()),
        Command::Storage { storage_type, key } => {
            commands::storage::run(cli, storage_type, key.as_deref())
        }
        Command::Inspect { actor_id, depth } => commands::inspect::run(cli, actor_id, *depth),
        Command::Sources { filter, pattern } => {
            commands::sources::run(cli, filter.as_deref(), pattern.as_deref())
        }
        Command::Screenshot { output, base64 } => {
            commands::screenshot::run(cli, output.as_deref(), *base64)
        }
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
        Command::Styles {
            selector,
            applied,
            layout,
        } => {
            if *applied {
                commands::styles::run_applied(cli, selector)
            } else if *layout {
                commands::styles::run_layout(cli, selector)
            } else {
                commands::styles::run(cli, selector)
            }
        }
        Command::Geometry { selectors } => commands::geometry::run(cli, selectors),
        Command::Responsive { selectors, widths } => {
            commands::responsive::run(cli, selectors, widths)
        }
        Command::Snapshot { depth, max_chars } => commands::snapshot::run(cli, *depth, *max_chars),
        Command::Recipes => {
            commands::recipes::run();
            Ok(())
        }
        Command::LlmHelp => commands::llm_help::run(cli),
        Command::Daemon => {
            server::run_daemon(&cli.host, cli.port, cli.daemon_timeout).map_err(AppError::Internal)
        }
    }
}
