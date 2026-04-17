use crate::cli::args::{A11yCommand, Cli, Command, DomCommand, PerfCommand, ScrollCommand};
use crate::commands;
use crate::commands::nav_action::NavAction;
use crate::daemon::server;
use crate::error::AppError;

/// Dispatch a CLI command to its handler.
///
/// # Connection routing
///
/// Most commands connect via the daemon proxy when one is available.  The
/// following commands always bypass the daemon and connect directly to
/// Firefox (`connect_direct`), because their protocol interactions are
/// incompatible with the daemon's watcher subscription or message routing:
///
/// | Command      | Reason                                          |
/// |--------------|-------------------------------------------------|
/// | `screenshot` | Two-step capture protocol conflicts with watcher|
/// | `cookies`    | `watchResources("cookies")` intercepted by daemon watcher |
/// | `storage`    | Same watcher interception issue as cookies       |
/// | `a11y`       | Accessibility walker actors conflict with proxy  |
/// | `sources`    | Thread actor `sources` method conflicts with proxy |
///
/// Commands that **require** daemon event buffering or streaming:
///
/// | Command                   | Reason                              |
/// |---------------------------|-------------------------------------|
/// | `console --follow`        | Streams buffered console events     |
/// | `network --follow`        | Streams buffered network events     |
/// | `navigate --with-network` | Captures network during navigation  |
/// | `network` (no --follow)   | Drains buffered network events      |
/// | `console` (no --follow)   | Drains buffered console events      |
///
/// All other commands use `connect_and_get_target`, which routes through
/// the daemon when available or falls back to direct connection.
///
/// **Note:** Non-streaming `network` and `console` intentionally use the
/// daemon path because they drain events the daemon has been buffering in
/// the background — this is the daemon's primary value proposition for
/// these commands.
pub fn dispatch(cli: &Cli) -> Result<(), AppError> {
    match &cli.command {
        Command::Tabs => commands::tabs::run(cli),
        Command::Navigate {
            url,
            with_network,
            network_timeout,
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
                commands::navigate::run_with_network(cli, url, &wait_opts, *network_timeout)
            } else {
                commands::navigate::run(cli, url, &wait_opts)
            }
        }
        Command::Eval {
            script,
            file,
            stdin,
            stringify,
        } => commands::eval::run(cli, script.as_deref(), file.as_deref(), *stdin, *stringify),
        Command::Reload {
            wait_idle,
            idle_ms,
            reload_timeout,
        } => {
            if *wait_idle {
                commands::nav_action::run_reload_wait_idle(cli, *idle_ms, *reload_timeout)
            } else {
                commands::nav_action::run(cli, NavAction::Reload)
            }
        }
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
            text_attrs,
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
                    } else if *text_attrs {
                        commands::dom::OutputMode::TextAttrs
                    } else {
                        commands::dom::OutputMode::OuterHtml
                    };
                    commands::dom::run(cli, sel, mode)
                }
            }
        },
        Command::Console {
            level,
            pattern,
            follow,
        } => {
            if *follow {
                commands::console::run_follow(cli, level.as_deref(), pattern.as_deref())
            } else {
                commands::console::run(cli, level.as_deref(), pattern.as_deref())
            }
        }
        Command::Network {
            filter,
            method,
            follow,
        } => {
            if *follow {
                commands::network::run_follow(cli, filter.as_deref(), method.as_deref())
            } else {
                commands::network::run(cli, filter.as_deref(), method.as_deref())
            }
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
            Some(A11yCommand::Summary) => commands::a11y_summary::run(cli),
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
        Command::Screenshot {
            output,
            base64,
            full_page,
            viewport_height,
        } => commands::screenshot::run(
            cli,
            &commands::screenshot::ScreenshotOpts {
                output_path: output.as_deref(),
                base64_mode: *base64,
                full_page: *full_page,
                viewport_height: *viewport_height,
            },
        ),
        Command::Launch {
            headless,
            profile,
            temp_profile,
            debug_port,
            auto_consent,
        } => commands::launch::run(
            cli,
            *headless,
            profile.as_deref(),
            *temp_profile,
            *debug_port,
            *auto_consent,
        ),
        Command::Computed {
            selector,
            prop,
            all,
        } => commands::computed::run(cli, selector, prop.as_deref(), *all),
        Command::Styles {
            selector,
            applied,
            layout,
            properties,
        } => {
            if *applied {
                commands::styles::run_applied(cli, selector)
            } else if *layout {
                commands::styles::run_layout(cli, selector)
            } else {
                commands::styles::run(cli, selector, properties.as_deref())
            }
        }
        Command::Geometry {
            selectors,
            visible_only,
        } => commands::geometry::run(cli, selectors, *visible_only),
        Command::Responsive { selectors, widths } => {
            commands::responsive::run(cli, selectors, widths)
        }
        Command::Snapshot { depth, max_chars } => commands::snapshot::run(cli, *depth, *max_chars),
        Command::Recipes => {
            commands::recipes::run();
            Ok(())
        }
        Command::LlmHelp => commands::llm_help::run(cli),
        Command::Scroll { scroll_command } => match scroll_command {
            ScrollCommand::To {
                selector,
                block,
                smooth,
            } => commands::scroll::run_to(cli, selector, *block, *smooth),
            ScrollCommand::By {
                dx,
                dy,
                page_down,
                page_up,
                smooth,
            } => commands::scroll::run_by(cli, *dx, *dy, *page_down, *page_up, *smooth),
            ScrollCommand::Container {
                selector,
                dx,
                dy,
                to_end,
                to_start,
            } => commands::scroll::run_container(cli, selector, *dx, *dy, *to_end, *to_start),
            ScrollCommand::Until {
                selector,
                direction,
                timeout,
            } => commands::scroll::run_until(cli, selector, direction, *timeout),
            ScrollCommand::Text { text } => commands::scroll::run_text(cli, text),
            ScrollCommand::Top => commands::scroll::run_top(cli),
            ScrollCommand::Bottom => commands::scroll::run_bottom(cli),
        },
        Command::Daemon => {
            server::run_daemon(&cli.host, cli.port, cli.daemon_timeout).map_err(AppError::Internal)
        }
    }
}
