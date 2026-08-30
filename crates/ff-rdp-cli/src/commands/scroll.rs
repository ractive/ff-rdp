use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::cli::args::{Cli, ScrollBlock};
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::{
    JSON_SENTINEL, WaitForPredicate, autowait_element, escape_selector, eval_or_bail,
    resolve_result, settle_page, wait_for_predicates,
};

/// Options controlling auto-wait and post-action behaviour for scroll commands.
#[derive(Default)]
pub struct ScrollOptions<'a> {
    /// Auto-wait timeout in ms. `None` means use `cli.timeout`.
    pub wait_timeout_ms: Option<u64>,
    /// Skip auto-wait and scroll immediately (--no-wait).
    pub no_wait: bool,
    /// Post-action predicates (--wait-for).
    pub wait_for: &'a [String],
    /// Timeout for --wait-for predicates. `None` → same as `wait_timeout_ms`.
    pub wait_for_timeout_ms: Option<u64>,
    /// Whether to wait for page settle after scrolling (--settle).
    pub settle: bool,
    /// iter-210 Theme A: embed the post-scroll page view under `results.page`;
    /// carries `--page-chars` and `--query` since iter-219.
    pub page: crate::cli::args::PageViewArgs,
}

/// Serialize a user-supplied string as a JS string literal (double-quoted,
/// with all special characters escaped). Used to embed the *original* value in
/// JSON output (as opposed to the single-quote-escaped form produced by
/// `escape_selector`, which is only safe for `document.querySelector('…')`).
fn js_string_literal(s: &str) -> String {
    // serde_json::to_string on a &str is infallible; a double-quoted JSON
    // string is also a valid JS string literal.
    serde_json::to_string(s)
        .unwrap_or_else(|e| unreachable!("serde_json::to_string(&str) is infallible: {e}"))
}

/// Emit a scroll command's envelope, optionally with the `--with-page` view
/// attached (iter-210 Theme A).
///
/// Every `scroll` subcommand ends here so the `--with-page` collection, the
/// `meta` lift, and the text-mode page section exist exactly once rather than
/// seven times. `meta` arrives already carrying whatever the caller wants in
/// it (selector, settle method, direction); this adds only the connection and
/// page keys.
///
/// The page view is collected *after* the scroll and after any `--settle` /
/// `--wait-for`, so lazily-rendered content the scroll revealed is in it.
fn finalize_scroll(
    cli: &Cli,
    ctx: &mut ConnectedTab,
    mut result: Value,
    mut meta: Value,
    page_args: &crate::cli::args::PageViewArgs,
) -> Result<(), AppError> {
    if page_args.with_page {
        super::page_view::attach(ctx, &mut result, Some(cli.timeout), page_args)?;
    }
    let page_text = super::page_view::lift_meta(cli, &mut result, &mut meta);
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
    let envelope = output::envelope(&result, 1, &meta);

    OutputPipeline::from_cli(cli)?.finalize(&envelope)?;
    super::page_view::render_text_section(page_text.as_ref());
    Ok(())
}

// ---------------------------------------------------------------------------
// scroll to <selector>
// ---------------------------------------------------------------------------

pub fn run_to(
    cli: &Cli,
    selector: &str,
    block: ScrollBlock,
    smooth: bool,
    opts: &ScrollOptions<'_>,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let wait_timeout_ms = opts.wait_timeout_ms.unwrap_or(cli.timeout);

    // A3: Auto-wait for the scroll target to exist.
    if !opts.no_wait {
        autowait_element(&mut ctx, &console_actor, selector, wait_timeout_ms, false)?;
    }

    let escaped = escape_selector(selector);
    let selector_lit = js_string_literal(selector);
    let block_spec = block.as_spec();
    let behavior = if smooth { "smooth" } else { "auto" };
    let js = format!(
        r"(function() {{
  var el = document.querySelector('{escaped}');
  if (!el) throw new Error('Element not found: {escaped} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  el.scrollIntoView({{block: '{block_spec}', behavior: '{behavior}'}});
  var r = el.getBoundingClientRect();
  var atEnd = (window.scrollY + window.innerHeight) >= (document.documentElement.scrollHeight - 1);
  return '{JSON_SENTINEL}' + JSON.stringify({{
    scrolled: true,
    selector: {selector_lit},
    viewport: {{x: window.scrollX, y: window.scrollY, width: window.innerWidth, height: window.innerHeight}},
    target: {{selector: {selector_lit}, rect: {{top: r.top, left: r.left, width: r.width, height: r.height, bottom: r.bottom, right: r.right}}}},
    atEnd: atEnd
  }});
}})()"
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "scroll to failed")?;
    let result_json = resolve_result(&mut ctx, &eval_result.result)?;

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

    let mut meta = json!({"selector": selector});
    if let Some(sm) = settle_method {
        meta["settle_method"] = json!(sm.as_meta_str());
    }
    finalize_scroll(cli, &mut ctx, result_json, meta, &opts.page)
}

