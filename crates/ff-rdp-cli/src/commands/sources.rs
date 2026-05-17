use ff_rdp_core::{DomWalkerActor, InspectorActor, ThreadActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_direct;
use super::js_helpers::eval_or_bail;

/// JavaScript fallback for listing script sources via the DOM and Performance API.
///
/// Used when the thread actor's `sources` method is unavailable (Firefox 149+
/// returns `undefined passed where a value is required`).
///
/// Collects script URLs from:
/// 1. `document.querySelectorAll('script[src]')` — external scripts
/// 2. `performance.getEntriesByType('resource')` filtered to script resources
///
/// Returns a JSON-sentinel-prefixed array of `{url, isBlackBoxed}` objects.
const SOURCES_JS: &str = r#"(function() {
  var seen = Object.create(null);
  var results = [];

  function addUrl(url) {
    if (!url || seen[url]) return;
    seen[url] = true;
    results.push({url: url, actor: '', isBlackBoxed: false});
  }

  // 1. Explicit <script src="..."> tags in the document.
  var scripts = document.querySelectorAll('script[src]');
  for (var i = 0; i < scripts.length; i++) {
    addUrl(scripts[i].src);
  }

  // 2. Resources from the Performance API (catches dynamically injected scripts).
  if (window.performance && performance.getEntriesByType) {
    var entries = performance.getEntriesByType('resource');
    for (var j = 0; j < entries.length; j++) {
      var e = entries[j];
      if (e.initiatorType === 'script') addUrl(e.name);
    }
  }

  return '__FF_RDP_JSON__' + JSON.stringify(results);
})()"#;

/// Which fallback method was used to obtain the source list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackMethod {
    /// Native thread-actor `sources` method (best, no fallback needed).
    SourcesActor,
    /// JS eval via `evaluateJSAsync` + DOM/Performance API.
    JsEval,
    /// WalkerActor `querySelectorAll("script")` — used when eval is CSP-blocked.
    WalkerActor,
}

impl FallbackMethod {
    fn as_meta_str(self) -> Option<&'static str> {
        match self {
            Self::SourcesActor => None, // No fallback annotation when native path worked.
            Self::JsEval => Some("js-eval"),
            Self::WalkerActor => Some("walker-actor"),
        }
    }
}

