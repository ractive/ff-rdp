use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::page_view::{self, CollectOptions, DEFAULT_INTERACTIVE_LIMIT};

pub fn run(cli: &Cli) -> Result<(), AppError> {
    // iter-210 Theme B: `connect_direct` until now, which made the daemon's
    // ref store unreachable from here — so the first thing an agent looks at
    // after navigating carried no `--ref` handles and it had to guess a
    // selector for `dom` before it could click anything. Nothing about this
    // command's protocol traffic conflicts with the proxy (see the
    // daemon-routing table in `dispatch.rs`), so it takes the normal route
    // now and registers refs exactly as `dom` does.
    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    // `--all` lifts the cap entirely; an explicit `--limit` overrides the
    // default. Resolved here rather than after collection so refs are minted
    // for exactly the entries the caller receives.
    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    let interactive_limit = if controls.all {
        None
    } else {
        Some(controls.limit.unwrap_or(DEFAULT_INTERACTIVE_LIMIT))
    };

    let page = page_view::collect(
        &mut ctx,
        &console_actor,
        &CollectOptions {
            interactive_limit,
            wait_complete_ms: None,
        },
    )?;

    let output_results = page.view;

    let mut meta = json!({});
    if ctx.via_daemon && let Some(obj) = meta.as_object_mut() {
        // Same contract as `dom`'s `meta.refs_registered` (iter-61j D1):
        // always emitted on the daemon route so a caller can check whether
        // the `ref` handles in the output are usable before relying on them.
        obj.insert("refs_registered".to_owned(), json!(page.refs_registered));
    }
    obj_insert_source(&mut meta, page.source.as_meta_str());
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
    let envelope = output::envelope(&output_results, 1, &meta);

    // Custom text rendering for a11y summary.
    if cli.format == "text" && cli.jq.is_none() {
        page_view::render_text(&output_results);
        return Ok(());
    }

    let hint_ctx = HintContext::new(HintSource::A11ySummary);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// Record how the view was produced under `meta.source`, matching `a11y`'s
/// own key so the two accessibility commands read the same way.
fn obj_insert_source(meta: &mut serde_json::Value, source: &str) {
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("source".to_owned(), json!(source));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The `--all` / `--limit` resolution this command feeds into the shared
    /// collector. Extracted as a table test because it is the only decision
    /// `run` makes that does not need a live Firefox.
    fn resolve_limit(all: bool, limit: Option<usize>) -> Option<usize> {
        if all {
            None
        } else {
            Some(limit.unwrap_or(DEFAULT_INTERACTIVE_LIMIT))
        }
    }

    #[test]
    fn all_flag_lifts_the_interactive_cap() {
        assert_eq!(resolve_limit(true, None), None);
        assert_eq!(resolve_limit(true, Some(5)), None);
    }

    #[test]
    fn explicit_limit_wins_over_the_default() {
        assert_eq!(resolve_limit(false, Some(5)), Some(5));
        assert_eq!(resolve_limit(false, None), Some(DEFAULT_INTERACTIVE_LIMIT));
    }

    #[test]
    fn source_lands_in_meta() {
        let mut meta = json!({});
        obj_insert_source(&mut meta, "js-fallback");
        assert_eq!(meta.get("source"), Some(&Value::String("js-fallback".into())));
    }
}
