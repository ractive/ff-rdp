use std::time::Duration;

use ff_rdp_core::{
    AccessibilityActor, AccessibleNode, ActorId, ProtocolError, RootActor, WebConsoleActor,
    filter_interactive,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_direct};
use super::js_helpers::{escape_selector, eval_or_bail, resolve_result};

/// Which tree an `a11y` response came from (iter-143 Theme A).
///
/// Reported unconditionally in `meta.source` via
/// [`crate::connection_meta::merge_source`] — see [DEC-027] and
/// `kb/iterations/iteration-143-native-a11y-tree.md`.
///
/// [DEC-027]: ../../../../kb/decision-log.md
enum A11ySource {
    /// The real Firefox platform accessibility tree (roles like `document`,
    /// `paragraph`, `link`).
    Native,
    /// A DOM-derived approximation (roles like `generic`) built by evaluating
    /// JS in the page — cannot see anything the platform computes but the DOM
    /// does not expose. `reason` names why this path ran instead of native.
    JsFallback(&'static str),
}

impl A11ySource {
    fn merge_into(&self, meta: &mut Value) {
        match self {
            Self::Native => crate::connection_meta::merge_source(meta, "native", None),
            Self::JsFallback(reason) => {
                crate::connection_meta::merge_source(meta, "js-fallback", Some(reason));
            }
        }
    }
}

/// Outcome of attempting to restore the platform accessibility service to
/// its pre-command state after an opt-in `--native` run (iter-149, follow-up
/// to iter-143 Theme B).
///
/// Threaded from [`run_native_opt_in`] to the envelope via
/// [`RestoreOutcome::merge_into`] so a failed restore is visible in the
/// default (non-`--verbose`) JSON output, not only on stderr behind
/// `--verbose`. See `kb/iterations/iteration-149-a11y-restore-honesty.md`.
///
/// A failed `disable()` does *not* necessarily mean Firefox's platform
/// accessibility service stays enabled indefinitely: verified live
/// (`live_149_service_already_on_is_not_touched`), Firefox tears the service
/// back down once the connection that enabled it disconnects — which for
/// this one-shot CLI connection happens moments after this outcome is
/// computed, whether or not `disable()` itself succeeded. The risk this
/// variant reports is real but narrow: the service stays on for the
/// remainder of *this* connection's lifetime, longer for a caller that holds
/// the connection open (e.g. an embedding), not "until Firefox restarts" —
/// see `kb/rdp/actors/accessibility.md` Gotchas.
#[derive(Debug, PartialEq, Eq)]
enum RestoreOutcome {
    /// ff-rdp did not enable the service for this call — either it was
    /// already enabled before the command ran (so this call must not touch
    /// it), or the call never took the `--native` opt-in path at all.
    /// Either way there was nothing to restore.
    NotNeeded,
    /// ff-rdp enabled the service and successfully restored it to disabled
    /// afterward.
    Restored,
    /// ff-rdp enabled the service but `disable_service` failed. The `String`
    /// is the formatted [`ff_rdp_core::ProtocolError`] — on Windows this is
    /// typically an active screen reader blocking `disable`
    /// (`kb/rdp/actors/accessibility.md`), an expected platform limitation
    /// that nonetheless must be reported.
    Failed(String),
}

impl RestoreOutcome {
    /// Always inserts both `service_left_enabled` (bool) and
    /// `service_restore_error` (nullable string) — the iter-128
    /// always-present-nullable-key convention (see
    /// `kb/iterations/iteration-128-network-hint-always-present.md`), the
    /// same treatment [`A11ySource::merge_into`] already gives `meta.source`.
    /// A caller never has to special-case "key absent means nothing went
    /// wrong".
    fn merge_into(&self, meta: &mut Value) {
        let (left_enabled, error) = match self {
            RestoreOutcome::NotNeeded | RestoreOutcome::Restored => (false, None),
            RestoreOutcome::Failed(reason) => (true, Some(reason.clone())),
        };
        if let Some(obj) = meta.as_object_mut() {
            obj.insert("service_left_enabled".to_string(), json!(left_enabled));
            obj.insert("service_restore_error".to_string(), json!(error));
        }
    }
}

/// Purpose-specific ceiling for accessibility walker requests (iter-143 Theme
/// C). The walker's root accessor stalls — it does not error — while the
/// platform accessibility service is off (iter-136): Firefox's
/// `document-ready` promise never settles. `run_native_or_js_fallback`
/// already checks `bootstrap().state.enabled` first, but a race (the service
/// getting disabled between that check and the walk) or a future call site
/// that skips the check would otherwise stall for the full `--timeout`
/// (default 10s, but user-configurable much higher). Bounding walker requests
/// to this instead means a mistake costs a few seconds, not the caller's
/// whole configured timeout.
const A11Y_WALKER_TIMEOUT: Duration = Duration::from_secs(3);

/// Run `f` with the transport's read timeout temporarily narrowed to
/// [`A11Y_WALKER_TIMEOUT`], restoring the previous value afterwards
/// regardless of whether `f` succeeded.
fn with_walker_timeout<T>(
    ctx: &mut ConnectedTab,
    f: impl FnOnce(&mut ff_rdp_core::RdpTransport) -> Result<T, ProtocolError>,
) -> Result<T, ProtocolError> {
    let previous = ctx.transport_mut().read_timeout().unwrap_or(None);
    let _ = ctx
        .transport_mut()
        .set_read_timeout(Some(A11Y_WALKER_TIMEOUT));
    let result = f(ctx.transport_mut());
    let _ = ctx.transport_mut().set_read_timeout(previous);
    result
}

pub fn run(
    cli: &Cli,
    depth: u32,
    max_chars: u32,
    selector: Option<&str>,
    interactive: bool,
    native: bool,
) -> Result<(), AppError> {
    let mut ctx = connect_direct(cli)?;

    let accessibility_actor = ctx.target.accessibility_actor.clone().ok_or_else(|| {
        AppError::User(
            "no accessibility actor available — accessibility may not be enabled in Firefox"
                .to_string(),
        )
    })?;

    // If selector is provided, use JS eval approach (similar to snapshot).
    // `--native` conflicts with `--selector`/`--ref` at the clap level (both
    // are inherently JS-derived paths — there is no native "root at
    // selector" primitive), so at most one of these two branches applies.
    let (tree, source, restore_outcome) = if let Some(sel) = selector {
        (
            run_selector_mode(&mut ctx, sel, depth, max_chars)?,
            A11ySource::JsFallback("selector-mode"),
            RestoreOutcome::NotNeeded,
        )
    } else if native {
        // Theme B: explicit opt-in to the platform tree. Never silently
        // falls back — any failure (enable failing, bootstrap still
        // reporting disabled, a stalled/erroring walker request) surfaces as
        // an explicit error. iter-149: the restore outcome is threaded
        // through instead of discarded, so a failed restore can be reported
        // in the envelope below rather than only on --verbose stderr.
        let (tree, restore_outcome) =
            run_native_opt_in(&mut ctx, &accessibility_actor, depth, max_chars, cli)?;
        (tree, A11ySource::Native, restore_outcome)
    } else {
        // Use native RDP protocol with JS eval fallback for Firefox 149+ where
        // both `getDocument` and `getRootNode` are unrecognized on the walker.
        // This path never enables the service itself, so there is nothing to
        // restore.
        let (tree, source) =
            run_native_or_js_fallback(&mut ctx, &accessibility_actor, depth, max_chars, cli)?;
        (tree, source, RestoreOutcome::NotNeeded)
    };

    // Apply interactive filter.
    let tree = if interactive {
        filter_interactive(&tree).unwrap_or_else(|| AccessibleNode {
            actor: None,
            role: "document".to_string(),
            name: Some("(no interactive elements)".to_string()),
            value: None,
            description: None,
            child_count: None,
            states: vec![],
            dom_node_type: None,
            index_in_parent: None,
            children: vec![],
            truncated: None,
        })
    } else {
        tree
    };

    // Strip internal actor IDs from output (not useful to end users).
    let mut tree_value = serde_json::to_value(&tree).map_err(|e| AppError::Internal(e.into()))?;
    strip_actor_ids(&mut tree_value);

    let mut meta = json!({
        "depth": depth,
        "max_chars": max_chars,
    });
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose — an
    // agent can tell how this command executed without a
    // separate `daemon status` round-trip.
    crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);
    // iter-143 Theme A: always present — the only way a caller can tell
    // which tree it is scoring without a separate --verbose round-trip.
    source.merge_into(&mut meta);
    // iter-149: always present — a caller must be able to tell that ff-rdp
    // left the platform accessibility service enabled without opting into
    // --verbose stderr. The tree above is still the primary result even when
    // this reports a failure (a restore failure never masks a successful walk).
    restore_outcome.merge_into(&mut meta);
    // Legacy fields kept for existing consumers: only set for an *automatic*
    // fallback (the caller asked for the native tree and didn't get it), not
    // for `--selector`, which is always JS-derived by design.
    if let A11ySource::JsFallback(reason) = &source
        && *reason != "selector-mode"
        && let Some(m) = meta.as_object_mut()
    {
        m.insert("fallback".to_string(), json!(true));
        m.insert("fallback_method".to_string(), json!("js-eval"));
    }

