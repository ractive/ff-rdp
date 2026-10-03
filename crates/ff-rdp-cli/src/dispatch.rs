use crate::cli::args::{
    A11yArgs, A11yCommand, BackForwardArgs, CascadeArgs, Cli, ClickArgs, Command, ComputedArgs,
    ConsentCommand, ConsoleArgs, CookiesArgs, DomArgs, DomCommand, EvalArgs, GeometryArgs,
    HomeArgs, IndexArgs, InspectArgs, InstallHookArgs, LaunchArgs, NavigateArgs, NetworkArgs,
    NetworkConditionsArgs, PerfArgs, PerfCommand, ProfilesCommand, RecordCommand, ReloadArgs,
    ResponsiveArgs, RunArgs, ScreenshotArgs, ScrollCommand, SnapshotArgs, SourcesArgs, StorageArgs,
    StylesArgs, TypeArgs, WaitArgs,
};
use crate::commands;
use crate::commands::index::IndexOpts;
use crate::commands::js_helpers::DispatchMode;
use crate::commands::nav_action::NavAction;
use crate::error::AppError;
use crate::output_controls::QueryFilter;
use crate::script::format::{
    ElementStep, ElementTarget, EvalStep, NavigateStep, ScreenshotStep, Step,
};

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
/// A ref resolves to the attribute selector of the element `snapshot`, `dom`
/// or a `--with-page` view stamped in the page (see [`ref_selector`]).
fn resolve_selector_or_ref(
    positional: Option<&str>,
    flag: Option<&str>,
    ref_id: Option<&str>,
    command: &str,
) -> Result<String, AppError> {
    match ref_id {
        Some(id) => ref_selector(id),
        None => resolve_selector(positional, flag, command).map(str::to_owned),
    }
}

/// The CSS selector for ref `id` (`e<N>`).
///
/// Refs live in the page, not in ff-rdp: the command that hands one out sets
/// `data-ffrdp-ref="e<N>"` on the element in the same evaluation that
/// enumerates it, so any later command — on any connection — finds it with
/// `[data-ffrdp-ref="e<N>"]`. A navigation replaces the document and with it
/// every ref; a stale ref then simply matches nothing.
pub(crate) fn ref_selector(id: &str) -> Result<String, AppError> {
    // Stamping produces `e1`, `e2`, … — no `e0`, no leading zeros.
    let valid = id.strip_prefix('e').is_some_and(|n| {
        !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    });
    if valid {
        Ok(format!(
            "[{}=\"{id}\"]",
            crate::commands::js_helpers::REF_ATTR
        ))
    } else {
        Err(AppError::User(format!(
            "--ref {id:?}: expected a ref like 'e3' from `snapshot`, `dom` or `--with-page`"
        )))
    }
}

/// A `--ref` that matched nothing usually belongs to a page that has since
/// navigated or re-rendered; say so instead of leaving a bare
/// "no element matches `[data-ffrdp-ref=…]`".
fn with_stale_ref_hint(err: AppError, selector: Option<&str>) -> AppError {
    match (err, selector) {
        (AppError::User(msg), Some(sel)) if is_ref_selector(sel) && msg.contains(sel) => {
            AppError::User(format!(
                "{msg}\nhint: refs are stamped in the page and die when it navigates or \
                 re-renders the element — run `ff-rdp snapshot` / `a11y summary` again for a fresh ref"
            ))
        }
        (err, _) => err,
    }
}

/// Whether `selector` is the attribute selector [`ref_selector`] produces.
fn is_ref_selector(selector: &str) -> bool {
    selector.starts_with(&format!("[{}=", crate::commands::js_helpers::REF_ATTR))
}

/// Resolve a ref ID to its CSS selector for use in script runner verbs.
///
/// Same resolution as `--ref` on the CLI, so script steps with `ref:` targets
/// behave identically.
pub(crate) fn resolve_ref_for_script(ref_id: &str, verb: &str) -> Result<String, AppError> {
    ref_selector(ref_id)
        .map_err(|e| AppError::User(format!("script step '{verb}' ref '{ref_id}': {e}")))
}

