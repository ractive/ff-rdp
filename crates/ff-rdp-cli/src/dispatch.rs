use crate::cli::args::{
    A11yCommand, Cli, Command, DaemonCommand, DomCommand, PerfCommand, RecordCommand, ScrollCommand,
};
use crate::commands;
use crate::commands::index::IndexOpts;
use crate::commands::js_helpers::DispatchMode;
use crate::commands::nav_action::NavAction;
use crate::daemon::registry;
use crate::daemon::server;
use crate::error::AppError;
use crate::script::format::{
    ElementStep, ElementTarget, EvalStep, NavigateStep, ScreenshotStep, Step,
};

/// Parse the `--since` flag for the `network` command into a navigation index.
///
/// - `"all"` or `"0"` → `0` (full buffer, no boundary filter)
/// - `"-1"` → `-1` (since most-recent navigation; also the default when `None`)
/// - Any other integer string → that value directly
fn parse_since_arg(s: Option<&str>) -> Result<i64, AppError> {
    match s {
        None => Ok(-1), // default: current navigation
        Some("all") => Ok(0),
        Some(v) => v
            .parse::<i64>()
            .map_err(|_| AppError::User(format!("--since: expected a number or 'all', got {v:?}"))),
    }
}

/// Parse a `--dispatch` flag value into a [`DispatchMode`].
fn parse_dispatch_mode(s: &str) -> Result<DispatchMode, AppError> {
    match s {
        "pointer" => Ok(DispatchMode::Pointer),
        "legacy" => Ok(DispatchMode::Legacy),
        "click-only" => Ok(DispatchMode::ClickOnly),
        other => Err(AppError::User(format!(
            "--dispatch must be 'pointer', 'legacy', or 'click-only', got: {other:?}"
        ))),
    }
}

/// Resolve a CSS selector from a positional arg or `--selector` flag.
///
/// Returns an error when both are supplied (ambiguous) or neither is supplied
/// (required).  Used by commands that accept the selector either way.
fn resolve_selector<'a>(
    positional: Option<&'a str>,
    flag: Option<&'a str>,
    command: &str,
) -> Result<&'a str, AppError> {
    match (positional, flag) {
        (Some(_), Some(_)) => Err(AppError::User(format!(
            "pass selector either positionally or via --selector, not both (command: {command})"
        ))),
        (Some(s), None) | (None, Some(s)) => Ok(s),
        (None, None) => Err(AppError::User(format!(
            "{command} requires a CSS selector — pass it positionally or with --selector"
        ))),
    }
}

/// Resolve a CSS selector from a positional/flag selector or a `--ref` ref ID.
///
/// When `ref_id` is `Some`, the ref is resolved via the daemon.  In
/// `--no-daemon` mode, refs are not available across invocations (the daemon
/// is the ref store), so we return a clear user error.
///
/// The returned `String` is owned because the ref resolver expression is heap-allocated.
fn resolve_selector_or_ref(
    positional: Option<&str>,
    flag: Option<&str>,
    ref_id: Option<&str>,
    command: &str,
    cli: &Cli,
) -> Result<String, AppError> {
    match ref_id {
        Some(id) => {
            if cli.no_daemon {
                return Err(AppError::User(
                    "--ref is not available with --no-daemon: ref IDs are stored by the daemon and are only valid within a single daemon session".to_string()
                ));
            }
            resolve_ref_via_daemon(cli, id)
        }
        None => resolve_selector(positional, flag, command).map(str::to_owned),
    }
}