    // When --limit / --all is set, flatten the tree into a list of nodes and
    // apply the limit.  Without a limit flag the output remains a single tree
    // object (the historical default behaviour).
    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let envelope = if cli.limit.is_some() || cli.all {
        // Pass the limit so flatten_tree can stop early instead of cloning all nodes.
        let early_stop = if controls.all { None } else { controls.limit };
        let mut flat = Vec::new();
        flatten_tree(&tree_value, &mut flat, early_stop);
        controls.apply_sort(&mut flat);
        let (limited, total, truncated) = controls.apply_limit(flat, None);
        let limited = controls.apply_fields(limited);
        let shown = limited.len();
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta)
    } else {
        output::envelope(&tree_value, 1, &meta)
    };

    // Text short-circuit: render an indented accessibility tree instead of JSON.
    // When --limit / --all is active we fall through to the pipeline which will
    // render the flat list as a table via the generic text renderer.
    if cli.format == "text" && cli.jq.is_none() && cli.limit.is_none() && !cli.all {
        render_a11y_text(&tree_value, 0);
        return Ok(());
    }

    let hint_ctx = HintContext::new(HintSource::A11y);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// Render an accessibility tree node (and its children) as an indented text tree.
///
/// Each node is printed as `<role> "<name>" [<value>] (<description>)` with
/// any optional fields omitted when absent.  Children are indented by two
/// spaces per depth level.
fn render_a11y_text(node: &Value, depth: usize) {
    use std::fmt::Write as _;
    let indent = "  ".repeat(depth);
    let role = node.get("role").and_then(Value::as_str).unwrap_or("?");
    let name = node.get("name").and_then(Value::as_str);
    let value = node.get("value").and_then(Value::as_str);
    let description = node.get("description").and_then(Value::as_str);

    let mut line = format!("{indent}{role}");
    if let Some(n) = name {
        let _ = write!(line, " \"{n}\"");
    }
    if let Some(v) = value {
        let _ = write!(line, " [{v}]");
    }
    if let Some(d) = description {
        let _ = write!(line, " ({d})");
    }
    println!("{line}");

    if let Some(truncated) = node.get("truncated").and_then(Value::as_str) {
        println!("{indent}  ... {truncated}");
    }

    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            render_a11y_text(child, depth + 1);
        }
    }
}