/// Convert a CLI command to a recordable `Step`, given the resolved selector
/// (already resolved from `--ref` by the dispatch path).
///
/// Returns `None` for inspection-only commands (tabs, dom, snapshot, console,
/// network, page-text, cookies, storage, sources, geometry, styles, computed,
/// responsive, a11y, doctor, launch, record *, inspect) that should
/// not appear in recorded scripts.
fn command_to_step(cmd: &Command, resolved_selector: Option<&str>) -> Option<Step> {
    match cmd {
        Command::Navigate(NavigateArgs {
            url,
            wait_text,
            wait_selector,
            ..
        }) => Some(Step::Navigate(NavigateStep {
            url: url.clone(),
            wait_text: wait_text.clone(),
            wait_selector: wait_selector.clone(),
            // no_wait and wait_for are not currently representable in the script format.
        })),
        Command::Click(ClickArgs { wait_for, .. }) => {
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
        Command::Type(TypeArgs {
            text_pos,
            text_flag,
            clear,
            ..
        }) => {
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
        Command::Wait(WaitArgs {
            selector,
            text,
            eval,
            wait_timeout,
            ..
        }) => {
            let effective_timeout = *wait_timeout;
            // Record whichever condition was used, including the explicit timeout.
            if selector.is_some() || text.is_some() || eval.is_some() || resolved_selector.is_some()
            {
                // Record the timeout only when it differs from the default (5000 ms)
                // so that scripts stay clean when the user didn't explicitly set it.
                const DEFAULT_WAIT_TIMEOUT_MS: u64 = 5000;
                let recorded_timeout = if effective_timeout == DEFAULT_WAIT_TIMEOUT_MS {
                    None
                } else {
                    Some(effective_timeout)
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
        Command::Screenshot(ScreenshotArgs {
            output,
            base64,
            full_page,
            ..
        }) => Some(Step::Screenshot(ScreenshotStep {
            output: output.clone(),
            base64: *base64,
            full_page: *full_page,
        })),
        Command::Eval(EvalArgs {
            script, stringify, ..
        }) => {
            // Only record inline positional evals (not --file or --stdin).
            let script_text = script.as_deref()?;
            Some(Step::Eval(EvalStep {
                script: script_text.to_owned(),
                stringify: *stringify,
            }))
        }
        // Reload/Back/Forward, Scroll, inspection-only, and meta commands — never recorded.
        Command::Reload(ReloadArgs { .. })
        | Command::Back(BackForwardArgs { .. })
        | Command::Forward(BackForwardArgs { .. })
        | Command::Scroll { .. }
        | Command::Tabs
        | Command::Dom(DomArgs { .. })
        | Command::Console(ConsoleArgs { .. })
        | Command::Network(NetworkArgs { .. })
        | Command::Perf(PerfArgs { .. })
        | Command::PageText(_)
        | Command::Cookies(CookiesArgs { .. })
        | Command::Storage(StorageArgs { .. })
        | Command::Sources(SourcesArgs { .. })
        | Command::Geometry(GeometryArgs { .. })
        | Command::Styles(StylesArgs { .. })
        | Command::Computed(ComputedArgs { .. })
        | Command::Cascade(CascadeArgs { .. })
        | Command::Responsive(ResponsiveArgs { .. })
        | Command::Manifest
        | Command::A11y(A11yArgs { .. })
        | Command::Snapshot(SnapshotArgs { .. })
        | Command::Inspect(InspectArgs { .. })
        | Command::Doctor
        | Command::Launch(LaunchArgs { .. })
        | Command::Record { .. }
        | Command::Run(RunArgs { .. })
        | Command::InstallSkill(_)
        | Command::InstallHook(_)
        | Command::Home(_)
        | Command::SkillDoc
        | Command::Profiles { .. }
        | Command::Index(IndexArgs { .. })
        | Command::Consent { .. }
        | Command::Completions { .. } => None,
    }
}

/// Dispatch a CLI command to its handler.
///
/// Every browser-touching command opens its own connection to Firefox, does
/// its work and disconnects; nothing outlives the process. State that must
/// span commands lives in the browser (the page, its refs, Firefox's console
/// cache), and event capture across commands is a `--follow` stream started
/// before them.
pub fn dispatch(cli: &Cli) -> Result<(), AppError> {
    // For recording: track the selector the command actually used (a `--ref`
    // resolves to its `[data-ffrdp-ref=…]` attribute selector).
    let mut recording_resolved_selector: Option<String> = None;

    let result = dispatch_inner(cli, &mut recording_resolved_selector)
        .map_err(|e| with_stale_ref_hint(e, recording_resolved_selector.as_deref()));

    // If the command succeeded and a recording is active, append the step.
    if result.is_ok()
        && let Some(step) = command_to_step(&cli.command, recording_resolved_selector.as_deref())
    {
        if recording_resolved_selector
            .as_deref()
            .is_some_and(is_ref_selector)
            && matches!(crate::script::recorder::read_state(), Ok(Some(_)))
        {
            // stderr-ok: (b) warn-and-continue — the step is still recorded.
            eprintln!(
                "warning: recorded a --ref target as its in-page ref selector; refs do not \
                 survive navigation, so replace it with a CSS selector before replaying"
            );
        }
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
        Command::Navigate(NavigateArgs {
            url,
            with_network,
            network_timeout,
            headers,
            security,
            wait_text,
            wait_selector,
            wait_timeout,
            no_wait,
            wait_for,
            wait,
            wait_strategy,
            auto_consent,
            conditions,
            page,
        }) => {
            let wait_opts = commands::navigate::WaitAfterNav {
                wait_text: wait_text.as_deref(),
                wait_selector: wait_selector.as_deref(),
                wait_timeout: *wait_timeout,
                no_wait: *no_wait,
                wait_for,
                wait_level: *wait,
                wait_strategy: *wait_strategy,
            };
            if *with_network {
                commands::navigate::run_with_network(
                    cli,
                    url,
                    &wait_opts,
                    *network_timeout,
                    commands::navigate::NetworkDetail {
                        headers: *headers,
                        security: *security,
                    },
                    *auto_consent,
                    conditions,
                    page,
                )
            } else {
                commands::navigate::run(cli, url, &wait_opts, *auto_consent, conditions, page)
            }
        }
        Command::Eval(EvalArgs {
            script,
            file,
            stdin,
            stringify,
            no_isolate,
            frame,
            inner_window,
            unwrap,
        }) => commands::eval::run(
            cli,
            script.as_deref(),
            file.as_deref(),
            *stdin,
            *stringify,
            *no_isolate,
            *unwrap,
            commands::eval::CliEvalScope {
                frame_url: frame.as_deref(),
                inner_window_id: *inner_window,
            },
        ),
        Command::Reload(ReloadArgs {
            wait_idle,
            idle_ms,
            reload_timeout,
            hard,
            no_wait,
            conditions,
            page,
        }) => {
            if *wait_idle {
                commands::nav_action::run_reload_wait_idle(
                    cli,
                    *idle_ms,
                    *reload_timeout,
                    *hard,
                    conditions,
                    page,
                )
            } else {
                commands::nav_action::run(
                    cli,
                    NavAction::Reload {
                        force: *hard,
                        no_wait: *no_wait,
                    },
                    conditions,
                    page,
                )
            }
        }
        Command::Back(BackForwardArgs { no_wait, page }) => commands::nav_action::run(
            cli,
            NavAction::Back { no_wait: *no_wait },
            &NetworkConditionsArgs::default(),
            page,
        ),
        Command::Forward(BackForwardArgs { no_wait, page }) => commands::nav_action::run(
            cli,
            NavAction::Forward { no_wait: *no_wait },
            &NetworkConditionsArgs::default(),
            page,
        ),
        Command::PageText(args) => commands::page_text::run(cli, args),
        Command::Dom(DomArgs {
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
            include_style,
            include_style_limit,
            query,
        }) => match dom_command {
            // iter-211: `--query` filters `dom <selector>` results only. The
            // sub-commands produce a stats record and a walker tree, neither
            // of which the filter applies to — refuse rather than accept the
            // flag and do nothing with it.
            Some(_) if QueryFilter::from_query_args(query).is_active() => Err(AppError::User(
                "--query/--query-regex applies to `dom <selector>`, not to `dom stats` or `dom tree`"
                    .to_owned(),
            )),
            Some(DomCommand::Stats) => commands::dom::run_stats(cli),
            Some(DomCommand::Tree {
                selector,
                depth,
                max_chars,
            }) => commands::dom_tree::run(cli, selector.as_deref(), *depth, *max_chars),
            None => {
                // --ref resolves to a genuinely-unique CSS selector (iter-140 Theme A).
                let resolved: Option<String> = if let Some(id) = ref_id.as_deref() {
                    Some(ref_selector(id)?)
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
                    let style_props: Vec<String> = include_style
                        .as_deref()
                        .map(|s| {
                            s.split(',')
                                .map(str::trim)
                                .filter(|p| !p.is_empty())
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    commands::dom::run(
                        cli,
                        sel,
                        mode,
                        *first,
                        &style_props,
                        *include_style_limit,
                        &QueryFilter::from_query_args(query),
                    )
                }
            }
        },
        Command::Console(ConsoleArgs {
            level,
            pattern,
            follow,
        }) => {
            if *follow {
                commands::console::run_follow(cli, level.as_deref(), pattern.as_deref())
            } else {
                commands::console::run(cli, level.as_deref(), pattern.as_deref())
            }
        }
        Command::Network(NetworkArgs {
            filter,
            method,
            follow,
            headers,
            security,
            source,
        }) => {
            if *follow {
                commands::network::run_follow(cli, filter.as_deref(), method.as_deref())
            } else {
                commands::network::run(
                    cli,
                    filter.as_deref(),
                    method.as_deref(),
                    *headers,
                    *security,
                    *source,
                )
            }
        }
        Command::Perf(PerfArgs {
            perf_command,
            entry_type,
            filter,
            group_by,
        }) => match perf_command {
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
        Command::Click(ClickArgs {
            selector_pos,
            selector_flag,
            ref_id,
            wait_for_network,
            network_timeout,
            headers,
            no_wait,
            dispatch,
            wait_for,
            wait_for_timeout,
            settle,
            frame,
            visible,
            index,
            page,
        }) => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "click",
            )?;
            // Capture the resolved selector for recording (ref → concrete CSS selector).
            *recording_resolved_selector = Some(selector.clone());
            let dispatch_mode = parse_dispatch_mode(dispatch)?;
            let match_policy = commands::js_helpers::MatchPolicy::from_flags(*visible, *index)?;
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
                    frame: frame.as_deref(),
                    match_policy,
                    page: page.clone(),
                    network_headers: *headers,
                    ..Default::default()
                },
            )
        }
        Command::Type(TypeArgs {
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
            visible,
            index,
            submit,
            page,
        }) => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "type",
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
            let match_policy = commands::js_helpers::MatchPolicy::from_flags(*visible, *index)?;
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
                    match_policy,
                    submit: *submit,
                    page: page.clone(),
                    ..Default::default()
                },
            )
        }
        Command::Wait(WaitArgs {
            selector,
            text,
            eval,
            ref_id,
            sleep_ms,
            wait_timeout,
        }) => {
            let effective_timeout = *wait_timeout;
            // --ref resolves to a genuinely-unique CSS selector; treat it as a --selector.
            let resolved_selector: Option<String> = if let Some(id) = ref_id.as_deref() {
                Some(ref_selector(id)?)
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
                    sleep_ms: *sleep_ms,
                    wait_timeout: effective_timeout,
                },
            )
        }
        Command::A11y(A11yArgs {
            a11y_command,
            depth,
            max_chars,
            selector,
            ref_id,
            interactive,
            critical,
            native,
        }) => {
            let resolved_selector: Option<String> = if let Some(id) = ref_id.as_deref() {
                Some(ref_selector(id)?)
            } else {
                None
            };
            let effective_selector = resolved_selector.as_deref().or(selector.as_deref());
            match a11y_command {
                Some(A11yCommand::Contrast {
                    selector: contrast_selector,
                    fail_only,
                }) => commands::a11y_contrast::run(cli, contrast_selector.as_deref(), *fail_only),
                Some(A11yCommand::Summary { query }) => {
                    commands::a11y_summary::run(cli, &QueryFilter::from_query_args(query))
                }
                None => {
                    if *critical {
                        commands::a11y::run_critical(cli, effective_selector)
                    } else {
                        commands::a11y::run(
                            cli,
                            *depth,
                            *max_chars,
                            effective_selector,
                            *interactive,
                            *native,
                        )
                    }
                }
            }
        }
        Command::Cookies(CookiesArgs {
            name, storage_only, ..
        }) => {
            // Theme D (iter-83): include document.cookie by default; `--storage-only` opts out.
            // `--include-document-cookie` is kept as a hidden accepted flag for backward compat
            // but is a no-op now that the behavior is the default.
            commands::cookies::run(cli, name.as_deref(), !storage_only)
        }
        Command::Storage(StorageArgs { storage_type, key }) => {
            commands::storage::run(cli, storage_type, key.as_deref())
        }
        Command::Inspect(InspectArgs { expression, depth }) => {
            commands::inspect::run(cli, expression, *depth)
        }
        Command::Sources(SourcesArgs { filter, pattern }) => {
            commands::sources::run(cli, filter.as_deref(), pattern.as_deref())
        }
        Command::Screenshot(ScreenshotArgs {
            output,
            base64,
            full_page,
            viewport_height,
            output_root,
            bulk,
            window_size,
        }) => commands::screenshot::run(
            cli,
            &commands::screenshot::ScreenshotOpts {
                output_path: output.as_deref(),
                base64_mode: *base64,
                full_page: *full_page,
                bulk: *bulk,
                viewport_height: *viewport_height,
                output_root: output_root.as_deref(),
                window_size: window_size.as_deref(),
            },
        ),
        Command::Launch(LaunchArgs {
            english_language_pack,
            restart_after_language_pack_install,
            url,
            headless,
            profile,
            temp_profile,
            debug_port,
            auto_consent,
            window_size,
            replace,
            force,
            launch_timeout,
        }) => commands::launch::run(
            cli,
            &commands::launch::LaunchOpts {
                english_language_pack: english_language_pack.as_deref(),
                restart_after_language_pack_install: *restart_after_language_pack_install,
                url: url.as_deref(),
                headless: *headless,
                profile: profile.as_deref(),
                temp_profile: *temp_profile,
                debug_port: *debug_port,
                auto_consent: *auto_consent,
                replace: *replace || *force,
                window_size: window_size.as_deref(),
                launch_timeout: *launch_timeout,
            },
        ),
        Command::Computed(ComputedArgs {
            selector_pos,
            selector_flag,
            ref_id,
            prop,
            all,
        }) => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "computed",
            )?;
            commands::computed::run(cli, &selector, prop, *all)
        }
        Command::Styles(StylesArgs {
            selector_pos,
            selector_flag,
            ref_id,
            applied,
            layout,
            properties,
            visible,
            index,
        }) => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "styles",
            )?;
            // iter-140 Theme C: `--visible`/`--index` resolve an ambiguous
            // selector to one element before `styles` does anything else —
            // `DomWalkerActor::query_selector` (which `styles` uses
            // internally) always takes the first DOM-order match, with no
            // way to pick a different one on its own.
            let match_policy = commands::js_helpers::MatchPolicy::from_flags(*visible, *index)?;
            let selector = match match_policy {
                Some(policy) => commands::js_helpers::resolve_disambiguated_selector_standalone(
                    cli,
                    &selector,
                    policy,
                    cli.timeout,
                )?,
                None => selector,
            };
            if *applied {
                commands::styles::run_applied(cli, &selector)
            } else if *layout {
                commands::styles::run_layout(cli, &selector)
            } else {
                commands::styles::run(cli, &selector, properties.as_deref())
            }
        }
        Command::Cascade(CascadeArgs {
            selector_pos,
            selector_flag,
            ref_id,
            prop,
            all,
            debug_raw,
        }) => {
            let selector = resolve_selector_or_ref(
                selector_pos.as_deref(),
                selector_flag.as_deref(),
                ref_id.as_deref(),
                "cascade",
            )?;
            commands::cascade::run(cli, &selector, prop.as_deref(), *all, *debug_raw)
        }
        Command::Geometry(GeometryArgs {
            selectors,
            ref_id,
            include_hidden,
        }) => {
            if let Some(id) = ref_id.as_deref() {
                let resolved = ref_selector(id)?;
                commands::geometry::run(cli, &[resolved], *include_hidden)
            } else {
                commands::geometry::run(cli, selectors, *include_hidden)
            }
        }
        Command::Responsive(ResponsiveArgs {
            selectors,
            ref_id,
            widths,
            include_hidden,
            strict,
        }) => {
            if let Some(id) = ref_id.as_deref() {
                let resolved = ref_selector(id)?;
                commands::responsive::run(cli, &[resolved], widths, *include_hidden, *strict)
            } else {
                commands::responsive::run(cli, selectors, widths, *include_hidden, *strict)
            }
        }
        Command::Manifest => commands::manifest::run(cli),
        Command::Snapshot(SnapshotArgs {
            depth,
            max_depth,
            max_chars,
            query,
        }) => {
            // --max-depth overrides --depth; must be ≥ 1.
            let effective_depth = if let Some(md) = max_depth {
                if *md == 0 {
                    return Err(AppError::User(
                        "snapshot: --max-depth must be ≥ 1 (0 would produce an empty tree)"
                            .to_owned(),
                    ));
                }
                *md
            } else {
                *depth
            };
            commands::snapshot::run(
                cli,
                effective_depth,
                *max_chars,
                &QueryFilter::from_query_args(query),
            )
        }
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
                page,
            } => {
                let resolved = resolve_selector_or_ref(
                    selector.as_deref(),
                    None,
                    ref_id.as_deref(),
                    "scroll to",
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
                        page: page.clone(),
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
                page,
            } => commands::scroll::run_by(
                cli,
                *dx,
                *dy,
                commands::scroll::ScrollByOptions {
                    page_down: *page_down,
                    page_up: *page_up,
                    smooth: *smooth,
                    page: page.clone(),
                },
            ),
            ScrollCommand::Container {
                selector,
                dx,
                dy,
                to_end,
                to_start,
                page,
            } => commands::scroll::run_container(
                cli, selector, *dx, *dy, *to_end, *to_start, page,
            ),
            ScrollCommand::Until {
                selector,
                direction,
                timeout,
                page,
            } => commands::scroll::run_until(cli, selector, direction, *timeout, page),
            ScrollCommand::Text { text, page } => commands::scroll::run_text(cli, text, page),
            ScrollCommand::Top { page } => commands::scroll::run_top(cli, page),
            ScrollCommand::Bottom { page } => commands::scroll::run_bottom(cli, page),
        },
        Command::Doctor => commands::doctor::run(cli),
        Command::Profiles { profiles_command } => match profiles_command {
            ProfilesCommand::List => commands::profiles::run_list(cli),
            ProfilesCommand::Prune {
                older_than,
                all,
                dry_run,
            } => commands::profiles::run_prune(cli, older_than, *all, *dry_run),
        },
        Command::Run(RunArgs {
            script,
            vars,
            vars_file,
            continue_on_failure,
            dry_run,
            show_secrets,
            record,
            record_strict,
            script_format,
            page_map,
            allow_env,
            allow_unsafe_script_paths,
        }) => {
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

            // Load --vars-file if provided.
            if let Some(vars_path) = vars_file.as_deref() {
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
        Command::InstallHook(args @ InstallHookArgs { .. }) => {
            commands::install_hook::run(cli, args)
        }
        Command::Home(args @ HomeArgs { .. }) => commands::home::run(cli, args),
        Command::SkillDoc => commands::skill_doc::run(cli),
        Command::InstallSkill(args) => {
            if !args.claude {
                return Err(AppError::User(
                    "--claude flag is required for install-skill (forward-compat; only Claude Code runtime is supported today)".to_string(),
                ));
            }
            commands::install_skill::run(cli, args)
        }
        Command::Index(IndexArgs {
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
        }) => {
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
        Command::Consent { consent_command } => match consent_command {
            ConsentCommand::Accept { allow_no_cmp } => commands::consent::run(cli, *allow_no_cmp),
        },
        Command::Completions { shell } => commands::completions::run(*shell),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_selector_is_the_in_page_attribute_selector() {
        let sel = ref_selector("e12").expect("e12 is a valid ref");
        assert_eq!(sel, "[data-ffrdp-ref=\"e12\"]");
        assert!(is_ref_selector(&sel));
        assert!(!is_ref_selector("button.primary"));
    }

    #[test]
    fn ref_selector_rejects_anything_that_is_not_e_digits() {
        for bad in [
            "", "e", "3", "E3", "e3a", "e-1", "button", "e3\"]", "e0", "e007",
        ] {
            assert!(ref_selector(bad).is_err(), "{bad:?} must be rejected");
        }
    }
}

#[cfg(test)]
mod ref_attr_tests {
    #[test]
    fn stamping_and_resolution_use_the_same_attribute() {
        let attr = crate::commands::js_helpers::REF_ATTR;
        assert!(
            crate::commands::js_helpers::STAMP_REF_JS_FN.contains(&format!("'{attr}'")),
            "STAMP_REF_JS_FN must stamp the attribute ref_selector resolves"
        );
        assert!(super::ref_selector("e4").unwrap().contains(attr));
    }
}