/// Connect to the running daemon and resolve a ref ID to its JS resolver expression.
///
/// Returns `AppError::User` with a clear message when the ref has expired,
/// when no daemon is running, or when `--no-daemon` was passed.
pub(crate) fn resolve_ref_via_daemon(cli: &Cli, ref_id: &str) -> Result<String, AppError> {
    use ff_rdp_core::{FramedReader, FramedWriter};
    use serde_json::{Value, json};
    use std::net::TcpStream;
    use std::time::Duration;

    if cli.no_daemon {
        return Err(AppError::User(
            "--ref is not available with --no-daemon: ref IDs are stored by the daemon and are only valid within a single daemon session".to_string()
        ));
    }

    let info = registry::read_registry()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("reading daemon registry: {e}")))?
        .ok_or_else(|| AppError::User(
            format!("--ref {ref_id}: no daemon is running — start the daemon first or use a CSS selector instead")
        ))?;

    let timeout = Duration::from_millis(cli.timeout);
    let addr = format!("127.0.0.1:{}", info.proxy_port)
        .parse::<std::net::SocketAddr>()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("parsing daemon addr: {e}")))?;

    let stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| {
        AppError::Connection(format!(
            "could not connect to daemon for ref resolution: {e}"
        ))
    })?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("setting read timeout: {e}")))?;

    let mut writer = FramedWriter::from_stream(
        stream
            .try_clone()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("cloning stream: {e}")))?,
    );
    writer
        .send(&json!({"auth": info.auth_token}))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("sending auth frame: {e}")))?;

    let mut reader = FramedReader::from_stream(stream);
    // Read and discard the greeting.
    reader
        .recv()
        .map_err(|e| AppError::User(format!("daemon auth failed: {e}")))?;

    // Send resolve-ref request and read responses until we get a daemon reply.
    writer
        .send(&json!({"to": "daemon", "type": "resolve-ref", "id": ref_id}))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("sending resolve-ref: {e}")))?;

    for _ in 0..64 {
        let resp = reader.recv().map_err(|e| {
            AppError::Internal(anyhow::anyhow!("receiving resolve-ref response: {e}"))
        })?;
        if resp.get("from").and_then(Value::as_str) == Some("daemon") {
            if let Some(err) = resp.get("error").and_then(Value::as_str) {
                return Err(AppError::User(err.to_owned()));
            }
            return resp
                .get("resolver")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    AppError::Internal(anyhow::anyhow!("resolve-ref response missing 'resolver'"))
                });
        }
    }
    Err(AppError::Internal(anyhow::anyhow!(
        "did not receive resolve-ref response within 64 frames"
    )))
}

/// Resolve a ref ID to its CSS selector expression for use in script runner verbs.
///
/// Provides the same ref-resolution logic as the CLI dispatch path so that
/// script steps with `ref:` targets work identically to `--ref` on the CLI.
pub(crate) fn resolve_ref_for_script(
    cli: &Cli,
    ref_id: &str,
    verb: &str,
) -> Result<String, AppError> {
    resolve_ref_via_daemon(cli, ref_id)
        .map_err(|e| AppError::User(format!("script step '{verb}' ref '{ref_id}': {e}")))
}

