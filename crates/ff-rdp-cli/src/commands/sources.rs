use ff_rdp_core::ThreadActor;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
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

pub fn run(cli: &Cli, filter: Option<&str>, pattern: Option<&str>) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    let thread_actor = ctx
        .target
        .thread_actor
        .clone()
        .ok_or_else(|| AppError::User("target does not expose a thread actor".into()))?;

    // Attempt native thread-actor source listing; fall back to JS eval on errors
    // that indicate Firefox 149+ protocol changes (unrecognized method or
    // `undefined passed where a value is required`).
    let sources = match ThreadActor::list_sources(ctx.transport_mut(), thread_actor.as_ref()) {
        Ok(s) => s
            .into_iter()
            .map(|s| {
                json!({
                    "url": s.url,
                    "actor": s.actor,
                    "isBlackBoxed": s.is_black_boxed,
                })
            })
            .collect::<Vec<_>>(),
        Err(e) if should_use_js_fallback(&e) => {
            eprintln!(
                "debug: sources thread actor failed ({e}); \
                 falling back to JS DOM/Performance API"
            );
            list_sources_via_js(&mut ctx)?
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

    let total = results.len();
    let result_json = json!(results);
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result_json, total, &meta);

    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
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