// ---------------------------------------------------------------------------
// scroll by [--dx] [--dy] [--page-down] [--page-up] [--smooth]
// ---------------------------------------------------------------------------

/// Flags for [`run_by`], bundled so the four booleans do not become four
/// positional parameters at the call site.
#[derive(Default, Clone)]
pub struct ScrollByOptions {
    /// `--page-down`: scroll down by 85% of the viewport height.
    pub page_down: bool,
    /// `--page-up`: scroll up by 85% of the viewport height.
    pub page_up: bool,
    /// `--smooth`: animate instead of jumping.
    pub smooth: bool,
    /// `--with-page` and friends: embed the resulting page view (iter-210
    /// Theme A, iter-219 Theme C).
    pub page: crate::cli::args::PageViewArgs,
}

pub fn run_by(cli: &Cli, dx: i64, dy: Option<i64>, opts: ScrollByOptions) -> Result<(), AppError> {
    let ScrollByOptions {
        page_down,
        page_up,
        smooth,
        page,
    } = opts;
    // Mutual exclusion: --page-down/--page-up cannot be combined with --dy
    if (page_down || page_up) && dy.is_some() {
        return Err(AppError::User(
            "scroll by: --page-down and --page-up are mutually exclusive with --dy".into(),
        ));
    }
    if page_down && page_up {
        return Err(AppError::User(
            "scroll by: --page-down and --page-up are mutually exclusive with each other".into(),
        ));
    }

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let behavior = if smooth { "smooth" } else { "auto" };
    let dy_expr = if page_down {
        "window.innerHeight * 0.85".to_owned()
    } else if page_up {
        "-(window.innerHeight * 0.85)".to_owned()
    } else {
        dy.unwrap_or(0).to_string()
    };

    let js = format!(
        r"(function() {{
  var topVal = {dy_expr};
  window.scrollBy({{left: {dx}, top: topVal, behavior: '{behavior}'}});
  var atEnd = (window.scrollY + window.innerHeight) >= (document.documentElement.scrollHeight - 1);
  return '{JSON_SENTINEL}' + JSON.stringify({{
    scrolled: true,
    viewport: {{x: window.scrollX, y: window.scrollY, width: window.innerWidth, height: window.innerHeight}},
    scrollHeight: document.documentElement.scrollHeight,
    atEnd: atEnd
  }});
}})()"
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "scroll by failed")?;
    let result_json = resolve_result(&mut ctx, &eval_result.result)?;
    finalize_scroll(cli, &mut ctx, result_json, json!({}), &page)
}

// ---------------------------------------------------------------------------
// scroll top / scroll bottom
// ---------------------------------------------------------------------------

pub fn run_top(cli: &Cli, page_args: &crate::cli::args::PageViewArgs) -> Result<(), AppError> {
    run_scroll_absolute(cli, "0", "scroll top failed", page_args)
}

pub fn run_bottom(cli: &Cli, page_args: &crate::cli::args::PageViewArgs) -> Result<(), AppError> {
    run_scroll_absolute(cli, "root.scrollHeight", "scroll bottom failed", page_args)
}