pub fn run(cli: &Cli, filter: Option<&str>, pattern: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_direct(cli)?;

    let thread_actor = ctx
        .target
        .thread_actor
        .clone()
        .ok_or_else(|| AppError::User("target does not expose a thread actor".into()))?;

    // Attempt native thread-actor source listing; fall back to JS eval on errors
    // that indicate Firefox 149+ protocol changes (unrecognized method or
    // `undefined passed where a value is required`).
    let (sources, fallback_method) =
        match ThreadActor::list_sources(ctx.transport_mut(), thread_actor.as_ref()) {
            Ok(s) => {
                let entries = s
                    .into_iter()
                    .map(|s| {
                        json!({
                            "url": s.url,
                            "actor": s.actor,
                            "isBlackBoxed": s.is_black_boxed,
                        })
                    })
                    .collect::<Vec<_>>();
                (entries, FallbackMethod::SourcesActor)
            }
            Err(e) if should_use_js_fallback(&e) => {
                if cli.is_verbose() {
                    eprintln!(
                        "debug: sources thread actor failed ({e}); \
                         trying JS DOM/Performance API fallback"
                    );
                }
                // C2: Probe whether the page CSP allows eval before invoking
                // the JS fallback.  A blocked eval throws an EvalError whose
                // message typically contains "Content Security Policy" or the
                // exception class is "EvalError".  We check for that and skip
                // directly to the walker fallback if eval is blocked.
                let eval_allowed = probe_eval_allowed(&mut ctx);
                if cli.is_verbose() && !eval_allowed {
                    eprintln!("debug: eval probe blocked by CSP — using walker-actor fallback");
                }
                if eval_allowed {
                    match list_sources_via_js(&mut ctx) {
                        Ok(entries) => (entries, FallbackMethod::JsEval),
                        Err(_) => {
                            // JS eval succeeded (not CSP-blocked) but returned an error.
                            // Fall through to walker.
                            (
                                list_sources_via_walker(&mut ctx)?,
                                FallbackMethod::WalkerActor,
                            )
                        }
                    }
                } else {
                    // CSP blocks eval — use walker directly.
                    (
                        list_sources_via_walker(&mut ctx)?,
                        FallbackMethod::WalkerActor,
                    )
                }
            }
            Err(e) => return Err(AppError::from(e)),
        };

    // Apply filters.
    let regex = pattern
        .map(|p| {
            regex::RegexBuilder::new(p)
                .size_limit(1_000_000)
                .build()
                .map_err(|e| AppError::User(format!("invalid --pattern regex: {e}")))
        })
        .transpose()?;

    let results: Vec<Value> = sources
        .into_iter()
        .filter(|s| {
            let url = s.get("url").and_then(Value::as_str).unwrap_or("");
            if let Some(f) = filter
                && !url.contains(f)
            {
                return false;
            }
            if let Some(ref re) = regex
                && !re.is_match(url)
            {
                return false;
            }
            true
        })
        .collect();

    // Apply --sort, --limit / --all, --fields output controls (no default limit for sources).
    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let mut results = results;
    controls.apply_sort(&mut results);
    let (limited, total, truncated) = controls.apply_limit(results, None);
    let limited = controls.apply_fields(limited);
    let shown = limited.len();
    let result_json = json!(limited);
    let mut meta = json!({});
    if let Some(method_str) = fallback_method.as_meta_str()
        && let Some(m) = meta.as_object_mut()
    {
        m.insert("fallback".to_string(), json!(true));
        m.insert("fallback_method".to_string(), json!(method_str));
    }
    let envelope = output::envelope_with_truncation(&result_json, shown, total, truncated, &meta);

    let hint_ctx = HintContext::new(HintSource::Sources);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Probe whether the page CSP allows `eval()` by attempting a no-op eval.
///
/// Returns `true` when eval is permitted (or when the probe itself fails for
/// non-CSP reasons — we err on the side of allowing the JS fallback and let
/// the actual eval surface the real error if there is one).
fn probe_eval_allowed(ctx: &mut super::connect_tab::ConnectedTab) -> bool {
    use ff_rdp_core::WebConsoleActor;

    let console_actor = ctx.target.console_actor.clone();
    // A no-op expression that `eval()` would accept.  We're not calling `eval`
    // directly here; `evaluateJSAsync` goes through the Firefox devtools
    // protocol and is normally unrestricted by CSP.  However some Firefox
    // versions apply the page's `script-src` CSP to debugger expressions; we
    // detect that by catching the EvalError / CSP exception class.
    let probe_js = "1+1";
    match WebConsoleActor::evaluate_js_async(ctx.transport_mut(), &console_actor, probe_js) {
        Err(_) => true, // Protocol error — don't assume CSP; let the caller decide.
        Ok(result) => {
            if let Some(ref exc) = result.exception {
                // CSP-blocked evals throw an EvalError; the message typically
                // contains "Content Security Policy", "unsafe-eval", or
                // "EvalError".  All indicate that eval-style execution is blocked.
                let msg = exc.message.as_deref().unwrap_or("");
                let is_csp = msg.contains("Content Security Policy")
                    || msg.contains("unsafe-eval")
                    || msg.contains("EvalError");
                !is_csp
            } else {
                true // No exception — eval is allowed.
            }
        }
    }
}

/// List script sources by walking `document.scripts` via the WalkerActor.
///
/// This is the CSP-safe fallback that does not use `eval()`.  It uses the
/// Firefox devtools WalkerActor protocol to list all `<script>` nodes in the
/// document and extract their `src` attributes (for external scripts) or
/// synthesise an `inline://document/<index>` URL (for inline scripts).
///
/// Returns entries in the same shape as the native thread-actor path:
/// `{url, actor: "", isBlackBoxed: false}`.
fn list_sources_via_walker(
    ctx: &mut super::connect_tab::ConnectedTab,
) -> Result<Vec<Value>, AppError> {
    use ff_rdp_core::ActorId;

    let inspector_actor = ctx
        .target
        .inspector_actor
        .clone()
        .ok_or_else(|| AppError::User("target does not expose an inspector actor".into()))?;

    // Get the DOM walker from the inspector.
    let walker_actor = InspectorActor::get_walker(ctx.transport_mut(), &inspector_actor)
        .map_err(AppError::from)?;

    // Get the document element to use as the root for querySelectorAll.
    let doc_root = DomWalkerActor::document_element(ctx.transport_mut(), &walker_actor)
        .map_err(AppError::from)?;

    let root_actor = doc_root
        .actor
        .as_deref()
        .ok_or_else(|| AppError::from(anyhow::anyhow!("document element has no actor ID")))?;
    let root_actor_id = ActorId::from(root_actor);

    // querySelectorAll("script") to get all <script> nodes.
    let script_nodes = DomWalkerActor::query_selector_all(
        ctx.transport_mut(),
        &walker_actor,
        &root_actor_id,
        "script",
    )
    .map_err(AppError::from)?;

    let mut entries = Vec::new();
    for (index, node) in script_nodes.iter().enumerate() {
        // Look for a `src` attribute.
        let src = node
            .attrs
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case("src"))
            .map(|a| a.value.as_str());

        let url = if let Some(s) = src
            && !s.is_empty()
        {
            s.to_owned()
        } else {
            // Inline script — synthesise a URL.
            format!("inline://document/{index}")
        };

        entries.push(json!({
            "url": url,
            "actor": "",
            "isBlackBoxed": false,
        }));
    }

    Ok(entries)
}