/// Convert a CLI command to a recordable `Step`, given the resolved selector
/// (already resolved from `--ref` by the dispatch path).
///
/// Returns `None` for inspection-only commands (tabs, dom, snapshot, console,
/// network, page-text, cookies, storage, sources, geometry, styles, computed,
/// responsive, a11y, doctor, daemon *, launch, record *, inspect) that should
/// not appear in recorded scripts.
fn command_to_step(cmd: &Command, resolved_selector: Option<&str>) -> Option<Step> {
    match cmd {
        Command::Navigate {
            url,
            wait_text,
            wait_selector,
            ..
        } => Some(Step::Navigate(NavigateStep {
            url: url.clone(),
            wait_text: wait_text.clone(),
            wait_selector: wait_selector.clone(),
            // no_wait and wait_for are not currently representable in the script format.
        })),
        Command::Click { wait_for, .. } => {
            let sel = resolved_selector?;
            // The `click` step in the script schema only supports `wait_for_text`
            // and `wait_for_selector`.  Capture the first matching predicate of
            // each kind, and warn about predicates that cannot round-trip
            // (additional `text:`/`selector:` repeats, or `url:`/`gone:` —
            // currently not representable in the script format).
            let wait_for_text = wait_for
                .iter()
                .find_map(|p| p.strip_prefix("text:").map(str::to_owned));
            let wait_for_selector = wait_for
                .iter()
                .find_map(|p| p.strip_prefix("selector:").map(str::to_owned));
            let dropped: Vec<&str> = wait_for
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let is_first_text = p.starts_with("text:")
                        && wait_for.iter().position(|q| q.starts_with("text:")) == Some(i);
                    let is_first_selector = p.starts_with("selector:")
                        && wait_for.iter().position(|q| q.starts_with("selector:")) == Some(i);
                    if is_first_text || is_first_selector {
                        None
                    } else {
                        Some(p.as_str())
                    }
                })
                .collect();
            if !dropped.is_empty() {
                eprintln!(
                    "warning: recorder cannot represent these click predicates in the script format and dropped them: {dropped:?}"
                );
            }
            Some(Step::Click(ElementStep {
                target: ElementTarget {
                    selector: Some(sel.to_owned()),
                    ..Default::default()
                },
                wait_for_text,
                wait_for_selector,
            }))
        }
        Command::Type {
            text_pos,
            text_flag,
            clear,
            ..
        } => {
            let sel = resolved_selector?;
            let text = text_pos.as_deref().or(text_flag.as_deref())?;
            Some(Step::Type(crate::script::format::TypeStep {
                target: ElementTarget {
                    selector: Some(sel.to_owned()),
                    ..Default::default()
                },
                text: text.to_owned(),
                clear: *clear,
                secret: false,
            }))
        }
        Command::Wait {
            selector,
            text,
            eval,
            wait_timeout,
            ..
        } => {
            // Record whichever condition was used, including the explicit timeout.
            if selector.is_some() || text.is_some() || eval.is_some() || resolved_selector.is_some()
            {
                // Record the timeout only when it differs from the default (5000 ms)
                // so that scripts stay clean when the user didn't explicitly set it.
                const DEFAULT_WAIT_TIMEOUT_MS: u64 = 5000;
                let recorded_timeout = if *wait_timeout == DEFAULT_WAIT_TIMEOUT_MS {
                    None
                } else {
                    Some(*wait_timeout)
                };
                Some(Step::Wait(crate::script::format::WaitStep {
                    selector: resolved_selector
                        .map(str::to_owned)
                        .or_else(|| selector.clone()),
                    text: text.clone(),
                    eval: eval.clone(),
                    timeout: recorded_timeout,
                }))
            } else {
                None
            }
        }
        Command::Screenshot {
            output,
            base64,
            full_page,
            ..
        } => Some(Step::Screenshot(ScreenshotStep {
            output: output.clone(),
            base64: *base64,
            full_page: *full_page,
        })),
        Command::Eval {
            script, stringify, ..
        } => {
            // Only record inline positional evals (not --file or --stdin).
            let script_text = script.as_deref()?;
            Some(Step::Eval(EvalStep {
                script: script_text.to_owned(),
                stringify: *stringify,
            }))
        }
        // Reload/Back/Forward, Scroll, inspection-only, and meta commands — never recorded.
        Command::Reload { .. }
        | Command::Back
        | Command::Forward
        | Command::Scroll { .. }
        | Command::Tabs
        | Command::Dom { .. }
        | Command::Console { .. }
        | Command::Network { .. }
        | Command::Perf { .. }
        | Command::PageText
        | Command::Cookies { .. }
        | Command::Storage { .. }
        | Command::Sources { .. }
        | Command::Geometry { .. }
        | Command::Styles { .. }
        | Command::Computed { .. }
        | Command::Responsive { .. }
        | Command::A11y { .. }
        | Command::Snapshot { .. }
        | Command::Inspect { .. }
        | Command::Doctor
        | Command::Launch { .. }
        | Command::Record { .. }
        | Command::Run { .. }
        | Command::Daemon { .. }
        | Command::DaemonInternal
        | Command::InstallSkill(_)
        | Command::Index { .. } => None,
    }
}

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
/// | `console` (no --follow)   | Calls `getCachedMessages` on the console actor — reads cached messages, not a daemon buffer |
///
/// All other commands use `connect_and_get_target`, which routes through
/// the daemon when available or falls back to direct connection.
///
/// **Note:** Non-streaming `network` and `console` intentionally use the
/// daemon path because they drain events the daemon has been buffering in
/// the background — this is the daemon's primary value proposition for
/// these commands.
pub fn dispatch(cli: &Cli) -> Result<(), AppError> {
    // For recording: track the resolved selector (refs resolved to CSS) so the
    // recorded step uses the concrete selector, not the ephemeral ref ID.
    let mut recording_resolved_selector: Option<String> = None;

    let result = dispatch_inner(cli, &mut recording_resolved_selector);

    // If the command succeeded and a recording is active, append the step.
    if result.is_ok()
        && let Some(step) = command_to_step(&cli.command, recording_resolved_selector.as_deref())
    {
        // Best-effort: log to stderr but don't fail the command.
        crate::script::recorder::record_step_if_active(&step);
    }

    result
}