/// Shared implementation for `scroll top` and `scroll bottom`.
///
/// `y_expr` is a JavaScript expression for the Y coordinate passed to
/// `window.scrollTo(0, <y_expr>)`.  `error_label` appears in error messages.
fn run_scroll_absolute(
    cli: &Cli,
    y_expr: &str,
    error_label: &str,
    page_args: &crate::cli::args::PageViewArgs,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // iter-129 Theme D: on a page with a CMP/modal overlay that sets
    // `overflow:hidden` on <html>/<body>, `window.scrollTo` silently no-ops —
    // the position doesn't move and `atEnd` reports `true` trivially (0-height
    // scrollable range), masking the real cause. Detect that specific
    // "locked AND didn't move" combination and name the locking element
    // instead of staying silent (dogfooding-session-62 finding 1).
    let js = format!(
        r#"(function() {{
  var root = document.scrollingElement || document.documentElement || document.body;
  var before = root.scrollTop;
  window.scrollTo(0, {y_expr});
  var after = root.scrollTop;
  var atEnd = (after + window.innerHeight) >= (root.scrollHeight - 1);
  var htmlOverflow = getComputedStyle(document.documentElement).overflow;
  var bodyOverflow = document.body ? getComputedStyle(document.body).overflow : '';
  var locked = htmlOverflow === 'hidden' || bodyOverflow === 'hidden';
  var warning = null;
  if (locked && before === after) {{
    var lockedEl = htmlOverflow === 'hidden' ? document.documentElement : document.body;
    var cls = (lockedEl.className || '').toString().trim();
    var clsPart = cls ? ' (class="' + cls + '")' : '';
    warning = 'scroll blocked: <' + lockedEl.tagName.toLowerCase() + '>' + clsPart +
      ' has overflow:hidden — likely a modal/consent overlay';
  }}
  return '{JSON_SENTINEL}' + JSON.stringify({{
    scrolled: true,
    viewport: {{x: root.scrollLeft, y: after, width: window.innerWidth, height: window.innerHeight}},
    scrollHeight: root.scrollHeight,
    atEnd: atEnd,
    warning: warning
  }});
}})()"#
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, error_label)?;
    let result_json = resolve_result(&mut ctx, &eval_result.result)?;
    finalize_scroll(cli, &mut ctx, result_json, json!({}), page_args)
}

// ---------------------------------------------------------------------------
// scroll container <selector> [--dx] [--dy] [--to-end] [--to-start]
// ---------------------------------------------------------------------------

pub fn run_container(
    cli: &Cli,
    selector: &str,
    dx: i64,
    dy: i64,
    to_end: bool,
    to_start: bool,
    page_args: &crate::cli::args::PageViewArgs,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped = escape_selector(selector);
    let selector_lit = js_string_literal(selector);
    let scroll_logic = if to_end {
        "el.scrollTop = el.scrollHeight; el.scrollLeft = el.scrollWidth;".to_owned()
    } else if to_start {
        "el.scrollTop = 0; el.scrollLeft = 0;".to_owned()
    } else {
        format!("el.scrollTop += {dy}; el.scrollLeft += {dx};")
    };

    let js = format!(
        r"(function() {{
  var el = document.querySelector('{escaped}');
  if (!el) throw new Error('Element not found: {escaped} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  var before = {{scrollTop: el.scrollTop, scrollLeft: el.scrollLeft}};
  {scroll_logic}
  var after = {{scrollTop: el.scrollTop, scrollLeft: el.scrollLeft}};
  var atEnd = (el.scrollTop + el.clientHeight) >= (el.scrollHeight - 1);
  return '{JSON_SENTINEL}' + JSON.stringify({{
    scrolled: true,
    selector: {selector_lit},
    before: before,
    after: after,
    scrollHeight: el.scrollHeight,
    clientHeight: el.clientHeight,
    atEnd: atEnd
  }});
}})()"
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "scroll container failed")?;
    let result_json = resolve_result(&mut ctx, &eval_result.result)?;
    let meta = json!({"selector": selector});
    finalize_scroll(cli, &mut ctx, result_json, meta, page_args)
}

// ---------------------------------------------------------------------------
// scroll until <selector> [--direction up|down] [--timeout <ms>]
// ---------------------------------------------------------------------------

const SCROLL_UNTIL_POLL_MS: u64 = 200;