/// Attempt native RDP accessibility protocol, falling back to JS eval on
/// `unrecognizedPacketType` errors. Current Firefox exposes the walker's root
/// only through the argument-less `children`; `getDocument` was never a
/// published protocol method and `getRootNode` was removed long ago, so both
/// answer with `unrecognizedPacketType` (iter-136, see
/// `AccessibilityActor::get_root`).
fn run_native_or_js_fallback(
    ctx: &mut ConnectedTab,
    accessibility_actor: &ActorId,
    depth: u32,
    max_chars: u32,
    cli: &Cli,
) -> Result<(AccessibleNode, A11ySource), AppError> {
    // Step 0: the native walker only answers while the platform accessibility
    // service is running; with it off, the root accessor stalls until the
    // socket read timeout instead of erroring (iter-136). Check first and take
    // the JS fallback immediately when it is off.
    match AccessibilityActor::is_service_enabled(ctx.transport_mut(), accessibility_actor) {
        Ok(true) => {}
        Ok(false) => {
            if cli.is_verbose() {
                eprintln!(
                    "debug: platform accessibility service is disabled; falling back to JS eval \
                     (enable it in Firefox to get the native accessibility tree, or pass --native \
                     to opt in for this command)"
                );
            }
            return run_selector_mode(ctx, "body", depth, max_chars)
                .map(|t| (t, A11ySource::JsFallback("accessibility-service-disabled")));
        }
        // Older Firefox without `bootstrap` on the accessibility actor: try the
        // native path anyway.
        Err(e) if e.is_unrecognized_packet_type() => {}
        Err(e) => return Err(map_a11y_error(e, cli)),
    }

    // Step 1: try to get the walker. Bounded to A11Y_WALKER_TIMEOUT (Theme C)
    // so a race with the service being disabled after the check above stalls
    // for seconds, not the full configured --timeout.
    let walker = match with_walker_timeout(ctx, |t| {
        AccessibilityActor::get_walker(t, accessibility_actor)
    }) {
        Ok(w) => w,
        Err(e) if e.is_unrecognized_packet_type() => {
            if cli.is_verbose() {
                eprintln!(
                    "debug: accessibility getWalker unrecognized in this Firefox version; \
                     falling back to JS eval"
                );
            }
            return run_selector_mode(ctx, "body", depth, max_chars)
                .map(|t| (t, A11ySource::JsFallback("walker-unrecognized")));
        }
        Err(ProtocolError::Timeout) => {
            if cli.is_verbose() {
                eprintln!(
                    "debug: accessibility getWalker timed out after {}s (bounded deadline, \
                     iter-143); falling back to JS eval",
                    A11Y_WALKER_TIMEOUT.as_secs()
                );
            }
            return run_selector_mode(ctx, "body", depth, max_chars)
                .map(|t| (t, A11ySource::JsFallback("walker-timeout")));
        }
        Err(e) => return Err(map_a11y_error(e, cli)),
    };

    // Step 2: try to get the root node via the walker.
    let root = match with_walker_timeout(ctx, |t| AccessibilityActor::get_root(t, &walker)) {
        Ok(r) => r,
        Err(e) if e.is_unrecognized_packet_type() => {
            // Both getDocument and getRootNode failed — Firefox 149+ protocol change.
            if cli.is_verbose() {
                eprintln!(
                    "debug: accessibility walker root methods unrecognized in this Firefox \
                     version (tried getDocument and getRootNode); falling back to JS eval"
                );
            }
            return run_selector_mode(ctx, "body", depth, max_chars)
                .map(|t| (t, A11ySource::JsFallback("root-unrecognized")));
        }
        Err(ProtocolError::Timeout) => {
            if cli.is_verbose() {
                eprintln!(
                    "debug: accessibility walker root request timed out after {}s (bounded \
                     deadline, iter-143); falling back to JS eval",
                    A11Y_WALKER_TIMEOUT.as_secs()
                );
            }
            return run_selector_mode(ctx, "body", depth, max_chars)
                .map(|t| (t, A11ySource::JsFallback("root-timeout")));
        }
        Err(e) => return Err(map_a11y_error(e, cli)),
    };

    // Step 3: walk the tree with the native protocol.
    with_walker_timeout(ctx, |t| {
        AccessibilityActor::walk_tree(t, &walker, &root, depth, max_chars)
    })
    .map(|t| (t, A11ySource::Native))
    .map_err(|e| map_a11y_error(e, cli))
}

