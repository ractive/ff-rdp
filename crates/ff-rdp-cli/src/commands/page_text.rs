use ff_rdp_core::LongStringActor;
use serde_json::{Value, json};

use crate::cli::args::{Cli, PageTextArgs};
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_controls::QueryFilter;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;
use super::js_helpers::eval_or_bail;

pub fn run(cli: &Cli, args: &PageTextArgs) -> Result<(), AppError> {
    // iter-211 Theme B: reject an unreachable cap up front, before the
    // browser round-trip — same rule `--max-frame-mb 0` follows in `main`.
    // `--max-chars 0` cannot mean "no cap" here because `--full` already
    // does, so the only thing it could produce is an always-empty result.
    if args.max_chars == 0 && !args.full {
        return Err(AppError::User(
            "--max-chars must be greater than 0 (use --full to lift the cap entirely)".to_owned(),
        ));
    }

    let mut ctx = connect_and_get_target(cli)?;
    let console_actor = ctx.target.console_actor.clone();

    let eval_result = eval_or_bail(
        &mut ctx,
        &console_actor,
        "document.body.innerText",
        "failed to extract page text",
    )?;

    let text = resolve_string_result(&mut ctx, &eval_result.result)?;

    let query = QueryFilter::from_query_args(&args.query);
    let cap = if args.full { None } else { Some(args.max_chars) };
    let excerpt = build_excerpt(&text, &query, args.context, cap);

    let mut meta = json!({
        "total_chars": excerpt.total_chars,
        "truncated": excerpt.truncated,
        "max_chars": match cap { Some(n) => json!(n), None => Value::Null },
    });
    if query.is_active()
        && let Some(obj) = meta.as_object_mut()
    {
        obj.insert("matches".to_owned(), json!(excerpt.matches));
        obj.insert("shown".to_owned(), json!(excerpt.shown));
        obj.insert("context_lines".to_owned(), json!(args.context));
        obj.insert("match_lines".to_owned(), json!(excerpt.match_lines));
    }
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

    // `results` holds the text string directly; the old `.text` alias has been
    // removed (iter-61j A1).  Use `--jq '.results'` to extract the text.
    let mut envelope = output::envelope(&json!(excerpt.text), 1, &meta);
    if excerpt.truncated
        && let Some(obj) = envelope.as_object_mut()
    {
        // Deliberately not `output::envelope_with_truncation`: that helper's
        // hint is "use --all for complete list", and `--all` does nothing
        // here — the escape hatches are `--full` and `--query`.
        obj.insert("truncated".to_owned(), json!(true));
        obj.insert("hint".to_owned(), json!(excerpt.hint()));
    }

    // Text rendering of a `--query` result prefixes each line with its 1-based
    // line number, so a follow-up `page-text --full` can be scrolled to the
    // right place. Only when there is no `--jq`: with a filter the pipeline
    // applies jq first and renders its output, matching iter-60 D2 behaviour.
    if query.is_active() && cli.format == "text" && cli.jq.is_none() {
        render_numbered(&excerpt);
        return Ok(());
    }

    let hint_ctx = HintContext::new(HintSource::PageText);
    OutputPipeline::from_cli(cli)?.finalize_with_hints(&envelope, Some(&hint_ctx))
}

/// Print a `--query` excerpt as `<line-number>: <line>` rows.
fn render_numbered(excerpt: &PageTextExcerpt) {
    for (n, line) in excerpt.line_numbers.iter().zip(excerpt.text.lines()) {
        println!("{n}: {line}");
    }
    if excerpt.truncated {
        println!("{}", excerpt.hint());
    }
}

/// The bounded (and optionally query-filtered) page text plus everything
/// `meta` reports about how it was produced.
pub(crate) struct PageTextExcerpt {
    /// What lands in `results`.
    pub(crate) text: String,
    /// 1-based line numbers of every line in `text`, when a `--query`
    /// selected them. Empty when no query was active (the whole document is
    /// returned and the numbering is the document's own).
    pub(crate) line_numbers: Vec<usize>,
    /// 1-based line numbers of the *matching* lines present in `text`
    /// (context lines excluded).
    pub(crate) match_lines: Vec<usize>,
    /// Characters in the full `innerText`, before any capping or filtering.
    pub(crate) total_chars: usize,
    /// Characters actually returned in `text`.
    pub(crate) shown_chars: usize,
    /// Matching lines in the whole document.
    pub(crate) matches: usize,
    /// Matching lines that survived the `--max-chars` cap.
    pub(crate) shown: usize,
    /// Whether the cap cut anything.
    pub(crate) truncated: bool,
}

