use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::ConnectedTab;
use super::connect_tab::connect_and_get_target;
use super::js_helpers::{
    JSON_SENTINEL, MatchPolicy, WaitForPredicate, autowait_element, escape_selector, eval_or_bail,
    poll_js_condition, resolve_disambiguated_target, resolve_result, settle_page,
    wait_for_predicates,
};

/// In-page helper that maps a character to a plausible `KeyboardEvent.code`
/// (iter-160 Theme C).
///
/// `code` is the physical-key identifier, so it is US-QWERTY-shaped by
/// definition; letters and digits cover what a page's `keydown` handler
/// realistically switches on. Anything else gets `''`, which is the value
/// `KeyboardEvent` uses when no physical key is known — a fabricated `code`
/// would be a worse answer than an honest empty one.
const KEY_CODE_JS_FN: &str = r"  function __ffrdpKeyCode(ch) {
    if (ch >= 'a' && ch <= 'z') { return 'Key' + ch.toUpperCase(); }
    if (ch >= 'A' && ch <= 'Z') { return 'Key' + ch; }
    if (ch >= '0' && ch <= '9') { return 'Digit' + ch; }
    if (ch === ' ') { return 'Space'; }
    if (ch === '\n') { return 'Enter'; }
    return '';
  }";

/// Build the in-page JS that `type` evaluates.
///
/// `escaped_text_json` is the text already encoded as a JSON string literal
/// (quotes included) by the caller.
///
/// iter-160 Theme C: the value used to be assigned in one shot with only
/// `input` and `change` dispatched afterwards, so a combobox that opens on
/// `keydown`, a search box that debounces `keyup`, or a form that validates on
/// `keypress` saw nothing at all while the command reported `{"typed": true}`.
/// Each character now gets `keydown` → `keypress` (printable only) → `keyup`,
/// with the value applied incrementally between `keypress` and `keyup`.
///
/// The ceiling is real and reported as `synthetic: true`: these events carry
/// `isTrusted: false`, and a `preventDefault()` on a synthetic `keydown`
/// cannot suppress an assignment ff-rdp makes directly. See `type`'s long help.
fn build_type_js(escaped_sel: &str, escaped_text_json: &str, clear: bool) -> String {
    let clear_flag_js = if clear { "true" } else { "false" };
    format!(
        r#"(function() {{
  "use strict";
  var el = document.querySelector('{escaped_sel}');
  if (!el) throw new Error('Element not found: {escaped_sel} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  var setter = null;
  if (window.HTMLInputElement && el instanceof window.HTMLInputElement) {{
    setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
  }} else if (window.HTMLTextAreaElement && el instanceof window.HTMLTextAreaElement) {{
    setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value').set;
  }} else if (window.HTMLSelectElement && el instanceof window.HTMLSelectElement) {{
    setter = Object.getOwnPropertyDescriptor(window.HTMLSelectElement.prototype, 'value').set;
  }}
  function applyValue(v) {{
    if (setter) {{ setter.call(el, v); }} else {{ el.value = v; }}
  }}
{KEY_CODE_JS_FN}
  if ({clear_flag_js}) {{ applyValue(''); }}
  var text = {escaped_text_json};
  for (var i = 0; i < text.length; i++) {{
    var ch = text.charAt(i);
    var init = {{key: ch, code: __ffrdpKeyCode(ch), bubbles: true, cancelable: true}};
    el.dispatchEvent(new KeyboardEvent('keydown', init));
    if (ch >= ' ') {{ el.dispatchEvent(new KeyboardEvent('keypress', init)); }}
    // Apply the prefix before `keyup` so a handler that reads el.value sees
    // what a real typist's keyup would have left there.
    applyValue(text.substring(0, i + 1));
    el.dispatchEvent(new KeyboardEvent('keyup', init));
  }}
  applyValue(text);
  el.dispatchEvent(new Event('input', {{bubbles: true}}));
  el.dispatchEvent(new Event('change', {{bubbles: true}}));
  return '{JSON_SENTINEL}' + JSON.stringify({{typed: true, synthetic: true, tag: el.tagName, value: el.value}});
}})()"#
    )
}