/// Opt-in native tree walk (iter-143 Theme B): enables the platform
/// accessibility service via the root actor's `parentAccessibilityActor` when
/// it is not already running, walks the native tree, then restores the
/// previous state afterward if — and only if — this call was the one that
/// turned it on (DEC-027: ff-rdp never leaves behind a browser-global
/// mutation the caller did not ask for, and never touches state it did not
/// create).
///
/// Unlike [`run_native_or_js_fallback`], this never falls back to the
/// JS-derived tree: a caller who passed `--native` asked for the platform
/// tree specifically, so any failure — `enable` failing, `bootstrap` still
/// reporting disabled after `enable`, or a stalled/erroring walker request —
/// surfaces as an explicit error instead of a silent substitution.
///
/// Returns the tree and the [`RestoreOutcome`] of the post-walk restore
/// attempt (iter-149) — surfaced so the caller can report a failed restore in
/// the envelope, and so tests and `--verbose` diagnostics can confirm
/// restoration.
fn run_native_opt_in(
    ctx: &mut ConnectedTab,
    accessibility_actor: &ActorId,
    depth: u32,
    max_chars: u32,
    cli: &Cli,
) -> Result<(AccessibleNode, RestoreOutcome), AppError> {
    let root_form = RootActor::get_root(ctx.transport_mut()).map_err(|e| map_a11y_error(e, cli))?;
    let parent_actor: ActorId = root_form
        .get("parentAccessibilityActor")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::User(
                "--native: this Firefox's root actor does not expose \
                 'parentAccessibilityActor' — cannot enable the platform accessibility \
                 service remotely. Omit --native to use the JS-derived fallback."
                    .to_string(),
            )
        })?
        .into();

    let was_enabled =
        AccessibilityActor::is_service_enabled(ctx.transport_mut(), accessibility_actor)
            .map_err(|e| map_a11y_error(e, cli))?;

    let we_enabled = if was_enabled {
        false
    } else {
        AccessibilityActor::enable_service(ctx.transport_mut(), &parent_actor).map_err(|e| {
            AppError::User(format!(
                "--native: failed to enable the platform accessibility service via \
                 parentAccessibilityActor.enable(): {e}"
            ))
        })?;
        let now_enabled =
            AccessibilityActor::is_service_enabled(ctx.transport_mut(), accessibility_actor)
                .map_err(|e| map_a11y_error(e, cli))?;
        if !now_enabled {
            return Err(AppError::User(
                "--native: called parentAccessibilityActor.enable() but bootstrap() still \
                 reports the accessibility service as disabled — refusing to walk the native \
                 tree rather than silently falling back. This may mean another consumer \
                 immediately disabled it, or this Firefox build doesn't honor a remote enable()."
                    .to_string(),
            ));
        }
        if cli.is_verbose() {
            eprintln!(
                "debug: --native: platform accessibility service was off; enabled it for this \
                 command and will restore it to disabled afterward"
            );
        }
        true
    };

    let walk_result = walk_native_tree_bounded(ctx, accessibility_actor, depth, max_chars, cli);

    // Best-effort restore: report a failure (iter-149: in both the envelope
    // and unconditionally on stderr) but don't let it mask the primary
    // result — the tree above is returned regardless of what happens here.
    // On Windows an active screen reader can block `disable`
    // (kb/rdp/actors/accessibility.md) — that's an expected platform
    // limitation, not a bug in ff-rdp, but an expected limitation still has
    // to be reported rather than silently leaving the service enabled.
    let restore_outcome = if we_enabled {
        let disable_target = force_restore_failure_target(&parent_actor);
        if let Err(e) = AccessibilityActor::disable_service(ctx.transport_mut(), &disable_target) {
            // Unconditional, not --verbose-gated: a human running this
            // interactively should not have to opt in to learning their
            // browser was left degraded (Theme C).
            eprintln!(
                "warning: --native: failed to restore the accessibility service to disabled \
                 after this opt-in run: {e}. Firefox's platform accessibility service stays \
                 enabled for as long as this connection remains open — normally that's just \
                 until this command exits, but a caller that reuses the connection will run \
                 every command slower until the service is disabled. See \
                 meta.service_restore_error in this command's output."
            );
            RestoreOutcome::Failed(e.to_string())
        } else {
            if cli.is_verbose() {
                eprintln!("debug: --native: restored the accessibility service to disabled");
            }
            RestoreOutcome::Restored
        }
    } else {
        RestoreOutcome::NotNeeded
    };

    walk_result.map(|tree| (tree, restore_outcome))
}

/// Test-only actor-boundary fault injection for iter-149's
/// `live_149_restore_failure_reported_in_meta` live test.
///
/// Forcing a real `disable()` failure against a live Firefox is otherwise
/// only reachable on Windows with an active screen reader blocking the call
/// (`kb/rdp/actors/accessibility.md`) — not reproducible in this project's
/// macOS/Linux live-test environment (`kb/iterations/iteration-149-a11y-restore-honesty.md`
/// Notes). When `FF_RDP_A11Y_FORCE_RESTORE_FAILURE=1` is set, the *restore*
/// call targets a deliberately-invalid actor ID instead of the real
/// `parentAccessibilityActor`, so Firefox genuinely answers with a
/// `noSuchActor`-style error over the wire — a real protocol failure, not a
/// mock. `enable_service` above is never affected by this: the service really
/// is turned on, and — because the disable call is the one being corrupted —
/// really is left on afterward. Never set in normal use; not documented in
/// `--help`.
fn force_restore_failure_target(real: &ActorId) -> ActorId {
    if std::env::var("FF_RDP_A11Y_FORCE_RESTORE_FAILURE").as_deref() == Ok("1") {
        ActorId::from(format!("{}-ff-rdp-149-force-failure", real.as_ref()))
    } else {
        real.clone()
    }
}

/// Walk the native accessibility tree (walker → root → recursive children),
/// with each step bounded by [`A11Y_WALKER_TIMEOUT`] (Theme C). Shared by
/// [`run_native_opt_in`]; unlike [`run_native_or_js_fallback`] there is no
/// fallback branch here — every error maps straight to an [`AppError`].
fn walk_native_tree_bounded(
    ctx: &mut ConnectedTab,
    accessibility_actor: &ActorId,
    depth: u32,
    max_chars: u32,
    cli: &Cli,
) -> Result<AccessibleNode, AppError> {
    let walker = with_walker_timeout(ctx, |t| {
        AccessibilityActor::get_walker(t, accessibility_actor)
    })
    .map_err(|e| map_a11y_error(e, cli))?;
    let root = with_walker_timeout(ctx, |t| AccessibilityActor::get_root(t, &walker))
        .map_err(|e| map_a11y_error(e, cli))?;
    with_walker_timeout(ctx, |t| {
        AccessibilityActor::walk_tree(t, &walker, &root, depth, max_chars)
    })
    .map_err(|e| map_a11y_error(e, cli))
}

