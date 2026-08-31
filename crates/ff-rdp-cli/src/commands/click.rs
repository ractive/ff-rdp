use std::time::{Duration, Instant};

/// Per-recv polling interval used while waiting for a matching network request.
/// Keeps the wall-clock deadline honored even when the transport's global
/// read timeout is larger than `--network-timeout`.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

use ff_rdp_core::{
    Grip, NetworkResource, NetworkResourceUpdate, ProtocolError, TabActor, TargetEvent,
    WatcherActor, WebConsoleActor, parse_network_resource_updates, parse_network_resources,
    sanitize_for_terminal,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::{
    DispatchMode, MatchPolicy, WaitForPredicate, autowait_element, build_click_js, escape_selector,
    resolve_disambiguated_target, resolve_result, settle_page, wait_for_predicates,
};
use super::network_events::build_network_entries;

/// Options controlling the `click` command's new iter-59 behaviour.
pub struct ClickOptions<'a> {
    /// Auto-wait timeout in ms. `None` means use `cli.timeout`.
    pub wait_timeout_ms: Option<u64>,
    /// Skip auto-wait and act immediately (--no-wait).
    pub no_wait: bool,
    /// Event dispatch mode (pointer / legacy / click-only).
    pub dispatch: DispatchMode,
    /// Post-action predicates (--wait-for).
    pub wait_for: &'a [String],
    /// Timeout for --wait-for predicates. `None` → same as `wait_timeout_ms`.
    pub wait_for_timeout_ms: Option<u64>,
    /// Whether to wait for page settle (--settle).
    pub settle: bool,
    /// iter-129: restrict the click to a frame whose URL contains this
    /// substring, skipping both the top-level attempt and the frame scan.
    /// `None` runs the default top-level-first, scan-on-not-found behaviour.
    pub frame: Option<&'a str>,
    /// iter-140 Theme C: `--visible` / `--index N` — disambiguate a selector
    /// that matches more than one element before doing anything else. `None`
    /// (the default, flag-less path) is completely unchanged: DOM-order index
    /// 0, same timing, same JS.
    pub match_policy: Option<MatchPolicy>,
    /// iter-210 Theme A: `--with-page` — embed the page the click produced
    /// under `results.page`, collected after the click settles. Carries
    /// `--page-chars` and `--query` with it since iter-219.
    pub page: crate::cli::args::PageViewArgs,
}

impl Default for ClickOptions<'_> {
    fn default() -> Self {
        Self {
            wait_timeout_ms: None,
            no_wait: false,
            dispatch: DispatchMode::Pointer,
            wait_for: &[],
            wait_for_timeout_ms: None,
            settle: false,
            frame: None,
            match_policy: None,
            page: crate::cli::args::PageViewArgs::default(),
        }
    }
}