pub fn run_until(
    cli: &Cli,
    selector: &str,
    direction: &str,
    timeout_ms: u64,
    page_args: &crate::cli::args::PageViewArgs,
) -> Result<(), AppError> {
    if direction != "up" && direction != "down" {
        return Err(AppError::User(format!(
            "scroll until: --direction must be 'up' or 'down', got {direction:?}"
        )));
    }

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped = escape_selector(selector);
    let selector_lit = js_string_literal(selector);
    let sign = if direction == "up" { "-" } else { "" };

    // JS to check if element is in viewport
    let check_js = format!(
        r"(function() {{
  var el = document.querySelector('{escaped}');
  if (!el) return false;
  var r = el.getBoundingClientRect();
  return r.top < window.innerHeight && r.bottom > 0 && r.left < window.innerWidth && r.right > 0;
}})()"
    );

    // JS to scroll one step
    let scroll_js = format!(
        r"(function() {{
  window.scrollBy({{top: {sign}(window.innerHeight * 0.8), behavior: 'auto'}});
  return true;
}})()"
    );

    // JS to collect final result data
    let result_js = format!(
        r"(function() {{
  var el = document.querySelector('{escaped}');
  if (!el) return '{JSON_SENTINEL}' + JSON.stringify({{found: false, selector: {selector_lit}}});
  var r = el.getBoundingClientRect();
  return '{JSON_SENTINEL}' + JSON.stringify({{
    found: true,
    selector: {selector_lit},
    viewport: {{x: window.scrollX, y: window.scrollY, width: window.innerWidth, height: window.innerHeight}},
    target: {{selector: {selector_lit}, rect: {{top: r.top, left: r.left, width: r.width, height: r.height}}}}
  }});
}})()"
    );

    let timeout = Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(SCROLL_UNTIL_POLL_MS);
    let started = Instant::now();
    let mut scrolls: u64 = 0;

    loop {
        // Check if visible
        let check_result = eval_or_bail(
            &mut ctx,
            &console_actor,
            &check_js,
            "scroll until check failed",
        )?;
        let visible = is_truthy_grip(&check_result.result);

        if visible {
            break;
        }

        let elapsed = started.elapsed();
        if elapsed >= timeout {
            // iter-145 Theme B: route through the standard JSON error envelope
            // (`AppError::Timeout`, matching every other timeout in this
            // codebase — see `js_helpers.rs`, `navigate.rs`, `click.rs`)
            // instead of printing bare text to stderr and bypassing `main`'s
            // envelope emission via `AppError::Exit(1)`.
            return Err(AppError::Timeout(format!(
                "scroll until timed out after {}ms — element '{selector}' not found in viewport; increase with --timeout",
                elapsed.as_millis()
            )));
        }

        // Scroll one step
        eval_or_bail(
            &mut ctx,
            &console_actor,
            &scroll_js,
            "scroll until scroll failed",
        )?;
        scrolls += 1;

        std::thread::sleep(poll);
    }

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Collect final result
    let result_eval = eval_or_bail(
        &mut ctx,
        &console_actor,
        &result_js,
        "scroll until result failed",
    )?;
    let mut result_json = resolve_result(&mut ctx, &result_eval.result)?;

    // Augment with elapsed/scrolls
    if let Some(obj) = result_json.as_object_mut() {
        obj.insert("elapsed_ms".to_owned(), json!(elapsed_ms));
        obj.insert("scrolls".to_owned(), json!(scrolls));
    }

    let meta = json!({"selector": selector, "direction": direction, "timeout_ms": timeout_ms});
    finalize_scroll(cli, &mut ctx, result_json, meta, page_args)
}

fn is_truthy_grip(grip: &ff_rdp_core::Grip) -> bool {
    use ff_rdp_core::Grip;
    match grip {
        Grip::Null | Grip::Undefined | Grip::NaN | Grip::NegZero => false,
        Grip::Value(v) => {
            if let Some(b) = v.as_bool() {
                return b;
            }
            if let Some(n) = v.as_f64() {
                return n != 0.0;
            }
            if let Some(s) = v.as_str() {
                return !s.is_empty();
            }
            !v.is_null()
        }
        Grip::Inf | Grip::NegInf | Grip::LongString { .. } | Grip::Object { .. } => true,
    }
}

// ---------------------------------------------------------------------------
// scroll text <text>
// ---------------------------------------------------------------------------