impl PageTextExcerpt {
    /// The truncation hint, worded for this command's actual escape hatches.
    pub(crate) fn hint(&self) -> String {
        format!(
            "showing {} of {} chars, use --full or --query <text>",
            self.shown_chars, self.total_chars
        )
    }
}

/// Build the excerpt `results` carries.
///
/// Two independent steps, in this order:
///
/// 1. **Select.** With an active `--query`, keep only the lines the filter
///    matches plus `context` lines either side, merged so overlapping windows
///    do not duplicate a line. Without one, keep the whole document.
/// 2. **Cap.** Trim the selection to `cap` characters. Line-selected output is
///    trimmed a whole line at a time (a half-line is not a useful excerpt);
///    unfiltered output is trimmed at the character boundary, matching the
///    "first N chars of the page" the `head -100` workaround approximated.
///
/// `total_chars` always reports the *full* document length regardless of what
/// either step removed — that is the number an agent needs in order to decide
/// whether `--full` is worth the tokens.
pub(crate) fn build_excerpt(
    full: &str,
    query: &QueryFilter,
    context: usize,
    cap: Option<usize>,
) -> PageTextExcerpt {
    let total_chars = full.chars().count();

    if !query.is_active() {
        let (text, truncated) = match cap {
            Some(cap) if total_chars > cap => (full.chars().take(cap).collect::<String>(), true),
            _ => (full.to_owned(), false),
        };
        let shown_chars = text.chars().count();
        return PageTextExcerpt {
            text,
            line_numbers: Vec::new(),
            match_lines: Vec::new(),
            total_chars,
            shown_chars,
            matches: 0,
            shown: 0,
            truncated,
        };
    }

    let lines: Vec<&str> = full.lines().collect();
    let match_idx: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| query.matches(line))
        .map(|(i, _)| i)
        .collect();
    let matches = match_idx.len();

    // Merge the ±context windows into an ordered, duplicate-free index list.
    let mut keep = vec![false; lines.len()];
    for &i in &match_idx {
        let lo = i.saturating_sub(context);
        let hi = (i + context).min(lines.len().saturating_sub(1));
        for slot in keep.iter_mut().take(hi + 1).skip(lo) {
            *slot = true;
        }
    }

    let mut kept_text: Vec<&str> = Vec::new();
    let mut line_numbers: Vec<usize> = Vec::new();
    let mut match_lines: Vec<usize> = Vec::new();
    let mut used = 0usize;
    let mut truncated = false;
    let mut shown = 0usize;
    for (i, keep_it) in keep.iter().enumerate() {
        if !keep_it {
            continue;
        }
        let line = lines[i];
        // +1 for the newline that will join this line to the previous one.
        let cost = line.chars().count() + usize::from(!kept_text.is_empty());
        if let Some(cap) = cap
            && used + cost > cap
        {
            truncated = true;
            break;
        }
        used += cost;
        kept_text.push(line);
        line_numbers.push(i + 1);
        if match_idx.binary_search(&i).is_ok() {
            match_lines.push(i + 1);
            shown += 1;
        }
    }

    let text = kept_text.join("\n");
    let shown_chars = text.chars().count();
    PageTextExcerpt {
        text,
        line_numbers,
        match_lines,
        total_chars,
        shown_chars,
        matches,
        shown,
        truncated,
    }
}