/// Click a DOM element and return the result value without printing.
///
/// Called by the script runner, which handles its own NDJSON output.
pub fn run_core(
    cli: &Cli,
    selector: &str,
    wait_for_network: Option<&str>,
    network_timeout: Option<u64>,
    opts: &ClickOptions<'_>,
) -> Result<(Value, bool), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    // When --wait-for-network is requested in direct mode, subscribe to the
    // watcher before clicking so we don't miss early events.
    let watcher_sub = if wait_for_network.is_some() && !ctx.via_daemon {
        let tab_actor = ctx.target_tab_actor().clone();
        let watcher_actor =
            TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
        WatcherActor::watch_resources(ctx.transport_mut(), &watcher_actor, &["network-event"])
            .map_err(AppError::from)?;
        Some(watcher_actor)
    } else {
        None
    };

    // For daemon mode with --wait-for-network, start streaming before click
    // so events that arrive immediately after the click aren't dropped.
    let daemon_streaming = if wait_for_network.is_some() && ctx.via_daemon {
        use crate::daemon::client::start_daemon_stream;
        start_daemon_stream(ctx.transport_mut(), "network-event").map_err(AppError::from)?;
        true
    } else {
        false
    };

    let wait_timeout_ms = opts.wait_timeout_ms.unwrap_or(cli.timeout);
    let console_actor = ctx.target.console_actor.clone();

    // iter-140 Theme C: `--visible`/`--index` resolve an ambiguous selector to
    // a single, genuinely-unique element selector up front, so every step
    // below (auto-wait, the click itself) acts on exactly the element the
    // flag named instead of blindly taking DOM-order index 0. Flag-less calls
    // skip this entirely — see `ClickOptions::match_policy`'s doc comment.
    let resolved_selector;
    let mut disambiguation: Option<(usize, usize)> = None; // (match_count, chosen_index)
    let selector: &str = if let Some(policy) = opts.match_policy {
        let target = resolve_disambiguated_target(
            &mut ctx,
            &console_actor,
            selector,
            policy,
            wait_timeout_ms,
        )?;
        disambiguation = Some((target.match_count, target.chosen_index));
        resolved_selector = target.selector;
        &resolved_selector
    } else {
        selector
    };

    // A1: Auto-wait for element readiness (unless --no-wait).
    //
    // iter-129: `autowait_element` only ever polled the top-level document.
    // Its behaviour for the common case (selector present on top) is
    // UNCHANGED — no extra eval calls, same JS, same mock-server contract
    // every existing test already relies on. Only on a top-level *timeout*
    // does this now attempt a frame-scan salvage before giving up: fetch
    // frame targets once and check whether the selector exists in any of
    // them. If so, proceed to `do_click` with that prefetched list (skipping
    // its own top-level retry — see `fetch_frame_targets`'s doc comment for
    // why a second enumerate call in the same connection would silently
    // return nothing, and why touching the top-level `console_actor` again
    // after the flag flip can hang). If the selector is missing everywhere,
    // the ORIGINAL top-level timeout error is re-raised unchanged, so
    // genuinely-missing selectors keep their pre-iter-129 error message and
    // ~`wait_timeout_ms` timing exactly. `--frame <substring>` skips
    // autowait entirely, matching `do_click`'s own skip-the-scan behaviour.
    let mut prefetched_targets: Option<Vec<TargetEvent>> = None;
    if !opts.no_wait
        && opts.frame.is_none()
        && let Err(timeout_err) =
            autowait_element(&mut ctx, &console_actor, selector, wait_timeout_ms, false)
    {
        let targets = fetch_frame_targets(&mut ctx)?;
        if selector_exists_in_targets(&mut ctx, &targets, selector)? {
            prefetched_targets = Some(targets);
        } else {
            return Err(timeout_err);
        }
    }

    // Perform the click using the chosen dispatch mode. `frame_url` is
    // `None` when the click landed on the top-level document, `Some(url)`
    // when it landed inside a frame (either via `--frame` or the
    // frame-scan fallback).
    let (mut click_json, frame_url) = do_click(
        &mut ctx,
        selector,
        opts.dispatch,
        opts.frame,
        prefetched_targets,
    )?;
    // iter-160 Theme A: the hit test inside the click JS refused to dispatch —
    // fail here, before --settle/--wait-for/--wait-for-network get a chance to
    // wait on a page nothing touched.
    if let Some(err) = unreachable_click_error(selector, &click_json) {
        return Err(err);
    }
    // iter-129: always-present key discipline (see the plan's `meta.frame_url`
    // note and iter-128's `hint` regression) — present and `null` on the
    // top-frame path, never omitted, so `--jq '.results.frame_url'` never
    // throws regardless of which path served the click.
    click_json["frame_url"] = json!(frame_url);

    // C2: --settle (network + DOM idle).
    let settle_method = if opts.settle {
        let sm = settle_page(&mut ctx, &console_actor, wait_timeout_ms)?;
        Some(sm)
    } else {
        None
    };

    // C1: --wait-for predicates.
    if !opts.wait_for.is_empty() {
        let wf_timeout = opts.wait_for_timeout_ms.unwrap_or(wait_timeout_ms);
        let predicates: Vec<WaitForPredicate<'_>> = opts
            .wait_for
            .iter()
            .map(|s| WaitForPredicate::parse(s))
            .collect::<Result<_, _>>()?;
        wait_for_predicates(&mut ctx, &console_actor, &predicates, wf_timeout)?;
    }

    // Gather the network result if requested.
    let network_result = if let Some(pattern) = wait_for_network {
        let timeout_ms = network_timeout.unwrap_or(cli.timeout);
        let matched = if ctx.via_daemon {
            wait_for_matching_request_daemon(&mut ctx, pattern, timeout_ms)?
        } else {
            wait_for_matching_request_direct(&mut ctx, pattern, timeout_ms)?
        };
        Some(matched)
    } else {
        None
    };

    // Clean up subscriptions after we have the result.
    if let Some(ref watcher_actor) = watcher_sub {
        let _ =
            WatcherActor::unwatch_resources(ctx.transport_mut(), watcher_actor, &["network-event"]);
    }
    if daemon_streaming {
        use crate::daemon::client::stop_daemon_stream;
        let _ = stop_daemon_stream(ctx.transport_mut(), "network-event");
    }

    // Build the output.
    let mut result = click_json;
    if let Some(net) = network_result {
        result["network"] = net;
    }
    if let Some(sm) = settle_method {
        result["settle_method"] = json!(sm.as_meta_str());
    }
    // iter-140 Theme B/C: when --visible/--index disambiguated an ambiguous
    // selector, report how many elements matched and which was chosen — the
    // same transparency the plan asks for on the failure path, surfaced here
    // on success so `--visible`/`--index` calls aren't silent about it.
    if let Some((match_count, chosen_index)) = disambiguation {
        result["match_count"] = json!(match_count);
        result["chosen_index"] = json!(chosen_index);
    }

    // iter-210 Theme A: `--with-page`. Last, on the connection the click
    // already owns, and after `--settle`/`--wait-for`/`--wait-for-network` —
    // a click that navigates must report the DESTINATION page, not the one it
    // left, which is the whole reason the flag exists (see `page_view`).
    if opts.page.with_page {
        super::page_view::attach(cli, &mut ctx, &mut result, Some(wait_timeout_ms), &opts.page)?;
    }

    Ok((result, ctx.via_daemon))
}