/// Selector-based subtree extraction via JS eval.
///
/// Uses ARIA properties and computed roles available on DOM elements to build
/// an accessibility-like tree rooted at the matched element.
fn run_selector_mode(
    ctx: &mut ConnectedTab,
    selector: &str,
    depth: u32,
    max_chars: u32,
) -> Result<AccessibleNode, AppError> {
    let js = A11Y_SELECTOR_JS_TEMPLATE
        .replace(
            "__SELECTOR__",
            &super::js_helpers::escape_selector(selector),
        )
        .replace("__DEPTH__", &depth.to_string())
        .replace("__MAX_CHARS__", &max_chars.to_string());

    let console_actor = ctx.target.console_actor.clone();
    let eval_result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, &js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = eval_result.exception {
        let msg = exc
            .message
            .as_deref()
            .unwrap_or("a11y selector evaluation failed");
        return Err(AppError::User(format!("a11y --selector failed: {msg}")));
    }

    let result = resolve_result(ctx, &eval_result.result)?;

    if result.is_null() {
        return Err(AppError::User(format!(
            "no element matching selector '{selector}'"
        )));
    }

    parse_js_a11y_tree(&result).ok_or_else(|| {
        AppError::User("failed to parse accessibility tree from JS evaluation".to_string())
    })
}

fn parse_js_a11y_tree(value: &Value) -> Option<AccessibleNode> {
    let role = value.get("role")?.as_str()?.to_string();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(String::from)
        .filter(|s| !s.is_empty());
    let value_str = value
        .get("value")
        .and_then(Value::as_str)
        .map(String::from)
        .filter(|s| !s.is_empty());
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .map(String::from)
        .filter(|s| !s.is_empty());

    let children: Vec<AccessibleNode> = value
        .get("children")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(parse_js_a11y_tree).collect())
        .unwrap_or_default();

    let truncated = value
        .get("truncated")
        .and_then(Value::as_str)
        .map(String::from);

    Some(AccessibleNode {
        actor: None,
        role,
        name,
        value: value_str,
        description,
        child_count: None,
        states: vec![],
        dom_node_type: None,
        index_in_parent: None,
        children,
        truncated,
    })
}

/// Map protocol errors to user-friendly messages.
fn map_a11y_error(err: ff_rdp_core::ProtocolError, cli: &Cli) -> AppError {
    match &err {
        ProtocolError::Timeout => AppError::User(
            "accessibility request timed out waiting for a reply from Firefox. If this \
             happened while walking the tree, the platform accessibility service is likely \
             off — Firefox's walker never replies in that case (iter-136). Check \
             `a11y --jq '.meta.source'`, or omit --native to use the JS-derived fallback."
                .to_string(),
        ),
        ff_rdp_core::ProtocolError::ActorError { error, .. }
            if error == "noSuchActor" || error == "unknownActor" =>
        {
            let hint = if cli.no_daemon {
                " — the accessibility actor may have expired after navigation. Re-run the command"
            } else {
                " — the accessibility actor may have expired after navigation. Re-run the command to get a fresh actor"
            };
            AppError::User(format!("accessibility actor is no longer valid{hint}"))
        }
        ff_rdp_core::ProtocolError::ActorError { error, message, .. }
            if error == "unrecognizedPacketType" =>
        {
            AppError::User(format!(
                "accessibility: Firefox does not recognise the '{message}' method \
                 — this may indicate a protocol incompatibility with your Firefox version. \
                 If you are running Firefox 125+, try updating ff-rdp. \
                 As a workaround, use `a11y --selector <css>` which uses JS evaluation \
                 instead of the native RDP accessibility actor."
            ))
        }
        _ => AppError::from(err),
    }
}

/// Flatten a nested accessibility tree into a pre-order list of nodes.
///
/// Each node (a JSON object) is appended to `out` with its `children` field
/// removed so that each entry is self-contained.  Children are visited
/// recursively in document order (pre-order depth-first).
///
/// `max` is an optional early-stop limit: recursion halts once
/// `out.len() >= max`, avoiding unnecessary clones when `--limit N` is set.
fn flatten_tree(node: &Value, out: &mut Vec<Value>, max: Option<usize>) {
    if let Some(limit) = max
        && out.len() >= limit
    {
        return;
    }
    if let Value::Object(map) = node {
        // Clone without children for the flat entry.
        let mut entry = serde_json::Map::new();
        for (k, v) in map {
            if k != "children" {
                entry.insert(k.clone(), v.clone());
            }
        }
        out.push(Value::Object(entry));

        if let Some(Value::Array(children)) = map.get("children") {
            for child in children {
                flatten_tree(child, out, max);
                if let Some(limit) = max
                    && out.len() >= limit
                {
                    break;
                }
            }
        }
    }
}

/// Strip actor IDs from the output JSON (internal detail not useful to users).
fn strip_actor_ids(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("actor");
            for v in map.values_mut() {
                strip_actor_ids(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                strip_actor_ids(v);
            }
        }
        _ => {}
    }
}