fn dispatch_inner(
    cli: &Cli,
    recording_resolved_selector: &mut Option<String>,
) -> Result<(), AppError> {
    match &cli.command {
        Command::Tabs => commands::tabs::run(cli),
        Command::Navigate {
            url,
            with_network,
            network_timeout,
            wait_text,
            wait_selector,
            wait_timeout,
            no_wait,
            wait_for,
            wait,
        } => {
            let wait_opts = commands::navigate::WaitAfterNav {
                wait_text: wait_text.as_deref(),
                wait_selector: wait_selector.as_deref(),
                wait_timeout: *wait_timeout,
                no_wait: *no_wait,
                wait_for,
                wait_level: *wait,
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
            no_isolate,
        } => commands::eval::run(
            cli,
            script.as_deref(),
            file.as_deref(),
            *stdin,
            *stringify,
            *no_isolate,
        ),
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
            ref_id,
            outer_html: _,
            inner_html,
            text,
            attrs,
            text_attrs,
            count,
            first,
        } => match dom_command {
            Some(DomCommand::Stats) => commands::dom::run_stats(cli),
            Some(DomCommand::Tree {
                selector,
                depth,
                max_chars,
            }) => commands::dom_tree::run(cli, selector.as_deref(), *depth, *max_chars),
            None => {
                // --ref resolves to a querySelectorAll expression usable as a selector.
                let resolved: Option<String> = if let Some(id) = ref_id.as_deref() {
                    Some(resolve_ref_via_daemon(cli, id)?)
                } else {
                    None
                };
                let sel = resolved.as_deref().or(selector.as_deref()).ok_or_else(|| {
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
                        // Default: ARIA-tree JSON (iter-60+).
                        // `--format html` switches to raw HTML in run().
                        commands::dom::OutputMode::AriaTree
                    };
                    commands::dom::run(cli, sel, mode, *first)
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
            headers,
            since,
        } => {
            if *follow {
                commands::network::run_follow(cli, filter.as_deref(), method.as_deref())
            } else {
                let since_nav = parse_since_arg(since.as_deref())?;
                commands::network::run(
                    cli,
                    filter.as_deref(),
                    method.as_deref(),
                    *headers,
                    since_nav,
                )
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
        Command::Click {
            selector_pos,
            selector_flag,
            ref_id,
            wait_for_network,
            network_timeout,
            no_wait,
            dispatch,
            wait_for,
            wait_for_timeout,
            settle,
        } => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "click",
                cli,
            )?;
            // Capture the resolved selector for recording (ref → concrete CSS selector).
            *recording_resolved_selector = Some(selector.clone());
            let dispatch_mode = parse_dispatch_mode(dispatch)?;
            commands::click::run(
                cli,
                &selector,
                wait_for_network.as_deref(),
                *network_timeout,
                &commands::click::ClickOptions {
                    no_wait: *no_wait,
                    dispatch: dispatch_mode,
                    wait_for,
                    wait_for_timeout_ms: *wait_for_timeout,
                    settle: *settle,
                    ..Default::default()
                },
            )
        }
        Command::Type {
            selector_pos,
            text_pos,
            selector_flag,
            text_flag,
            ref_id,
            clear,
            no_wait,
            wait_for,
            wait_for_timeout,
            settle,
        } => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "type",
                cli,
            )?;
            // Capture the resolved selector for recording.
            *recording_resolved_selector = Some(selector.clone());
            let text = match (text_pos.as_deref(), text_flag.as_deref()) {
                (Some(_), Some(_)) => {
                    return Err(AppError::User(
                        "pass text either positionally or via --text, not both".to_owned(),
                    ));
                }
                (Some(t), None) | (None, Some(t)) => t,
                (None, None) => {
                    return Err(AppError::User(
                        "type requires text — pass it positionally (\"ff-rdp type '<sel>' '<text>'\") or with --text"
                            .to_owned(),
                    ));
                }
            };
            commands::type_text::run(
                cli,
                &selector,
                text,
                *clear,
                &commands::type_text::TypeOptions {
                    no_wait: *no_wait,
                    wait_for,
                    wait_for_timeout_ms: *wait_for_timeout,
                    settle: *settle,
                    ..Default::default()
                },
            )
        }
        Command::Wait {
            selector,
            text,
            eval,
            ref_id,
            wait_timeout,
        } => {
            // --ref resolves to a JS querySelectorAll expression; treat it as a --selector.
            let resolved_selector: Option<String> = if let Some(id) = ref_id.as_deref() {
                Some(
                    resolve_ref_via_daemon(cli, id)
                        .map_err(|e| AppError::User(format!("--ref: {e}")))?,
                )
            } else {
                None
            };
            // Capture resolved selector for recording.
            if let Some(ref sel) = resolved_selector {
                *recording_resolved_selector = Some(sel.clone());
            }
            commands::wait::run(
                cli,
                &commands::wait::WaitOptions {
                    selector: resolved_selector.as_deref().or(selector.as_deref()),
                    text: text.as_deref(),
                    eval: eval.as_deref(),
                    wait_timeout: *wait_timeout,
                },
            )
        }
        Command::A11y {
            a11y_command,
            depth,
            max_chars,
            selector,
            ref_id,
            interactive,
        } => {
            let resolved_selector: Option<String> = if let Some(id) = ref_id.as_deref() {
                Some(resolve_ref_via_daemon(cli, id)?)
            } else {
                None
            };
            let effective_selector = resolved_selector.as_deref().or(selector.as_deref());
            match a11y_command {
                Some(A11yCommand::Contrast {
                    selector: contrast_selector,
                    fail_only,
                }) => commands::a11y_contrast::run(cli, contrast_selector.as_deref(), *fail_only),
                Some(A11yCommand::Summary) => commands::a11y_summary::run(cli),
                None => {
                    commands::a11y::run(cli, *depth, *max_chars, effective_selector, *interactive)
                }
            }
        }
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
            output_root,
            bulk,
        } => commands::screenshot::run(
            cli,
            &commands::screenshot::ScreenshotOpts {
                output_path: output.as_deref(),
                base64_mode: *base64,
                full_page: *full_page,
                bulk: *bulk,
                viewport_height: *viewport_height,
                output_root: output_root.as_deref(),
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
            selector_pos,
            selector_flag,
            ref_id,
            prop,
            all,
        } => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "computed",
                cli,
            )?;
            commands::computed::run(cli, &selector, prop, *all)
        }
        Command::Styles {
            selector_pos,
            selector_flag,
            ref_id,
            applied,
            layout,
            properties,
        } => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "styles",
                cli,
            )?;
            if *applied {
                commands::styles::run_applied(cli, &selector)
            } else if *layout {
                commands::styles::run_layout(cli, &selector)
            } else {
                commands::styles::run(cli, &selector, properties.as_deref())
            }
        }
        Command::Geometry {
            selectors,
            ref_id,
            include_hidden,
        } => {
            if let Some(id) = ref_id.as_deref() {
                let resolved = resolve_ref_via_daemon(cli, id)?;
                commands::geometry::run(cli, &[resolved], *include_hidden)
            } else {
                commands::geometry::run(cli, selectors, *include_hidden)
            }
        }
        Command::Responsive {
            selectors,
            ref_id,
            widths,
            include_hidden,
        } => {
            if let Some(id) = ref_id.as_deref() {
                let resolved = resolve_ref_via_daemon(cli, id)?;
                commands::responsive::run(cli, &[resolved], widths, *include_hidden)
            } else {
                commands::responsive::run(cli, selectors, widths, *include_hidden)
            }
        }
        Command::Snapshot { depth, max_chars } => commands::snapshot::run(cli, *depth, *max_chars),
        Command::Scroll { scroll_command } => match scroll_command {
            ScrollCommand::To {
                selector,
                ref_id,
                block,
                smooth,
                no_wait,
                wait_for,
                wait_for_timeout,
                settle,
            } => {
                let resolved = resolve_selector_or_ref(
                    selector.as_deref(),
                    None,
                    ref_id.as_deref(),
                    "scroll to",
                    cli,
                )?;
                commands::scroll::run_to(
                    cli,
                    &resolved,
                    *block,
                    *smooth,
                    &commands::scroll::ScrollOptions {
                        no_wait: *no_wait,
                        wait_for,
                        wait_for_timeout_ms: *wait_for_timeout,
                        settle: *settle,
                        ..Default::default()
                    },
                )
            }
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
        Command::DaemonInternal => {
            server::run_daemon(&cli.host, cli.port, cli.daemon_timeout).map_err(AppError::Internal)
        }
        Command::Daemon { daemon_command } => match daemon_command {
            DaemonCommand::Status => crate::daemon::client::run_daemon_status(cli),
            DaemonCommand::Stop => crate::daemon::client::run_daemon_stop(cli),
        },
        Command::Doctor => commands::doctor::run(cli),
        Command::Run {
            script,
            vars,
            vars_file,
            env_file,
            continue_on_failure,
            dry_run,
            show_secrets,
            record,
            record_strict,
            script_format,
            page_map,
            allow_env,
            allow_unsafe_script_paths,
        } => {
            // Parse --vars KEY=VALUE flags.
            let mut extra_vars: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for kv in vars {
                if let Some((k, v)) = kv.split_once('=') {
                    extra_vars.insert(k.to_owned(), v.to_owned());
                } else {
                    return Err(AppError::User(format!(
                        "--vars must be in KEY=VALUE format, got: {kv:?}"
                    )));
                }
            }

            // Resolve vars file: --vars-file takes priority; --env-file is deprecated alias.
            let effective_vars_file = vars_file.as_deref().or_else(|| {
                if env_file.is_some() {
                    eprintln!(
                        "warning: --env-file is deprecated; use --vars-file instead \
                         (values go to {{{{vars.X}}}}, not the process environment)"
                    );
                }
                env_file.as_deref()
            });

            // Load --vars-file / --env-file if provided.
            if let Some(vars_path) = effective_vars_file {
                let content = std::fs::read_to_string(vars_path).map_err(|e| {
                    AppError::User(format!(
                        "reading --vars-file '{}': {e}",
                        vars_path.display()
                    ))
                })?;
                for (line_num, raw_line) in content.lines().enumerate() {
                    let line = raw_line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        // All key/value pairs from the vars-file are merged into
                        // `extra_vars` (the same map populated by --vars flags).
                        // `or_insert_with` makes CLI --vars values win over
                        // vars-file values for the same key.  Secret-shaped
                        // entries are then auto-redacted by the runner via
                        // `is_secret_name`.
                        extra_vars
                            .entry(k.to_owned())
                            .or_insert_with(|| v.to_owned());
                    } else {
                        return Err(AppError::User(format!(
                            "--vars-file '{}' line {}: expected KEY=VALUE, got: {line:?}",
                            vars_path.display(),
                            line_num + 1
                        )));
                    }
                }
            }

            let opts = commands::run::RunCommandOpts {
                script_path: script,
                extra_vars,
                bail_on_failure: !continue_on_failure,
                dry_run: *dry_run,
                show_secrets: *show_secrets,
                record_output: record.as_deref(),
                record_strict: *record_strict,
                format_override: script_format.as_deref(),
                page_map_path: page_map.as_deref(),
                allow_env: allow_env.clone(),
                allow_unsafe_script_paths: *allow_unsafe_script_paths,
            };
            commands::run::run(cli, &opts)
        }
        Command::Record { record_command } => match record_command {
            RecordCommand::Start { output, name } => {
                commands::record::run_start(output, name.as_deref())
            }
            RecordCommand::Stop => commands::record::run_stop(),
            RecordCommand::Status => commands::record::run_status(),
        },
        Command::InstallSkill(args) => {
            if !args.claude {
                return Err(AppError::User(
                    "--claude flag is required for install-skill (forward-compat; only Claude Code runtime is supported today)".to_string(),
                ));
            }
            commands::install_skill::run(cli, args)
        }
        Command::Index {
            base_url,
            out,
            depth,
            max_pages,
            include,
            exclude,
            format,
            cross_origin,
            ignore_robots,
            cookies_from: _,
            bearer: _,
            login_script,
            check,
            page_map,
            report,
            output_root,
        } => {
            let opts = IndexOpts {
                base_url: base_url.as_deref(),
                out,
                depth: *depth,
                max_pages: *max_pages,
                include: include.as_deref(),
                exclude: exclude.as_deref(),
                format,
                cross_origin: *cross_origin,
                ignore_robots: *ignore_robots,
                login_script: login_script.as_deref(),
                check: *check,
                page_map: page_map.as_deref(),
                report: report.as_deref(),
                silent: false,
                output_root: output_root.as_deref(),
            };
            commands::index::run(cli, &opts)
        }
    }
}