pub fn run(
    cli: &Cli,
    selector: &str,
    wait_for_network: Option<&str>,
    network_timeout: Option<u64>,
    opts: &ClickOptions<'_>,
) -> Result<(), AppError> {
    let (mut result, via_daemon) =
        run_core(cli, selector, wait_for_network, network_timeout, opts)?;

    // Preserve the pre-iter-61c CLI output shape: `settle_method` belongs in
    // `meta`, not in `results`.  The script runner reads it from `results`
    // (where `run_core` placed it) and re-emits it in its own NDJSON line.
    let settle_method = result
        .as_object_mut()
        .and_then(|o| o.remove("settle_method"));
    // iter-140 Theme E: `--help` documents `frame_url` as present in BOTH
    // `results` AND `meta` (never omitted from either) — the code used to
    // `.remove()` it from `results` here, so `--jq '.results.frame_url'`
    // threw on every call. Copy instead of removing so it stays in both.
    let frame_url = result.get("frame_url").cloned().unwrap_or(Value::Null);
    let mut meta = json!({"selector": selector});
    if let Some(sm) = settle_method {
        meta["settle_method"] = sm;
    }
    meta["frame_url"] = frame_url;
    let page_text = super::page_view::lift_meta(cli, &mut result, &mut meta);
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose.
    crate::connection_meta::merge_route(&mut meta, via_daemon);
    let envelope = output::envelope(&result, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::Click).with_selector(selector);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))?;
    super::page_view::render_text_section(page_text.as_ref());
    Ok(())
}

/// Cheap, single-shot existence probe: does `selector` match anything in the
/// document evaluated by `console_actor`? No polling, no stability check —
/// just `!!document.querySelector(...)`. Used to decide whether the (slow,
/// polling) top-level `autowait_element` is worth running at all, and to
/// check candidate frames without paying for a full auto-wait in each.
fn selector_exists(
    ctx: &mut ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    selector: &str,
) -> Result<bool, AppError> {
    let escaped = escape_selector(selector);
    let js = format!("!!document.querySelector('{escaped}')");
    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, &js)
        .map_err(AppError::from)?;
    if eval_result.exception.is_some() {
        return Ok(false);
    }
    Ok(matches!(eval_result.result, Grip::Value(Value::Bool(true))))
}

/// Enumerate this tab's frame targets — the one shared entry point every
/// frame-aware call site in this file goes through.
///
/// Delegates to [`crate::commands::frame_targets::fetch_frame_targets`], which
/// picks the mechanism the current connection supports: the daemon's recorded
/// target snapshot when proxied, the live `watchTargets` drain when direct.
/// Before iter-137 this always took the direct path, which is a no-op through
/// the daemon (the daemon already subscribed at startup) — so `--frame` and
/// the cross-origin frame scan reported "0 frame(s) available" for every
/// invocation that did not pass `--no-daemon`.
///
/// **Callers MUST NOT call this more than once per `click` invocation.**
/// `enumerate_frame_targets` deliberately never sends `unwatchTargets` (see
/// its doc comment — unwatching under `isServerTargetSwitchingEnabled`
/// destroys every target it just returned), which means a *second*
/// `watchTargets("frame")` on an already-watched connection is a no-op: it
/// does **not** re-deliver the already-known targets, so a second call here
/// silently returns an empty list on a direct connection. Confirmed live
/// against Firefox 153 — this is why the auto-wait pre-check threads its
/// fetched `Vec<TargetEvent>` through to `do_click` (`prefetched_targets`)
/// instead of letting `click_in_scanned_frame` re-enumerate.
fn fetch_frame_targets(ctx: &mut ConnectedTab) -> Result<Vec<TargetEvent>, AppError> {
    crate::commands::frame_targets::fetch_frame_targets(ctx)
}