/// `a11y --critical` (Theme E, iter-80): surface only nodes that fail a basic
/// WCAG audit. Runs a small JS audit in the page (the native accessibility
/// actor exposes contrast violations via `a11y contrast` but does not surface a
/// general "critical" severity at the tree level), then returns a flat array
/// of violation records suitable for piping into automation.
///
/// `root_selector` scopes the audit to a subtree when set; defaults to the
/// whole document.
pub fn run_critical(cli: &Cli, root_selector: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_direct(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let root = root_selector.unwrap_or(":root");
    let js = A11Y_CRITICAL_JS_TEMPLATE.replace("__SELECTOR__", &escape_selector(root));

    let eval_result = eval_or_bail(
        &mut ctx,
        &console_actor,
        &js,
        "a11y --critical query failed",
    )?;
    let parsed = resolve_result(&mut ctx, &eval_result.result)?;
    let violations: Vec<Value> = match parsed {
        Value::Array(arr) => arr,
        Value::Null => Vec::new(),
        other => {
            return Err(AppError::User(format!(
                "a11y --critical: unexpected result shape: {other}"
            )));
        }
    };

    let mut meta = json!({
        "root": root,
    });
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose — an
    // agent can tell how this command executed without a
    // separate `daemon status` round-trip.
    crate::connection_meta::merge_route(&mut meta, ctx.via_daemon);
    // iter-143 Theme A: `--critical` has no native-tree equivalent — the
    // platform accessibility service doesn't expose a WCAG-critical severity
    // — so this is always JS-derived. Reported for consistency with the
    // plain `a11y` tree's `meta.source` rather than as an actual fallback.
    crate::connection_meta::merge_source(&mut meta, "js-fallback", Some("critical-audit-js-only"));

    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let mut items = violations;
    controls.apply_sort(&mut items);
    let (limited, total, truncated) = controls.apply_limit(items, None);
    let limited = controls.apply_fields(limited);
    let shown = limited.len();
    let envelope =
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta);

    let hint_ctx = HintContext::new(HintSource::A11y);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// JS audit: returns a JSON array of violation records. Each record has
/// `{role, selector, violation, severity}` and optionally `name`. Severity is
/// always `"critical"` here — the helper is the WCAG-critical subset.
const A11Y_CRITICAL_JS_TEMPLATE: &str = r#"(function() {
  var root = document.querySelector('__SELECTOR__') || document.body || document.documentElement;
  if (!root) return '__FF_RDP_JSON__[]';
  var out = [];

  function selectorFor(el) {
    if (el.id) return '#' + el.id;
    var path = [];
    var n = el;
    while (n && n.nodeType === 1 && n !== root && path.length < 5) {
      var seg = n.tagName.toLowerCase();
      if (n.classList && n.classList.length) seg += '.' + n.classList[0];
      var parent = n.parentNode;
      if (parent) {
        var same = 0, idx = 0;
        for (var i = 0; i < parent.children.length; i++) {
          var sib = parent.children[i];
          if (sib.tagName === n.tagName) { same++; if (sib === n) idx = same; }
        }
        if (same > 1) seg += ':nth-of-type(' + idx + ')';
      }
      path.unshift(seg);
      n = n.parentElement;
    }
    return path.join(' > ');
  }

  function hasAccessibleName(el) {
    if (el.getAttribute && el.getAttribute('aria-label')) return true;
    var labelledBy = el.getAttribute && el.getAttribute('aria-labelledby');
    if (labelledBy) {
      var ids = labelledBy.split(/\s+/);
      for (var k = 0; k < ids.length; k++) {
        if (!ids[k]) continue;
        var label = document.getElementById(ids[k]);
        if (label && label.textContent && label.textContent.trim()) return true;
      }
    }
    if (el.labels && el.labels.length) {
      for (var i = 0; i < el.labels.length; i++) {
        if ((el.labels[i].textContent || '').trim()) return true;
      }
    }
    var text = (el.textContent || '').trim();
    if (text) return true;
    if (el.title) return true;
    return false;
  }

  // 1) <img> without alt
  var imgs = root.querySelectorAll('img');
  for (var i = 0; i < imgs.length; i++) {
    var img = imgs[i];
    if (!img.hasAttribute('alt')) {
      out.push({
        role: 'img',
        selector: selectorFor(img),
        violation: 'missing-alt',
        severity: 'critical'
      });
    }
  }

  // 2) <button>, role=button without accessible name
  var btns = root.querySelectorAll('button, [role="button"]');
  for (var j = 0; j < btns.length; j++) {
    var btn = btns[j];
    if (!hasAccessibleName(btn)) {
      out.push({
        role: 'button',
        selector: selectorFor(btn),
        violation: 'missing-name',
        severity: 'critical'
      });
    }
  }

  // 3) Form controls without an accessible name
  var ctrls = root.querySelectorAll('input:not([type="hidden"]):not([type="submit"]):not([type="button"]), select, textarea');
  for (var k = 0; k < ctrls.length; k++) {
    var c = ctrls[k];
    if (!hasAccessibleName(c) && !(c.getAttribute && c.getAttribute('placeholder'))) {
      out.push({
        role: c.tagName.toLowerCase(),
        selector: selectorFor(c),
        violation: 'missing-label',
        severity: 'critical'
      });
    }
  }

  // 4) Links without accessible name
  var as = root.querySelectorAll('a[href]');
  for (var m = 0; m < as.length; m++) {
    if (!hasAccessibleName(as[m])) {
      out.push({
        role: 'link',
        selector: selectorFor(as[m]),
        violation: 'missing-name',
        severity: 'critical'
      });
    }
  }

  return '__FF_RDP_JSON__' + JSON.stringify(out);
})()"#;

