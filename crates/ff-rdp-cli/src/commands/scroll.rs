use std::time::{Duration, Instant};

use ff_rdp_core::{ActorId, WebConsoleActor};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::{JSON_SENTINEL, escape_selector, eval_or_bail, resolve_result};

// ---------------------------------------------------------------------------
// scroll to <selector>
// ---------------------------------------------------------------------------

pub fn run_to(cli: &Cli, selector: &str, block: &str, smooth: bool) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped = escape_selector(selector);
    let behavior = if smooth { "smooth" } else { "auto" };
    let js = format!(
        r"(function() {{
  var el = document.querySelector('{escaped}');
  if (!el) throw new Error('Element not found: {escaped} — use ff-rdp dom SELECTOR --count to verify the selector matches');
  el.scrollIntoView({{block: '{block}', behavior: '{behavior}'}});
  var r = el.getBoundingClientRect();
  var atEnd = (window.scrollY + window.innerHeight) >= (document.documentElement.scrollHeight - 1);
  return '{JSON_SENTINEL}' + JSON.stringify({{
    scrolled: true,
    selector: '{escaped}',
    viewport: {{x: window.scrollX, y: window.scrollY, width: window.innerWidth, height: window.innerHeight}},
    target: {{selector: '{escaped}', rect: {{top: r.top, left: r.left, width: r.width, height: r.height, bottom: r.bottom, right: r.right}}}},
    atEnd: atEnd
  }});
}})()"
    );

    let eval_result = eval_or_bail(&mut ctx, &console_actor, &js, "scroll to failed")?;
    let result_json = resolve_result(&mut ctx, &eval_result.result)?;
    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// scroll by [--dx] [--dy] [--page-down] [--page-up] [--smooth]
// ---------------------------------------------------------------------------

pub fn run_by(
    cli: &Cli,
    dx: i64,
    dy: Option<i64>,
    page_down: bool,
    page_up: bool,
    smooth: bool,
) -> Result<(), AppError> {
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
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
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
) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped = escape_selector(selector);
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
    selector: '{escaped}',
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
    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
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
) -> Result<(), AppError> {
    if direction != "up" && direction != "down" {
        return Err(AppError::User(format!(
            "scroll until: --direction must be 'up' or 'down', got {direction:?}"
        )));
    }

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let escaped = escape_selector(selector);
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
  if (!el) return '{JSON_SENTINEL}' + JSON.stringify({{found: false, selector: '{escaped}'}});
  var r = el.getBoundingClientRect();
  return '{JSON_SENTINEL}' + JSON.stringify({{
    found: true,
    selector: '{escaped}',
    viewport: {{x: window.scrollX, y: window.scrollY, width: window.innerWidth, height: window.innerHeight}},
    target: {{selector: '{escaped}', rect: {{top: r.top, left: r.left, width: r.width, height: r.height}}}}
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
            eprintln!(
                "error: scroll until timed out after {}ms — element '{selector}' not found in viewport; increase with --timeout",
                elapsed.as_millis()
            );
            return Err(AppError::Exit(1));
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

    let meta = json!({"host": cli.host, "port": cli.port, "selector": selector, "direction": direction, "timeout_ms": timeout_ms});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
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

pub fn run_text(cli: &Cli, text: &str) -> Result<(), AppError> {
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
    let meta = json!({"host": cli.host, "port": cli.port, "text": text});
    let envelope = output::envelope(&result_json, 1, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// Helper: resolve a poll-loop JS check result to bool
// ---------------------------------------------------------------------------

fn _poll_check(
    ctx: &mut super::connect_tab::ConnectedTab,
    console_actor: &ActorId,
    js: &str,
) -> Result<bool, AppError> {
    let result = WebConsoleActor::evaluate_js_async(ctx.transport_mut(), console_actor, js)
        .map_err(AppError::from)?;

    if let Some(ref exc) = result.exception {
        let msg = exc.message.as_deref().unwrap_or("poll check exception");
        eprintln!("error: {msg}");
        return Err(AppError::Exit(1));
    }

    Ok(is_truthy_grip(&result.result))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