/// [`selector_exists`] against every non-top frame in `targets`,
/// short-circuiting on the first match. Takes an already-fetched target list
/// (see [`fetch_frame_targets`]'s single-enumerate-per-invocation rule).
fn selector_exists_in_targets(
    ctx: &mut ConnectedTab,
    targets: &[TargetEvent],
    selector: &str,
) -> Result<bool, AppError> {
    for target in targets.iter().filter(|t| !t.is_top_level) {
        let Some(console_actor) = target.console_actor.as_ref() else {
            continue;
        };
        if selector_exists(ctx, console_actor, selector)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Substring the click JS's thrown error carries when the selector matches
/// nothing in the evaluated document — see `build_click_js` / the
/// `ClickOnly` JS below. Used to distinguish "wrong frame, try scanning" from
/// a genuine JS failure (which must propagate immediately, not trigger a
/// pointless frame scan).
const ELEMENT_NOT_FOUND_MARKER: &str = "Element not found:";

/// Classify a thrown click-JS exception message.
///
/// `None` means "element not found" — the caller should proceed to (or
/// continue) the frame scan. `Some(err)` means a genuine JS failure that must
/// be surfaced immediately rather than triggering (or continuing) a scan that
/// cannot help.
///
/// iter-145 Theme A: the genuine-failure case routes through the standard
/// JSON error envelope (`AppError::User`, matching `eval.rs`'s iter-141
/// Theme E handling of a thrown script exception) instead of printing bare
/// text to stderr and bypassing `main`'s envelope emission via
/// `AppError::Exit(1)`. A thrown click-JS exception is the caller's
/// selector/page misbehaving, not an ff-rdp bug, so `User` is the right
/// classification — same reasoning as `eval.rs`. Shared by both call sites
/// (`do_click`'s top-level attempt and `click_in_scanned_frame`'s per-frame
/// retry) so the classification can't drift between them.
fn classify_click_exception(msg: &str) -> Option<AppError> {
    if msg.contains(ELEMENT_NOT_FOUND_MARKER) {
        None
    } else {
        Some(AppError::User(sanitize_for_terminal(msg).into_owned()))
    }
}

/// Click `selector`, returning the parsed result JSON and — when the click
/// landed inside a non-top frame — that frame's URL.
///
/// iter-129 frame-aware click (Theme B): tries the top-level document first
/// (identical to pre-iter-129 behaviour, no watcher round-trip). Only on a
/// selector-not-found result does it enumerate frame targets and retry the
/// same click JS against each non-top frame's own console actor — the
/// mechanism verified in `kb/research/frame-targets.md` for reaching
/// cross-origin CMP iframes (e.g. Sourcepoint on theguardian.com).
///
/// `frame_filter`, when set (`--frame <url-substring>`), skips both the
/// top-level attempt and the open-ended scan and clicks directly inside the
/// first frame whose URL contains the substring.
///
/// `prefetched_targets`, when `Some`, means `run_core`'s auto-wait pre-check
/// already called [`fetch_frame_targets`] and confirmed the selector is
/// absent from the top document — `do_click` skips its own top-level attempt
/// entirely (both to honour that finding and because a *second*
/// `fetch_frame_targets` call in the same connection would silently return
/// nothing; see that function's doc comment) and scans the given targets
/// directly.
fn do_click(
    ctx: &mut ConnectedTab,
    selector: &str,
    mode: DispatchMode,
    frame_filter: Option<&str>,
    prefetched_targets: Option<Vec<TargetEvent>>,
) -> Result<(Value, Option<String>), AppError> {
    let escaped = escape_selector(selector);
    let js = build_click_js_for_mode(&escaped, mode);

    if let Some(targets) = prefetched_targets {
        return click_in_scanned_frame(ctx, selector, &js, &targets, frame_filter);
    }
    if frame_filter.is_some() {
        let targets = fetch_frame_targets(ctx)?;
        return click_in_scanned_frame(ctx, selector, &js, &targets, frame_filter);
    }

    let console_actor = ctx.target.console_actor.clone();
    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    let Some(exc) = eval_result.exception else {
        let json_val = resolve_result(ctx, &eval_result.result)?;
        return Ok((json_val, None));
    };

    let msg = exc.message.unwrap_or_else(|| "click failed".to_owned());
    if let Some(err) = classify_click_exception(&msg) {
        // A genuine JS failure (not a missing-selector case) — surface it
        // immediately rather than paying for a frame scan that cannot help.
        return Err(err);
    }

    let targets = fetch_frame_targets(ctx)?;
    click_in_scanned_frame(ctx, selector, &js, &targets, None)
}

/// Build the click JS for the given dispatch mode (shared by the top-level
/// attempt and every frame-scan retry — same JS, different console actor).
///
/// iter-160 Theme A: `ClickOnly` used to be a hand-written copy of the JS here
/// rather than a [`build_click_js`] mode, and that copy is exactly how the
/// hardcoded `entered: true` literal survived in one dispatch mode after being
/// computed in the other two. The centre-point hit test has to run in all three
/// modes — `el.click()` is no more able to reach a covered button than
/// `dispatchEvent` is — so there is one producer now and this is a thin
/// delegation kept because both the top-level attempt and the frame scan name it.
fn build_click_js_for_mode(escaped_selector: &str, mode: DispatchMode) -> String {
    build_click_js(escaped_selector, mode)
}

/// Turn a click JS result that reports `reachable: false` into the error the
/// caller must see (iter-160 Theme A).
///
/// An obscured or off-screen click is a **failed action**, not an informational
/// outcome: a caller writing `ff-rdp click X && ff-rdp type Y …` has to stop.
/// Returns `None` when the click did land, so the success envelope flows on
/// unchanged.
///
/// Reuses [`AppError::Unsupported`] (exit 1, stable `error_type`) rather than
/// introducing a variant, and merges `matched` / `reachable` / `obscured_by`
/// into the error envelope so the JSON a failing caller parses names the
/// covering element instead of only saying "no".
///
/// Only a literal `reachable: false` fails. `reachable: null` — the hit test
/// could not decide, e.g. inside an out-of-process iframe whose document was
/// never laid out — is **not** a failure: the events were dispatched and the
/// envelope says the verdict is unknown. Turning "I could not tell" into exit 1
/// would be the same overstatement this iteration removes, pointed the other way.
fn unreachable_click_error(selector: &str, result: &Value) -> Option<AppError> {
    if result.get("reachable").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let obscured_by = result.get("obscured_by").and_then(Value::as_str);
    let (error_type, message) = match obscured_by {
        Some(desc) => (
            "click_obscured",
            format!(
                "selector '{selector}' matched an element that is covered by {desc} at its centre \
                 point — no click was dispatched. Dismiss the overlay first (e.g. ff-rdp consent \
                 accept), or target {desc} if that is what you meant to click."
            ),
        ),
        None => (
            "click_offscreen",
            format!(
                "selector '{selector}' matched an element whose centre point is still outside the \
                 viewport after scrolling it into view — no click was dispatched. It may be inside \
                 a clipped or zero-size scroll container; check with \
                 `ff-rdp geometry '{selector}'`."
            ),
        ),
    };
    Some(AppError::Unsupported {
        error_type,
        message,
        details: Some(json!({
            "matched": result.get("matched").cloned().unwrap_or(json!(true)),
            "reachable": false,
            "obscured_by": obscured_by,
        })),
    })
}

/// Try `js` against each non-top frame in `targets` (or, with
/// `frame_filter`, only frames whose URL contains the substring) — an
/// already-fetched target list, per [`fetch_frame_targets`]'s
/// single-enumerate-per-invocation rule.
///
/// Returns `Ok((result, Some(frame_url)))` for the first frame where `js`
/// does not throw. Returns a descriptive `AppError::User` — never the bare
/// upstream timeout — when `frame_filter` matches no frame, or when every
/// candidate frame's eval still throws the not-found error.
// iter-140 Theme D: on a many-frame page (theguardian.com: 97 frames, most
// of them consent-string-laden ad iframe URLs) joining every URL raw
// produced a 65 KB error message. Cap both the number of URLs listed and
// each URL's length (reusing iter-128's `middle_ellipsis`, already wired
// into the same shape of problem for `perf`/`network` — see iter-139).
const MAX_LISTED_FRAME_URLS: usize = 10;
const FRAME_URL_MAX_LEN: usize = 80;

fn click_in_scanned_frame(
    ctx: &mut ConnectedTab,
    selector: &str,
    js: &str,
    targets: &[TargetEvent],
    frame_filter: Option<&str>,
) -> Result<(Value, Option<String>), AppError> {
    let candidates: Vec<_> = targets
        .iter()
        .filter(|t| !t.is_top_level)
        .filter(|t| match frame_filter {
            None => true,
            Some(f) => t.url.as_deref().is_some_and(|u| u.contains(f)),
        })
        .collect();

    let bounded_urls = |targets: &[&TargetEvent]| -> String {
        let total = targets.len();
        let listed: Vec<String> = targets
            .iter()
            .take(MAX_LISTED_FRAME_URLS)
            .map(|t| {
                crate::output::middle_ellipsis(
                    t.url.as_deref().unwrap_or("<no-url>"),
                    FRAME_URL_MAX_LEN,
                )
            })
            .collect();
        if total > MAX_LISTED_FRAME_URLS {
            format!(
                "{} (+{} more)",
                listed.join(", "),
                total - MAX_LISTED_FRAME_URLS
            )
        } else {
            listed.join(", ")
        }
    };

    if let Some(filter) = frame_filter
        && candidates.is_empty()
    {
        let all: Vec<&TargetEvent> = targets.iter().collect();
        return Err(AppError::User(format!(
            "click --frame '{filter}' matched no frame ({} frame(s) available: {})",
            targets.len(),
            bounded_urls(&all)
        )));
    }

    for target in &candidates {
        let Some(console_actor) = target.console_actor.as_ref() else {
            continue;
        };
        let eval_result =
            WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, js)
                .map_err(AppError::from)?;
        let Some(exc) = eval_result.exception else {
            let json_val = resolve_result(ctx, &eval_result.result)?;
            let frame_url = target.url.clone().unwrap_or_default();
            return Ok((json_val, Some(frame_url)));
        };
        let msg = exc.message.unwrap_or_default();
        if let Some(err) = classify_click_exception(&msg) {
            // A genuine JS failure inside this frame — surface it directly
            // rather than silently trying the next candidate.
            return Err(err);
        }
    }

    // Nothing matched anywhere — the informative, frame-aware diagnostic
    // that replaces the old bare "element not found" / 10s timeout.
    //
    // iter-140 Theme D: this must count `candidates` — the frames actually
    // tried — not `targets.len()`. With `--frame guim` on a 97-frame page,
    // `--frame` narrows the scan to a handful of candidates; reporting
    // "matched in 0 of 97 frames" claimed every frame was tried when only the
    // filtered subset was.
    let tried = candidates.len();
    let total = targets.len();
    Err(AppError::User(format!(
        "click: selector {selector:?} matched in 0 of {tried} frame(s) tried (of {total} total): {}",
        bounded_urls(&candidates)
    )))
}

/// Wait for a resolved network request matching `pattern` using the daemon stream.
///
/// The daemon is already streaming events to us (started before the click).
/// We read the stream until we find a completed request whose URL contains
/// `pattern`, or until the timeout fires.
fn wait_for_matching_request_daemon(
    ctx: &mut ConnectedTab,
    pattern: &str,
    timeout_ms: u64,
) -> Result<Value, AppError> {
    let timeout = Duration::from_millis(timeout_ms);
    let started = Instant::now();

    let mut pending: std::collections::HashMap<u64, NetworkResource> =
        std::collections::HashMap::new();

    // Cap per-recv blocking via POLL_INTERVAL so the wall-clock deadline is
    // honored even when the global transport read timeout is larger than the
    // requested --network-timeout.  Restored to the global value before
    // returning so subsequent transport reads behave normally.
    let _ = ctx.transport_mut().set_read_timeout(Some(POLL_INTERVAL));

    let outcome = run_wait_loop(ctx, pattern, timeout, started, timeout_ms, &mut pending);

    let _ = ctx.transport_mut().set_read_timeout(None);

    outcome
}

fn run_wait_loop(
    ctx: &mut ConnectedTab,
    pattern: &str,
    timeout: Duration,
    started: Instant,
    timeout_ms: u64,
    pending: &mut std::collections::HashMap<u64, NetworkResource>,
) -> Result<Value, AppError> {
    loop {
        if started.elapsed() >= timeout {
            return Err(AppError::Timeout(format!(
                "no network request matching '{pattern}' completed within {timeout_ms}ms"
            )));
        }

        match ctx.transport_mut().recv() {
            Ok(msg) => {
                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or_default();
                match msg_type {
                    "resources-available-array" => {
                        for res in parse_network_resources(&msg) {
                            if res.url.contains(pattern) {
                                pending.insert(res.resource_id, res);
                            }
                        }
                    }
                    "resources-updated-array" => {
                        for update in parse_network_resource_updates(&msg) {
                            if let Some(res) = pending.remove(&update.resource_id) {
                                if update.status.is_some() {
                                    return Ok(build_matched_entry(&res, &update));
                                }
                                // Status not yet available — put it back.
                                pending.insert(res.resource_id, res);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(ProtocolError::Timeout) => {
                // Per-read timeout — check wall-clock deadline on next iteration.
            }
            Err(e) => return Err(AppError::from(e)),
        }
    }
}

/// Wait for a resolved network request matching `pattern` in direct (non-daemon) mode.
///
/// The watcher subscription was already set up before the click. We drain
/// events from the transport until we find a completed matching request or
/// the timeout fires.
fn wait_for_matching_request_direct(
    ctx: &mut ConnectedTab,
    pattern: &str,
    timeout_ms: u64,
) -> Result<Value, AppError> {
    // Reuse the same loop logic — the transport delivers watcher events the same
    // way in direct mode; the watcher subscription was set up before the click.
    wait_for_matching_request_daemon(ctx, pattern, timeout_ms)
}

/// Build a single network entry JSON from a matched resource + its update.
fn build_matched_entry(res: &NetworkResource, update: &NetworkResourceUpdate) -> Value {
    let mut entries = build_network_entries(
        std::slice::from_ref(res),
        &std::iter::once((res.resource_id, update.clone())).collect(),
    );
    entries.pop().unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B1 acceptance: verify the pointer-dispatch JS payload contains the full
    /// event sequence for Radix/Headless-UI compatibility.
    #[test]
    fn pointer_dispatch_js_contains_full_event_sequence() {
        let js = build_click_js("button", DispatchMode::Pointer);
        // Must include all five semantic events.
        assert!(js.contains("pointerover"), "missing pointerover: {js}");
        assert!(js.contains("pointerenter"), "missing pointerenter: {js}");
        assert!(js.contains("pointerdown"), "missing pointerdown: {js}");
        assert!(js.contains("pointerup"), "missing pointerup: {js}");
        assert!(js.contains("'click'"), "missing click: {js}");
        // Must use PointerEvent constructor.
        assert!(js.contains("PointerEvent"), "missing PointerEvent: {js}");
        // Must include the sentinel so the result can be decoded.
        assert!(
            js.contains(crate::commands::js_helpers::JSON_SENTINEL),
            "missing JSON_SENTINEL: {js}"
        );
    }

    #[test]
    fn pointer_dispatch_js_includes_button_buttons_opts() {
        let js = build_click_js("button", DispatchMode::Pointer);
        // pointerdown/mousedown must have button:0, buttons:1.
        assert!(
            js.contains("buttons: 1"),
            "missing buttons:1 for down: {js}"
        );
        // pointerup/mouseup/click must have button:0, buttons:0.
        assert!(js.contains("buttons: 0"), "missing buttons:0 for up: {js}");
    }

    #[test]
    fn legacy_dispatch_js_uses_mouse_events_only() {
        let js = build_click_js("button", DispatchMode::Legacy);
        assert!(js.contains("MouseEvent"), "missing MouseEvent: {js}");
        assert!(
            !js.contains("PointerEvent"),
            "should not have PointerEvent: {js}"
        );
        assert!(js.contains("mousedown"), "missing mousedown: {js}");
        assert!(js.contains("mouseup"), "missing mouseup: {js}");
    }

    #[test]
    fn click_only_dispatch_js_uses_dot_click() {
        // iter-160: ClickOnly is no longer a hand-written copy in `do_click`;
        // it is a `build_click_js` mode like the other two, so assert against
        // the JS the command actually sends rather than a re-typed lookalike.
        let js = build_click_js(&escape_selector("button.submit"), DispatchMode::ClickOnly);
        assert!(js.contains("el.click()"), "missing el.click(): {js}");
        assert!(js.contains(crate::commands::js_helpers::JSON_SENTINEL));
    }

    // ── iter-129 Theme B: frame-aware click ─────────────────────────────────

    #[test]
    fn build_click_js_for_mode_click_only_matches_manual_form() {
        let js = build_click_js_for_mode("button.submit", DispatchMode::ClickOnly);
        assert!(js.contains("el.click()"));
        assert!(js.contains(ELEMENT_NOT_FOUND_MARKER));
    }

    #[test]
    fn build_click_js_for_mode_pointer_delegates_to_build_click_js() {
        let js = build_click_js_for_mode("button", DispatchMode::Pointer);
        assert!(js.contains("PointerEvent"));
    }

    /// AC: `live_129_click_zero_match_error` (message-shape half) — the
    /// not-found marker used to trigger a frame scan is the exact substring
    /// every click JS variant throws, so the fast top-level path and the
    /// per-frame retries share one detection rule.
    #[test]
    fn element_not_found_marker_matches_all_click_js_variants() {
        for mode in [
            DispatchMode::Pointer,
            DispatchMode::Legacy,
            DispatchMode::ClickOnly,
        ] {
            let js = build_click_js_for_mode("button", mode);
            assert!(
                js.contains(ELEMENT_NOT_FOUND_MARKER),
                "mode {mode:?} JS missing the not-found marker: {js}"
            );
        }
    }

    // ── iter-160 Theme A/B: the click envelope says what it knows ──────────

    /// AC `unit_160_click_js_hit_tests_centre_point`: the dispatched JS
    /// consults the page's hit-test tree, and does so *before* it dispatches
    /// anything — a hit test that ran afterwards would tell the caller the
    /// truth about a click it had already fired blind.
    #[test]
    fn unit_160_click_js_hit_tests_centre_point() {
        for mode in [
            DispatchMode::Pointer,
            DispatchMode::Legacy,
            DispatchMode::ClickOnly,
        ] {
            let js = build_click_js_for_mode("button", mode);
            assert!(
                js.contains("getBoundingClientRect"),
                "mode {mode:?}: no rect read: {js}"
            );
            assert!(
                js.contains("elementFromPoint"),
                "mode {mode:?}: no hit test: {js}"
            );
            assert!(
                js.contains("el.contains("),
                "mode {mode:?}: no descendant check — a <span> inside a <button> \
                 must count as reachable: {js}"
            );
            assert!(
                js.contains("hit.contains(el)"),
                "mode {mode:?}: no ancestor check — `click body` hit-tests to <html>, \
                 and an ancestor cannot obscure its own descendant: {js}"
            );
            assert!(
                js.contains("scrollIntoView"),
                "mode {mode:?}: below-the-fold is not an obstruction — the element \
                 must be scrolled into view before the verdict: {js}"
            );
            let hit_at = js.find("elementFromPoint").expect("hit test present");
            if let Some(dispatch_at) = js.find("dispatchEvent") {
                assert!(
                    hit_at < dispatch_at,
                    "mode {mode:?}: hit test must precede the first dispatchEvent: {js}"
                );
            }
        }
    }

    /// AC `unit_160_click_result_reports_matched_and_reachable`: the result
    /// JSON names the two separate claims, and the old `entered` — which meant
    /// "querySelector was non-null" while its name said "the pointer could
    /// enter" — is gone from every dispatch mode.
    #[test]
    fn unit_160_click_result_reports_matched_and_reachable() {
        for mode in [
            DispatchMode::Pointer,
            DispatchMode::Legacy,
            DispatchMode::ClickOnly,
        ] {
            for js in [
                build_click_js("button", mode),
                build_click_js_for_mode("button", mode),
            ] {
                assert!(js.contains("matched:"), "mode {mode:?}: no matched: {js}");
                assert!(
                    js.contains("reachable:"),
                    "mode {mode:?}: no reachable: {js}"
                );
                assert!(
                    js.contains("obscured_by:"),
                    "mode {mode:?}: no obscured_by: {js}"
                );
                assert!(
                    !js.contains("entered"),
                    "mode {mode:?}: `entered` survived: {js}"
                );
            }
        }
    }

    #[test]
    fn unit_160_unreachable_click_error_names_the_covering_element() {
        let result = json!({
            "clicked": false, "matched": true, "reachable": false,
            "obscured_by": "div#veil", "offscreen": false,
        });
        let err = unreachable_click_error("#t", &result).expect("must be an error");
        assert_eq!(err.error_type(), "click_obscured");
        assert_eq!(err.exit_code(), 1);
        let json = err.to_error_json();
        assert_eq!(json["obscured_by"], json!("div#veil"));
        assert_eq!(json["matched"], json!(true));
        assert_eq!(json["reachable"], json!(false));
        assert!(
            err.to_string().contains("div#veil"),
            "human message must name the overlay: {err}"
        );
    }

    #[test]
    fn unit_160_offscreen_click_is_a_distinct_error_type() {
        // `elementFromPoint` returns null outside the viewport — that is not
        // an overlay and must not be reported as one.
        let result = json!({
            "clicked": false, "matched": true, "reachable": false,
            "obscured_by": Value::Null, "offscreen": true,
        });
        let err = unreachable_click_error("#t", &result).expect("must be an error");
        assert_eq!(err.error_type(), "click_offscreen");
        assert_eq!(err.to_error_json()["obscured_by"], Value::Null);
    }

    /// An indeterminate hit test (`reachable: null`) is not a failure. Measured
    /// inside the out-of-process iframe `live_129_click_cross_origin_frame`
    /// clicks: an ordinary `<a>` hit-tests to `null` because the child document
    /// was never laid out. Reporting that as off-screen would break every
    /// cross-origin frame click to satisfy a verdict the page never gave.
    #[test]
    fn unit_160_indeterminate_hit_test_is_not_an_error() {
        let result = json!({
            "clicked": true, "matched": true, "reachable": Value::Null,
            "obscured_by": Value::Null, "offscreen": false,
        });
        assert!(unreachable_click_error("a", &result).is_none());
    }

    #[test]
    fn unit_160_reachable_click_produces_no_error() {
        let result = json!({"clicked": true, "matched": true, "reachable": true});
        assert!(unreachable_click_error("#t", &result).is_none());
    }

    #[test]
    fn click_options_default_frame_is_none() {
        let opts = ClickOptions::default();
        assert!(opts.frame.is_none());
    }

    #[test]
    fn clap_click_frame_flag_parses() {
        use crate::cli::args::Command;
        use clap::Parser as _;

        let cli = Cli::try_parse_from(["ff-rdp", "click", "button", "--frame", "sourcepoint"])
            .expect("should parse --frame");
        let Command::Click(args) = cli.command else {
            panic!("expected Click command");
        };
        assert_eq!(args.frame.as_deref(), Some("sourcepoint"));
    }

    #[test]
    fn clap_click_without_frame_flag_defaults_to_none() {
        use crate::cli::args::Command;
        use clap::Parser as _;

        let cli = Cli::try_parse_from(["ff-rdp", "click", "button"])
            .expect("should parse without --frame");
        let Command::Click(args) = cli.command else {
            panic!("expected Click command");
        };
        assert!(args.frame.is_none());
    }

    // ── iter-145 Theme A: click JS exceptions route through the envelope ───

    /// AC: `unit_145_click_exception_maps_to_user_error_type` — a thrown JS
    /// exception during click maps to `error_type: "User"`, not `Internal`.
    #[test]
    fn unit_145_click_exception_maps_to_user_error_type() {
        let err = classify_click_exception("TypeError: something broke")
            .expect("a genuine JS exception must classify as an error, not be swallowed");
        assert!(
            matches!(err, AppError::User(_)),
            "thrown click exception must map to error_type User, not Internal: {err:?}"
        );
    }

    #[test]
    fn classify_click_exception_element_not_found_returns_none() {
        // The frame-scan salvage marker must NOT classify as a genuine
        // failure — it means "keep scanning frames", per
        // `ELEMENT_NOT_FOUND_MARKER`'s doc comment.
        assert!(
            classify_click_exception(
                "Element not found: button — use ff-rdp dom SELECTOR --count to verify the selector matches"
            )
            .is_none()
        );
    }

    #[test]
    fn classify_click_exception_sanitizes_message_for_terminal() {
        // The classified message must go through `sanitize_for_terminal`
        // (control chars stripped) — the same treatment `eval.rs` applies to
        // thrown exception text before it reaches the JSON envelope.
        let err = classify_click_exception("boom\x1b[31mred\x1b[0m").unwrap();
        let AppError::User(msg) = err else {
            panic!("expected AppError::User, got {err:?}");
        };
        assert!(
            !msg.contains('\x1b'),
            "raw ANSI escape must be stripped: {msg:?}"
        );
    }
}