/// Returns `true` for errors that should trigger the JS DOM fallback.
///
/// Matches `unrecognizedPacketType` (method renamed/removed) and
/// `undefined passed where a value is required` (Firefox 149+ bug where the
/// thread actor's `sources` method returns undefined internally).
fn should_use_js_fallback(err: &ff_rdp_core::ProtocolError) -> bool {
    if err.is_unrecognized_packet_type() {
        return true;
    }
    if let ff_rdp_core::ProtocolError::ActorError { message, .. } = err
        && (message.contains("undefined") || message.contains("not available"))
    {
        return true;
    }
    false
}

/// Gather script source URLs via JS DOM + Performance API eval.
///
/// Returns a `Vec<Value>` where each entry has `url`, `actor` (empty), and
/// `isBlackBoxed` (false) fields, matching the native thread-actor format.
fn list_sources_via_js(ctx: &mut super::connect_tab::ConnectedTab) -> Result<Vec<Value>, AppError> {
    use super::js_helpers::resolve_result;

    let console_actor = ctx.target.console_actor.clone();
    let eval_result = eval_or_bail(ctx, &console_actor, SOURCES_JS, "sources JS eval failed")?;

    let parsed = resolve_result(ctx, &eval_result.result)?;

    match parsed {
        Value::Array(arr) => Ok(arr),
        _ => Err(AppError::from(anyhow::anyhow!(
            "sources JS fallback returned non-array"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that a normal pattern compiles successfully under the size limit.
    #[test]
    fn accepts_reasonable_regex() {
        let result = regex::RegexBuilder::new(r"\.js$|\.ts$")
            .size_limit(1_000_000)
            .build();
        assert!(result.is_ok());
    }

    /// Verify that a pattern exceeding a small compiled-regex size limit is rejected.
    #[test]
    fn rejects_oversized_regex() {
        let oversized = (0..100)
            .map(|i| format!("literal_{i}"))
            .collect::<Vec<_>>()
            .join("|");
        let result = regex::RegexBuilder::new(&oversized).size_limit(64).build();
        assert!(result.is_err(), "expected oversized pattern to be rejected");
    }

    #[test]
    fn should_use_js_fallback_unrecognized_packet_type() {
        let err = ff_rdp_core::ProtocolError::ActorError {
            actor: "conn0/thread1".to_owned(),
            kind: ff_rdp_core::ActorErrorKind::UnrecognizedPacketType,
            error: "unrecognizedPacketType".to_owned(),
            message: "sources".to_owned(),
        };
        assert!(should_use_js_fallback(&err));
    }

    #[test]
    fn should_use_js_fallback_undefined_message() {
        let err = ff_rdp_core::ProtocolError::ActorError {
            actor: "conn0/thread1".to_owned(),
            kind: ff_rdp_core::ActorErrorKind::Other("serverError".to_owned()),
            error: "serverError".to_owned(),
            message: "undefined passed where a value is required".to_owned(),
        };
        assert!(should_use_js_fallback(&err));
    }

    #[test]
    fn should_use_js_fallback_not_available_message() {
        let err = ff_rdp_core::ProtocolError::ActorError {
            actor: "conn0/thread1".to_owned(),
            kind: ff_rdp_core::ActorErrorKind::Other("serverError".to_owned()),
            error: "serverError".to_owned(),
            message: "sources not available".to_owned(),
        };
        assert!(should_use_js_fallback(&err));
    }

    #[test]
    fn should_use_js_fallback_false_for_network_error() {
        assert!(!should_use_js_fallback(
            &ff_rdp_core::ProtocolError::Timeout
        ));
    }

    #[test]
    fn should_use_js_fallback_false_for_unrelated_actor_error() {
        let err = ff_rdp_core::ProtocolError::ActorError {
            actor: "conn0/thread1".to_owned(),
            kind: ff_rdp_core::ActorErrorKind::WrongState,
            error: "wrongState".to_owned(),
            message: "thread is already attached".to_owned(),
        };
        assert!(!should_use_js_fallback(&err));
    }

    #[test]
    fn sources_js_has_sentinel() {
        assert!(SOURCES_JS.contains("__FF_RDP_JSON__"));
    }

    #[test]
    fn sources_js_collects_script_tags() {
        assert!(SOURCES_JS.contains("querySelectorAll"));
        assert!(SOURCES_JS.contains("script[src]"));
    }

    #[test]
    fn sources_js_uses_performance_api() {
        assert!(SOURCES_JS.contains("performance.getEntriesByType"));
        assert!(SOURCES_JS.contains("initiatorType"));
    }
}