// ---------------------------------------------------------------------------
// `--submit` (iter-210 Theme C)
// ---------------------------------------------------------------------------

/// How long to watch for a navigation caused by the synthetic Enter before
/// falling back to `form.requestSubmit()`.
///
/// Short on purpose: this is not "how long can a page take to load", it is
/// "did pressing Enter start anything at all". A real submission changes
/// `location.href` (or begins a load) within a couple of hundred milliseconds;
/// waiting longer just delays the fallback on every page whose Enter handler
/// does nothing, which is the common case that made `--submit` necessary.
const ENTER_NAVIGATION_GRACE_MS: u64 = 600;

/// JS that presses Enter on the element and reports what it found there.
///
/// It deliberately does NOT submit the form: whether the fallback is needed
/// depends on whether this Enter navigated, which no synchronous script can
/// observe. Rust decides, after watching for a navigation.
fn build_enter_js(escaped_sel: &str) -> String {
    format!(
        r#"(function() {{
  "use strict";
  var el = document.querySelector('{escaped_sel}');
  if (!el) throw new Error('Element not found: {escaped_sel} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  var before = window.location.href;
  var init = {{key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true}};
  var notPrevented = el.dispatchEvent(new KeyboardEvent('keydown', init));
  el.dispatchEvent(new KeyboardEvent('keypress', init));
  el.dispatchEvent(new KeyboardEvent('keyup', init));
  var form = el.form || (el.closest ? el.closest('form') : null);
  return '{JSON_SENTINEL}' + JSON.stringify({{
    url_before: before,
    default_prevented: !notPrevented,
    has_form: !!form
  }});
}})()"#
    )
}

/// JS that submits the element's owning form for real.
///
/// `requestSubmit()` is the correct call — unlike `form.submit()` it fires the
/// `submit` event and runs constraint validation, so a page's own handler and
/// its `required` fields behave as they would for a human. `form.submit()` is
/// the fallback for the (older) case where `requestSubmit` is absent.
fn build_request_submit_js(escaped_sel: &str) -> String {
    format!(
        r#"(function() {{
  "use strict";
  var el = document.querySelector('{escaped_sel}');
  if (!el) throw new Error('Element not found: {escaped_sel}');
  var form = el.form || (el.closest ? el.closest('form') : null);
  if (!form) return '{JSON_SENTINEL}' + JSON.stringify({{requested: false, reason: 'no_form'}});
  if (typeof form.requestSubmit === 'function') {{
    form.requestSubmit();
  }} else {{
    form.submit();
  }}
  return '{JSON_SENTINEL}' + JSON.stringify({{requested: true, reason: null}});
}})()"#
    )
}

/// Press Enter on `selector` and, when that did not navigate, submit its form.
///
/// Returns `{submitted, navigated, method}` for `results`.
///
/// Why two steps rather than one: the synthetic `keydown` is `isTrusted:
/// false` (see `type --help`), and Firefox does not perform its own implicit
/// form submission for untrusted Enter — so on most forms the key press alone
/// does nothing at all. But on the pages that *do* handle Enter in script
/// (every search box with a JS handler), calling `requestSubmit()` as well
/// would submit twice. Watching for a navigation in between is what tells the
/// two apart.
fn press_enter_and_submit(
    ctx: &mut ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    escaped_sel: &str,
) -> Result<serde_json::Value, AppError> {
    let enter = eval_or_bail(
        ctx,
        console_actor,
        &build_enter_js(escaped_sel),
        "submit failed",
    )?;
    let enter_json = resolve_result(ctx, &enter.result)?;
    let url_before = enter_json
        .get("url_before")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let has_form = enter_json
        .get("has_form")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let default_prevented = enter_json
        .get("default_prevented")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    if navigated_away(ctx, console_actor, &url_before, ENTER_NAVIGATION_GRACE_MS) {
        return Ok(json!({"submitted": true, "navigated": true, "method": "enter"}));
    }

    // Enter did nothing observable. Fall back to the form, when there is one
    // and no handler asked us not to.
    if !has_form || default_prevented {
        return Ok(json!({
            "submitted": false,
            "navigated": false,
            "method": if has_form { "enter_prevented" } else { "no_form" },
        }));
    }

    let req = eval_or_bail(
        ctx,
        console_actor,
        &build_request_submit_js(escaped_sel),
        "submit failed",
    )?;
    let req_json = resolve_result(ctx, &req.result)?;
    let requested = req_json
        .get("requested")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !requested {
        return Ok(json!({"submitted": false, "navigated": false, "method": "no_form"}));
    }
    let navigated = navigated_away(ctx, console_actor, &url_before, ENTER_NAVIGATION_GRACE_MS);
    Ok(json!({
        "submitted": true,
        "navigated": navigated,
        "method": "request_submit",
    }))
}