/// Resolve a Grip to a string, fetching the full content if it's a LongString.
fn resolve_string_result(
    ctx: &mut super::connect_tab::ConnectedTab,
    grip: &ff_rdp_core::Grip,
) -> Result<String, AppError> {
    match grip {
        ff_rdp_core::Grip::Value(serde_json::Value::String(s)) => Ok(s.clone()),
        ff_rdp_core::Grip::LongString {
            actor,
            length,
            initial: _,
        } => LongStringActor::full_string(ctx.transport_mut(), actor.as_ref(), *length)
            .map_err(AppError::from),
        ff_rdp_core::Grip::Null | ff_rdp_core::Grip::Undefined => Ok(String::new()),
        other => Ok(other.to_json().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::QueryArgs;

    /// The filter a caller gets when neither `--query` nor `--query-regex`
    /// was passed.
    fn inactive() -> QueryFilter {
        QueryFilter::from_query_args(&QueryArgs::default())
    }

    fn substring(text: &str) -> QueryFilter {
        QueryFilter::from_query_args(&QueryArgs {
            query: Some(text.to_owned()),
            query_regex: None,
        })
    }

    /// 60 lines with the needle on line 40 — the AC fixture shape.
    fn sixty_lines() -> String {
        (1..=60)
            .map(|n| {
                if n == 40 {
                    "the needle is here".to_owned()
                } else {
                    format!("filler line {n}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn query_keeps_only_matching_lines_with_context() {
        let doc = sixty_lines();
        let e = build_excerpt(&doc, &substring("needle"), 2, None);
        assert_eq!(e.matches, 1);
        assert_eq!(e.shown, 1);
        assert_eq!(e.text.lines().count(), 5, "±2 lines of context: {}", e.text);
        assert_eq!(e.line_numbers, vec![38, 39, 40, 41, 42]);
        assert_eq!(e.match_lines, vec![40]);
        assert!(!e.truncated);
        assert_eq!(e.total_chars, doc.chars().count());
    }

    #[test]
    fn overlapping_context_windows_do_not_duplicate_lines() {
        let doc = "a\nneedle one\nb\nneedle two\nc";
        let e = build_excerpt(doc, &substring("needle"), 2, None);
        assert_eq!(e.matches, 2);
        assert_eq!(e.text.lines().count(), 5);
        assert_eq!(e.line_numbers, vec![1, 2, 3, 4, 5]);
        assert_eq!(e.match_lines, vec![2, 4]);
    }

    #[test]
    fn zero_context_keeps_only_the_match() {
        let doc = sixty_lines();
        let e = build_excerpt(&doc, &substring("needle"), 0, None);
        assert_eq!(e.text, "the needle is here");
        assert_eq!(e.line_numbers, vec![40]);
    }

    #[test]
    fn no_match_yields_empty_text_and_zero_counts() {
        let e = build_excerpt(&sixty_lines(), &substring("no-such-token"), 2, None);
        assert_eq!(e.matches, 0);
        assert_eq!(e.shown, 0);
        assert!(e.text.is_empty());
        assert!(!e.truncated, "an empty match set is not a truncation");
    }

    #[test]
    fn cap_applies_without_a_query_and_reports_the_full_length() {
        let doc = "x".repeat(20_000);
        let e = build_excerpt(&doc, &inactive(), 2, Some(8_000));
        assert_eq!(e.total_chars, 20_000);
        assert_eq!(e.shown_chars, 8_000);
        assert!(e.truncated);
        assert!(e.hint().contains("showing 8000 of 20000 chars"), "{}", e.hint());
    }

    #[test]
    fn full_lifts_the_cap() {
        let doc = "x".repeat(20_000);
        let e = build_excerpt(&doc, &inactive(), 2, None);
        assert_eq!(e.shown_chars, 20_000);
        assert!(!e.truncated);
    }

    /// A cap that bites mid-excerpt drops whole lines and says so — `shown`
    /// must then be smaller than `matches`, which is the only signal a caller
    /// gets that there were more hits further down.
    #[test]
    fn cap_trims_query_results_line_by_line_and_shrinks_shown() {
        let doc = (1..=10)
            .map(|n| format!("needle {n} {}", "z".repeat(20)))
            .collect::<Vec<_>>()
            .join("\n");
        let e = build_excerpt(doc.as_str(), &substring("needle"), 0, Some(60));
        assert!(e.truncated);
        assert_eq!(e.matches, 10);
        assert!(e.shown < e.matches, "shown={} matches=10", e.shown);
        assert!(e.shown_chars <= 60, "shown_chars={}", e.shown_chars);
        // Whole lines only — no half-line ever lands in the excerpt.
        for line in e.text.lines() {
            assert!(doc.lines().any(|l| l == line), "partial line: {line:?}");
        }
    }

    #[test]
    fn utf8_cap_counts_characters_not_bytes() {
        let doc = "é".repeat(100);
        let e = build_excerpt(&doc, &inactive(), 2, Some(10));
        assert_eq!(e.total_chars, 100);
        assert_eq!(e.shown_chars, 10);
        assert_eq!(e.text.chars().count(), 10);
    }
}