/// JS template for selector-based accessibility tree extraction.
///
/// Uses ARIA properties and computed roles available on DOM elements.
/// `__SELECTOR__`, `__DEPTH__`, and `__MAX_CHARS__` are replaced before evaluation.
const A11Y_SELECTOR_JS_TEMPLATE: &str = r#"(function() {
  var SKIP = {SCRIPT:1,STYLE:1,NOSCRIPT:1,SVG:1};
  var ROLE_MAP = {NAV:'navigation',HEADER:'banner',FOOTER:'contentinfo',MAIN:'main',
    ASIDE:'complementary',ARTICLE:'article',SECTION:'region',FORM:'form',
    DIALOG:'dialog',A:'link',BUTTON:'button',INPUT:'textbox',SELECT:'combobox',
    TEXTAREA:'textbox',H1:'heading',H2:'heading',H3:'heading',H4:'heading',
    H5:'heading',H6:'heading',IMG:'img',TABLE:'table',UL:'list',OL:'list',
    LI:'listitem',DETAILS:'group',SUMMARY:'button'};
  var maxDepth = __DEPTH__;
  var maxChars = __MAX_CHARS__;
  var totalChars = 0;

  function getRole(el) {
    var explicit = el.getAttribute && el.getAttribute('role');
    if (explicit) return explicit;
    if (el.computedRole && el.computedRole !== 'generic') return el.computedRole;
    return ROLE_MAP[el.tagName] || 'generic';
  }

  function getName(el) {
    if (el.ariaLabel) return el.ariaLabel;
    var labelledBy = el.getAttribute && el.getAttribute('aria-labelledby');
    if (labelledBy) {
      var label = document.getElementById(labelledBy);
      if (label) return label.textContent.trim();
    }
    if (el.tagName === 'IMG') return el.alt || '';
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') {
      if (el.labels && el.labels.length) return el.labels[0].textContent.trim();
      return el.placeholder || '';
    }
    if (!el.children || el.children.length === 0) {
      var t = el.textContent && el.textContent.trim();
      if (t && t.length <= 200) return t;
      if (t) return t.slice(0, 200) + '...';
    }
    return '';
  }

  function walk(node, depth) {
    if (node.nodeType === 3) {
      var t = node.textContent.trim();
      if (!t || totalChars >= maxChars) return null;
      totalChars += t.length;
      return {role: 'text', name: t.length > 200 ? t.slice(0,200)+'...' : t};
    }
    if (node.nodeType !== 1) return null;
    if (SKIP[node.tagName]) return null;

    try {
      var cs = window.getComputedStyle(node);
      if (cs.display === 'none' || cs.visibility === 'hidden') return null;
    } catch(e) {}
    if (node.getAttribute && node.getAttribute('aria-hidden') === 'true') return null;

    var role = getRole(node);
    var name = getName(node);
    var o = {role: role};
    if (name) o.name = name;

    var desc = node.getAttribute && node.getAttribute('aria-description');
    if (desc) o.description = desc;

    var val = node.value;
    if (val && (node.tagName === 'INPUT' || node.tagName === 'TEXTAREA' || node.tagName === 'SELECT')) {
      o.value = String(val);
    }

    if (depth >= maxDepth) {
      if (node.children.length > 0) o.truncated = node.children.length + ' children not shown';
      return o;
    }

    var children = [];
    var charCapped = false;
    for (var i = 0; i < node.childNodes.length; i++) {
      if (totalChars >= maxChars) { charCapped = true; break; }
      var c = walk(node.childNodes[i], depth + 1);
      if (c !== null && c.role !== 'generic') children.push(c);
      else if (c !== null && c.children) {
        for (var j = 0; j < c.children.length; j++) children.push(c.children[j]);
      }
    }
    if (children.length) o.children = children;
    if (charCapped) o.truncated = 'max characters reached';
    return o;
  }

  var root = document.querySelector("__SELECTOR__");
  if (!root) return '__FF_RDP_JSON__null';
  var tree = walk(root, 0);
  return '__FF_RDP_JSON__' + JSON.stringify(tree);
})()"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a11y_js_template_substitution() {
        let js = A11Y_SELECTOR_JS_TEMPLATE
            .replace("__SELECTOR__", "main")
            .replace("__DEPTH__", "4")
            .replace("__MAX_CHARS__", "20000");
        assert!(js.contains("var maxDepth = 4;"));
        assert!(js.contains("var maxChars = 20000;"));
        assert!(!js.contains("__DEPTH__"));
        assert!(!js.contains("__MAX_CHARS__"));
    }

    #[test]
    fn a11y_js_template_has_sentinel() {
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("__FF_RDP_JSON__"));
    }

    #[test]
    fn a11y_js_template_skips_hidden_elements() {
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("aria-hidden"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("display === 'none'"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("visibility === 'hidden'"));
    }

    #[test]
    fn a11y_js_template_has_role_map() {
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("ROLE_MAP"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("BUTTON"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("INPUT"));
        assert!(A11Y_SELECTOR_JS_TEMPLATE.contains("'link'"));
    }

    #[test]
    fn parse_js_a11y_tree_minimal() {
        let val = json!({"role": "button", "name": "Submit"});
        let node = parse_js_a11y_tree(&val).expect("should parse");
        assert_eq!(node.role, "button");
        assert_eq!(node.name.as_deref(), Some("Submit"));
        assert!(node.children.is_empty());
    }

    #[test]
    fn parse_js_a11y_tree_with_children() {
        let val = json!({
            "role": "list",
            "children": [
                {"role": "listitem", "name": "First"},
                {"role": "listitem", "name": "Second"},
            ]
        });
        let node = parse_js_a11y_tree(&val).expect("should parse");
        assert_eq!(node.role, "list");
        assert_eq!(node.children.len(), 2);
        assert_eq!(node.children[0].name.as_deref(), Some("First"));
    }

    #[test]
    fn parse_js_a11y_tree_empty_name_filtered() {
        let val = json!({"role": "generic", "name": "", "value": ""});
        let node = parse_js_a11y_tree(&val).expect("should parse");
        assert!(node.name.is_none());
        assert!(node.value.is_none());
    }

    #[test]
    fn parse_js_a11y_tree_missing_role_returns_none() {
        let val = json!({"name": "No role here"});
        assert!(parse_js_a11y_tree(&val).is_none());
    }

    #[test]
    fn flatten_tree_single_node() {
        let node = json!({"role": "button", "name": "OK"});
        let mut out = Vec::new();
        flatten_tree(&node, &mut out, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "button");
        assert_eq!(out[0]["name"], "OK");
    }

    #[test]
    fn flatten_tree_nested_children_preorder() {
        let node = json!({
            "role": "document",
            "children": [
                {
                    "role": "list",
                    "children": [
                        {"role": "listitem", "name": "A"},
                        {"role": "listitem", "name": "B"},
                    ]
                },
                {"role": "button", "name": "Submit"},
            ]
        });
        let mut out = Vec::new();
        flatten_tree(&node, &mut out, None);
        // Pre-order: document, list, listitem A, listitem B, button
        assert_eq!(out.len(), 5);
        assert_eq!(out[0]["role"], "document");
        assert_eq!(out[1]["role"], "list");
        assert_eq!(out[2]["role"], "listitem");
        assert_eq!(out[2]["name"], "A");
        assert_eq!(out[3]["role"], "listitem");
        assert_eq!(out[3]["name"], "B");
        assert_eq!(out[4]["role"], "button");
    }

    #[test]
    fn flatten_tree_removes_children_from_entries() {
        let node = json!({
            "role": "list",
            "children": [{"role": "listitem", "name": "X"}]
        });
        let mut out = Vec::new();
        flatten_tree(&node, &mut out, None);
        // The flat entry for "list" must not carry children.
        assert!(out[0].get("children").is_none());
    }

    #[test]
    fn flatten_tree_early_exit_with_max() {
        let node = json!({
            "role": "document",
            "children": [
                {"role": "heading", "name": "A"},
                {"role": "heading", "name": "B"},
                {"role": "heading", "name": "C"},
            ]
        });
        let mut out = Vec::new();
        // max=2: should stop after document + first heading
        flatten_tree(&node, &mut out, Some(2));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "document");
        assert_eq!(out[1]["role"], "heading");
        assert_eq!(out[1]["name"], "A");
    }

    #[test]
    fn flatten_tree_max_zero_produces_empty() {
        let node = json!({"role": "button", "name": "OK"});
        let mut out = Vec::new();
        flatten_tree(&node, &mut out, Some(0));
        assert!(out.is_empty());
    }

    #[test]
    fn strip_actor_ids_removes_actor_field() {
        let mut val = json!({
            "actor": "conn1/accessibility1",
            "role": "document",
            "children": [
                {"actor": "conn1/accessible2", "role": "button", "name": "OK"}
            ]
        });
        strip_actor_ids(&mut val);
        assert!(val.get("actor").is_none());
        assert!(val["children"][0].get("actor").is_none());
        assert_eq!(val["children"][0]["role"], "button");
    }

    // ── render_a11y_text ─────────────────────────────────────────────────────

    #[test]
    fn render_a11y_text_does_not_panic_with_minimal_node() {
        let node = json!({"role": "button", "name": "OK"});
        render_a11y_text(&node, 0);
    }

    #[test]
    fn render_a11y_text_does_not_panic_with_nested_tree() {
        let node = json!({
            "role": "document",
            "children": [
                {
                    "role": "list",
                    "children": [
                        {"role": "listitem", "name": "First"},
                        {"role": "listitem", "name": "Second", "value": "2", "description": "item two"},
                    ]
                },
                {"role": "button", "name": "Submit", "truncated": "3 children not shown"},
            ]
        });
        render_a11y_text(&node, 0);
    }

    #[test]
    fn render_a11y_text_does_not_panic_with_empty_object() {
        render_a11y_text(&json!({}), 0);
    }

    // ── RestoreOutcome::merge_into (iter-149) ───────────────────────────────

    #[test]
    fn unit_149_restore_outcome_maps_to_meta() {
        let mut not_needed = json!({});
        RestoreOutcome::NotNeeded.merge_into(&mut not_needed);
        assert_eq!(not_needed["service_left_enabled"], false);
        assert!(not_needed["service_restore_error"].is_null());

        let mut restored = json!({});
        RestoreOutcome::Restored.merge_into(&mut restored);
        assert_eq!(restored["service_left_enabled"], false);
        assert!(restored["service_restore_error"].is_null());

        let mut failed = json!({});
        RestoreOutcome::Failed("noSuchActor".to_string()).merge_into(&mut failed);
        assert_eq!(failed["service_left_enabled"], true);
        assert_eq!(failed["service_restore_error"], "noSuchActor");
    }

    #[test]
    fn restore_outcome_merge_into_overwrites_existing_keys() {
        // A restore-failure envelope must not accumulate a stale success
        // signal from an earlier call site — merge_into always replaces both
        // keys rather than only inserting when absent.
        let mut meta = json!({"service_left_enabled": false, "service_restore_error": null});
        RestoreOutcome::Failed("disable() timed out".to_string()).merge_into(&mut meta);
        assert_eq!(meta["service_left_enabled"], true);
        assert_eq!(meta["service_restore_error"], "disable() timed out");
    }

    #[test]
    fn force_restore_failure_target_defaults_to_real_actor() {
        // No env mutation here: this crate denies `unsafe_code`, and
        // `std::env::set_var`/`remove_var` require `unsafe` since the 2024
        // edition. The flag is not set by the test harness, so this simply
        // asserts the default (unset) behaviour.
        let real: ActorId = "conn0/accessibility1".into();
        assert_eq!(force_restore_failure_target(&real), real);
    }
}