/// Poll for `window.location.href` moving away from `url_before`.
///
/// A timeout means "no navigation observed", which is a legitimate answer
/// (a same-page form handler, an XHR submit) — not an error.
fn navigated_away(
    ctx: &mut ConnectedTab,
    console_actor: &ff_rdp_core::ActorId,
    url_before: &str,
    timeout_ms: u64,
) -> bool {
    let before_lit = serde_json::to_string(url_before).unwrap_or_else(|_| "\"\"".to_owned());
    let js = format!("window.location.href !== {before_lit}");
    poll_js_condition(
        ctx,
        console_actor,
        &js,
        timeout_ms,
        "navigation probe threw",
        "no navigation observed",
    )
    .is_ok()
}

/// Options controlling auto-wait and post-action behaviour for `type`.
#[derive(Default)]
pub struct TypeOptions<'a> {
    /// Auto-wait timeout in ms. `None` means use `cli.timeout`.
    pub wait_timeout_ms: Option<u64>,
    /// Skip auto-wait and type immediately (--no-wait).
    pub no_wait: bool,
    /// Post-action predicates (--wait-for).
    pub wait_for: &'a [String],
    /// Timeout for --wait-for predicates. `None` → same as `wait_timeout_ms`.
    pub wait_for_timeout_ms: Option<u64>,
    /// Whether to wait for page settle after typing (--settle).
    pub settle: bool,
    /// iter-140 Theme C: `--visible` / `--index N` — disambiguate a selector
    /// that matches more than one element before doing anything else. `None`
    /// (the default, flag-less path) is unchanged.
    pub match_policy: Option<MatchPolicy>,
    /// iter-210 Theme C: `--submit` — press Enter on the element and, if that
    /// did not navigate and the element is inside a `<form>`, call
    /// `form.requestSubmit()`.
    pub submit: bool,
    /// iter-210 Theme A: `--with-page` — embed the resulting page under
    /// `results.page`.
    pub with_page: bool,
}

/// Type text into a DOM element and return the result value without printing.
///
/// Called by the script runner, which handles its own NDJSON output.
pub fn run_core(
    cli: &Cli,
    selector: &str,
    text: &str,
    clear: bool,
    opts: &TypeOptions<'_>,
) -> Result<(serde_json::Value, bool), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let wait_timeout_ms = opts.wait_timeout_ms.unwrap_or(cli.timeout);

    // iter-140 Theme C: resolve `--visible`/`--index` to a single,
    // genuinely-unique element selector up front — see the matching comment
    // in click.rs's run_core for why this must happen before auto-wait.
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

    // A2: Auto-wait for the element to be focusable (also calls .focus()).
    if !opts.no_wait {
        autowait_element(&mut ctx, &console_actor, selector, wait_timeout_ms, true)?;
    }

    let escaped_sel = escape_selector(selector);
    let escaped_text_json = serde_json::to_string(text)
        .map_err(|e| AppError::from(anyhow::anyhow!("failed to encode text argument: {e}")))?;
    let js = build_type_js(&escaped_sel, &escaped_text_json, clear);

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "type failed")?;

    let mut result_json = resolve_result(&mut ctx, &eval_result.result)?;

    // iter-210 Theme C: --submit. Runs before --settle/--wait-for so those
    // observe the page the submission produced.
    if opts.submit {
        let submit_json = press_enter_and_submit(&mut ctx, &console_actor, &escaped_sel)?;
        if let (Some(dst), Some(src)) = (result_json.as_object_mut(), submit_json.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
    }

    // C2: --settle.
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

    let mut result = result_json;
    if let Some(sm) = settle_method {
        result["settle_method"] = json!(sm.as_meta_str());
    }
    // iter-140 Theme B/C: report disambiguation transparency on success too.
    if let Some((match_count, chosen_index)) = disambiguation {
        result["match_count"] = json!(match_count);
        result["chosen_index"] = json!(chosen_index);
    }

    // iter-210 Theme A: `--with-page`, collected after `--submit` so a form
    // that navigated reports the page it landed on.
    if opts.with_page {
        super::page_view::attach(&mut ctx, &mut result, Some(wait_timeout_ms))?;
    }

    Ok((result, ctx.via_daemon))
}