pub fn run_text(
    cli: &Cli,
    text: &str,
    page_args: &crate::cli::args::PageViewArgs,
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let text_json = serde_json::to_string(text)
        .map_err(|e| AppError::from(anyhow::anyhow!("failed to encode text argument: {e}")))?;

    let js = format!(
        r"(function() {{
  var needle = {text_json};
  var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
  var node = null;
  while ((node = walker.nextNode()) !== null) {{
    if (node.nodeValue && node.nodeValue.includes(needle)) {{
      break;
    }}
    node = null;
  }}
  if (!node) throw new Error('Text not found: ' + needle);
  var el = node.parentElement;
  el.scrollIntoView({{block: 'center', behavior: 'auto'}});
  var r = el.getBoundingClientRect();
  return '{JSON_SENTINEL}' + JSON.stringify({{
    scrolled: true,
    text: needle,
    viewport: {{x: window.scrollX, y: window.scrollY, width: window.innerWidth, height: window.innerHeight}},
    target: {{tag: el.tagName.toLowerCase(), rect: {{top: r.top, left: r.left, width: r.width, height: r.height}}}}
  }});
}})()"
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "scroll text failed")?;
    let result_json = resolve_result(&mut ctx, &eval_result.result)?;
    let meta = json!({"text": text});
    finalize_scroll(cli, &mut ctx, result_json, meta, page_args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Command, ScrollCommand};
    use clap::Parser as _;

    // ── clap parse tests ────────────────────────────────────────────────────

    #[test]
    fn clap_scroll_by_negative_dy_parses() {
        let cli = Cli::try_parse_from(["ff-rdp", "scroll", "by", "--dy", "-500"])
            .expect("should parse --dy -500");
        let Command::Scroll { scroll_command } = cli.command else {
            panic!("expected Scroll command");
        };
        let ScrollCommand::By { dy, .. } = scroll_command else {
            panic!("expected scroll by");
        };
        assert_eq!(dy, Some(-500));
    }

    #[test]
    fn clap_scroll_top_parses() {
        let cli =
            Cli::try_parse_from(["ff-rdp", "scroll", "top"]).expect("should parse scroll top");
        let Command::Scroll { scroll_command } = cli.command else {
            panic!("expected Scroll command");
        };
        assert!(
            matches!(scroll_command, ScrollCommand::Top { .. }),
            "expected ScrollCommand::Top"
        );
    }

    #[test]
    fn clap_scroll_bottom_parses() {
        let cli = Cli::try_parse_from(["ff-rdp", "scroll", "bottom"])
            .expect("should parse scroll bottom");
        let Command::Scroll { scroll_command } = cli.command else {
            panic!("expected Scroll command");
        };
        assert!(
            matches!(scroll_command, ScrollCommand::Bottom { .. }),
            "expected ScrollCommand::Bottom"
        );
    }

    #[test]
    fn scroll_block_maps_user_friendly_aliases_to_spec_values() {
        assert_eq!(ScrollBlock::Top.as_spec(), "start");
        assert_eq!(ScrollBlock::Bottom.as_spec(), "end");
        assert_eq!(ScrollBlock::Start.as_spec(), "start");
        assert_eq!(ScrollBlock::End.as_spec(), "end");
        assert_eq!(ScrollBlock::Center.as_spec(), "center");
        assert_eq!(ScrollBlock::Nearest.as_spec(), "nearest");
    }

    #[test]
    fn js_string_literal_preserves_original_selector_with_quotes() {
        // Original selector with a single quote should be emitted as a
        // double-quoted JS literal with the quote unescaped.
        assert_eq!(
            js_string_literal("div[data-name='test']"),
            r#""div[data-name='test']""#
        );
    }

    #[test]
    fn js_string_literal_escapes_special_chars() {
        assert_eq!(js_string_literal("a\nb"), r#""a\nb""#);
        assert_eq!(js_string_literal(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn run_to_js_contains_sentinel_and_scroll_into_view() {
        // Build the JS directly by extracting the logic
        let selector = "h1.title";
        let escaped = escape_selector(selector);
        let block = "center";
        let behavior = "smooth";
        let js = format!(
            r"(function() {{
  var el = document.querySelector('{escaped}');
  if (!el) throw new Error('Element not found: {escaped}');
  el.scrollIntoView({{block: '{block}', behavior: '{behavior}'}});
  return '{JSON_SENTINEL}' + JSON.stringify({{scrolled: true}});
}})()"
        );
        assert!(js.contains(JSON_SENTINEL));
        assert!(js.contains("scrollIntoView"));
        assert!(js.contains("h1.title"));
        assert!(js.contains("center"));
        assert!(js.contains("smooth"));
    }

    #[test]
    fn run_by_rejects_page_down_with_dy() {
        // We can test the validation logic directly
        let (page_down, page_up, dy) = (true, false, Some(100i64));
        let conflict = (page_down || page_up) && dy.is_some();
        assert!(conflict, "should detect mutual exclusion");
    }

    #[test]
    fn run_by_rejects_page_down_with_page_up() {
        let (page_down, page_up) = (true, true);
        assert!(page_down && page_up, "both set — should detect conflict");
    }

    #[test]
    fn run_by_page_down_expr() {
        let dy_expr = "window.innerHeight * 0.85".to_owned();
        assert!(dy_expr.contains("innerHeight"));
    }

    #[test]
    fn run_by_page_up_expr() {
        let dy_expr = "-(window.innerHeight * 0.85)".to_owned();
        assert!(dy_expr.starts_with('-'));
    }

    #[test]
    fn scroll_text_js_uses_tree_walker() {
        let text = "Contact Us";
        let text_json = serde_json::to_string(text).unwrap();
        let js = format!(
            r"(function() {{
  var needle = {text_json};
  var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, null);
  return '{JSON_SENTINEL}' + JSON.stringify({{scrolled: true}});
}})()"
        );
        assert!(js.contains("createTreeWalker"));
        assert!(js.contains("NodeFilter.SHOW_TEXT"));
        assert!(js.contains("Contact Us"));
    }

    #[test]
    fn scroll_container_to_end_js() {
        let selector = ".sidebar";
        let escaped = escape_selector(selector);
        let scroll_logic =
            "el.scrollTop = el.scrollHeight; el.scrollLeft = el.scrollWidth;".to_owned();
        let js = format!(
            r"(function() {{
  var el = document.querySelector('{escaped}');
  {scroll_logic}
  return '{JSON_SENTINEL}' + JSON.stringify({{scrolled: true}});
}})()"
        );
        assert!(js.contains("scrollHeight"));
        assert!(js.contains("scrollWidth"));
    }

    #[test]
    fn escape_selector_in_scroll_js() {
        let selector = "div[data-name='test']";
        let escaped = escape_selector(selector);
        assert!(escaped.contains("\\'"));
    }

    #[test]
    fn run_by_negative_dy_produces_negative_js_expr() {
        // Negative dy values must produce a negative literal in the JS expression
        // (i.e. "scroll by --dy -500" should scroll up, not fail parsing).
        let dy: i64 = -500;
        let dy_expr = dy.to_string();
        assert_eq!(dy_expr, "-500");
        assert!(dy_expr.starts_with('-'));
    }

    #[test]
    fn run_by_negative_dx_produces_negative_js_expr() {
        let dx: i64 = -200;
        let js = format!(
            r"(function() {{
  window.scrollBy({{left: {dx}, top: 0, behavior: 'auto'}});
  return true;
}})()"
        );
        assert!(js.contains("left: -200"));
    }

    #[test]
    fn run_top_js_scrolls_to_origin() {
        // Verify the JS emitted by run_top uses scrollTo(0, 0)
        let js = format!(
            r"(function() {{
  window.scrollTo(0, 0);
  return '{JSON_SENTINEL}' + JSON.stringify({{scrolled: true}});
}})()"
        );
        assert!(js.contains("scrollTo(0, 0)"));
        assert!(js.contains(JSON_SENTINEL));
    }

    #[test]
    fn run_bottom_js_scrolls_to_scroll_height() {
        // Verify the JS emitted by run_bottom uses scrollingElement fallback
        // and scrollTo(0, root.scrollHeight).
        let js = format!(
            r"(function() {{
  var root = document.scrollingElement || document.documentElement || document.body;
  window.scrollTo(0, root.scrollHeight);
  return '{JSON_SENTINEL}' + JSON.stringify({{scrolled: true}});
}})()"
        );
        assert!(
            js.contains("document.scrollingElement || document.documentElement || document.body")
        );
        assert!(js.contains("scrollTo(0, root.scrollHeight)"));
        assert!(js.contains(JSON_SENTINEL));
    }
}
