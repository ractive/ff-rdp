use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::{OutputControls, QueryFilter, SortDir};
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::page_view::{self, CollectOptions, DEFAULT_INTERACTIVE_LIMIT};

pub fn run(cli: &Cli, query: &QueryFilter) -> Result<(), AppError> {
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

    // iter-211 Theme A: with `--query` the cap must be applied AFTER the
    // filter, not before — the whole point is to find a control that may sit
    // past the 50th interactive element, and capping first would hide exactly
    // the entry the caller asked for. Collect uncapped, filter, then cap.
    // Refs are registered for the uncapped set in that case; the extras are
    // registered-but-unreferenced entries in the daemon, which cost nothing
    // and expire with the next navigation (the same trade `snapshot` makes).
    let collect_limit = if query.is_active() {
        None
    } else {
        interactive_limit
    };

    let page = page_view::collect(
        &mut ctx,
        &console_actor,
        &CollectOptions {
            interactive_limit: collect_limit,
            wait_complete_ms: None,
        },
    )?;

    let mut output_results = page.view;
    let query_matches = if query.is_active() {
        let matches = filter_page_view(&mut output_results, query);
        page_view::apply_interactive_limit(&mut output_results, interactive_limit);
        Some(matches)
    } else {
        None
    };
    let output_results = output_results;

    let mut meta = json!({});
    if ctx.via_daemon
        && let Some(obj) = meta.as_object_mut()
    {
        // Same contract as `dom`'s `meta.refs_registered` (iter-61j D1):
        // always emitted on the daemon route so a caller can check whether
        // the `ref` handles in the output are usable before relying on them.
        obj.insert("refs_registered".to_owned(), json!(page.refs_registered));
    }
    if let (Some(matches), Some(obj)) = (query_matches, meta.as_object_mut()) {
        obj.insert("matches".to_owned(), json!(matches));
    }
    obj_insert_source(&mut meta, page.source);
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

/// Filter `headings`, `landmarks` and `interactive` down to the entries
/// matching `query`, returning the total number of survivors (iter-211
/// Theme A).
///
/// Each section is judged on its own human-readable field — `text` for
/// headings, `label` for landmarks, `name` for interactive entries — plus
/// `href`, so "find the link to /pricing" works as well as "find the link
/// called Pricing". Entries are kept whole, `ref` included, so a survivor is
/// immediately usable with `click --ref`.
fn filter_page_view(view: &mut Value, query: &QueryFilter) -> usize {
    const MATCH_FIELDS: [&str; 4] = ["text", "label", "name", "href"];
    let Some(obj) = view.as_object_mut() else {
        return 0;
    };
    let mut kept = 0usize;
    for section in ["landmarks", "headings", "interactive"] {
        let Some(Value::Array(entries)) = obj.get_mut(section) else {
            continue;
        };
        entries.retain(|entry| {
            MATCH_FIELDS.iter().any(|field| {
                matches!(entry.get(*field), Some(Value::String(s)) if query.matches(s))
            })
        });
        kept += entries.len();
    }
    // `interactive_total` / `interactive_truncated` describe the pre-filter
    // collection and would misreport the filtered list, so drop them here;
    // `apply_interactive_limit` re-adds them if the cap still bites.
    obj.remove("interactive_total");
    obj.remove("interactive_truncated");
    kept
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
        assert_eq!(
            meta.get("source"),
            Some(&Value::String("js-fallback".into()))
        );
    }
}