pub fn run(
    cli: &Cli,
    selector: &str,
    text: &str,
    clear: bool,
    opts: &TypeOptions<'_>,
) -> Result<(), AppError> {
    let (mut result_json, via_daemon) = run_core(cli, selector, text, clear, opts)?;

    // Preserve the pre-iter-61c CLI output shape: `settle_method` belongs in
    // `meta`, not in `results`.  The script runner reads it from `results`.
    let settle_method = result_json
        .as_object_mut()
        .and_then(|o| o.remove("settle_method"));
    let mut meta = json!({"selector": selector});
    if let Some(sm) = settle_method {
        meta["settle_method"] = sm;
    }
    let page_text = super::page_view::lift_meta(cli, &mut result_json, &mut meta);
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    // iter-134: always present, not gated by --verbose.
    crate::connection_meta::merge_route(&mut meta, via_daemon);
    let envelope = output::envelope(&result_json, 1, &meta);

    let hint_ctx = HintContext::new(HintSource::TypeText).with_selector(selector);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))?;
    super::page_view::render_text_section(page_text.as_ref());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC `unit_160_type_help_states_synthetic_ceiling` (JS half): the result
    /// the command reports carries the `synthetic` qualifier, so a caller who
    /// only reads `results` still learns the events were untrusted.
    #[test]
    fn unit_160_type_js_reports_synthetic_true() {
        let js = build_type_js("input", "\"hi\"", false);
        assert!(js.contains("synthetic: true"), "no synthetic flag: {js}");
    }

    /// AC `unit_160_type_help_states_synthetic_ceiling` (help half). Read from
    /// clap's own long help so the paragraph can't drift out of `--help` while
    /// still living in a comment somewhere.
    #[test]
    fn unit_160_type_help_states_synthetic_ceiling() {
        use clap::CommandFactory as _;
        let cmd = crate::cli::args::Cli::command();
        let type_cmd = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "type")
            .expect("`type` subcommand must exist");
        let help = type_cmd.clone().render_long_help().to_string();
        assert!(
            help.contains("isTrusted: false"),
            "type --help must state the isTrusted ceiling; got:\n{help}"
        );
        assert!(
            help.contains("preventDefault"),
            "type --help must say preventDefault cannot suppress the character; got:\n{help}"
        );
    }

    /// Theme C: every character produces the three-event sequence, and the
    /// value is applied incrementally rather than in one shot at the end.
    #[test]
    fn unit_160_type_js_dispatches_key_sequence_per_character() {
        let js = build_type_js("input", "\"hi\"", false);
        for event in ["keydown", "keypress", "keyup"] {
            assert!(js.contains(event), "missing {event}: {js}");
        }
        assert!(js.contains("KeyboardEvent"), "no KeyboardEvent: {js}");
        assert!(
            js.contains("text.substring(0, i + 1)"),
            "value must be applied incrementally: {js}"
        );
        // input/change still fire once, after the loop.
        assert_eq!(js.matches("new Event('input'").count(), 1, "{js}");
        assert_eq!(js.matches("new Event('change'").count(), 1, "{js}");
    }

    #[test]
    fn unit_160_type_js_clear_applies_empty_value_first() {
        let cleared = build_type_js("input", "\"hi\"", true);
        assert!(
            cleared.contains("if (true) { applyValue(''); }"),
            "{cleared}"
        );
        let kept = build_type_js("input", "\"hi\"", false);
        assert!(kept.contains("if (false) { applyValue(''); }"), "{kept}");
    }
}
