//! The shared "page view" collector (iter-210 Theme A, iter-219 Themes B–D).
//!
//! One JS payload and one ref registration produce the compact page view that
//! `a11y summary` prints and that every state-changing command can embed under
//! `results.page` via `--with-page`. Keeping both surfaces on this module is
//! the point: the axi benchmark (see `kb/research/axi-benchmark-comparison.md`)
//! measured ff-rdp needing 8 turns where a browser tool that returns the page
//! after every action needs 4, because `navigate`/`click`/`type` returned only
//! `{status, committed_url, ready_state}` and refs came out of `dom <selector>`
//! alone — so the agent had to already know a selector to get a handle.
//!
//! The view is deliberately the *summary* (headings, interactive elements, a
//! bounded excerpt), not the DOM tree `snapshot` returns: the benchmark's cost
//! advantage for ff-rdp came from small outputs, and a few hundred tokens per
//! action is what keeps that.
//!
//! # What iter-219 changed, and why
//!
//! The 2026-08-30 re-measurement found `--with-page` did *not* shorten the
//! click-through tasks even when agents reached for it, for two reasons that
//! were both visible in one call on `en.wikipedia.org/wiki/Ada_Lovelace`:
//!
//! 1. `interactive` was the first 50 links **in DOM order**, which on a real
//!    page is entirely site chrome ("Jump to content", "Main page", …) while
//!    `interactive_total: 1659` hid the article's own links — including the one
//!    the task needed. `click --ref` from the view was useless exactly where it
//!    mattered.
//! 2. There was no text at all, so an agent that needed "what does the page
//!    say now" fetched `page-text` anyway, spending the round trip
//!    `--with-page` exists to save.
//!
//! Both are answered by running Mozilla's `Readability.js` — the algorithm
//! behind Firefox Reader View — **on the live page** (see
//! [`super::page_view_js`]). Interactive entries inside the article element get
//! `zone: "content"`, the rest `"chrome"`, content sorts first, and the cap is
//! applied after the sort. The article's text becomes [`PageView::view`]'s
//! `excerpt`.

use std::time::{Duration, Instant};

use ff_rdp_core::ActorId;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output_controls::QueryFilter;

use super::connect_tab::ConnectedTab;
use super::js_helpers::{eval_or_bail, poll_js_condition, resolve_result};
use super::page_view_js::{build_injection_js, build_page_view_js};

/// Default cap on `interactive` entries, matching `a11y summary`'s own
/// pre-iter-210 default.
///
/// `a11y summary` lets `--all` lift it and `--limit N` override it.
/// `--with-page` always uses it: an embedded page view rides along with every
/// click, and an uncapped one on a link-heavy page would undo the token
/// advantage the whole feature is meant to preserve. Since iter-219 the cap
/// lands *after* the content/chrome sort, so it is the article's links that
/// survive it.
pub(crate) const DEFAULT_INTERACTIVE_LIMIT: usize = 50;

/// Default `--page-chars`: how much article text `--with-page` returns.
///
/// 1 500 characters is roughly a lede plus two paragraphs — enough to answer
/// "what does this page say" (the benchmark's actual question) without
/// approaching `page-text`'s 8 000-character cap, which exists for the case
/// where you want the whole article.
pub(crate) const DEFAULT_PAGE_CHARS: usize = 1500;

/// Ceiling on how much article text crosses the wire when `--query` is active.
///
/// With a query the match can be anywhere in the document, so the JS cannot
/// pre-trim to `--page-chars`; it sends up to this much and Rust picks the
/// window. Deliberately larger than `page-text --max-chars`' default and
/// smaller than "the whole DOM" — an article past this length has already lost
/// the argument about token cost.
const QUERY_TEXT_BUDGET: usize = 200_000;

/// The value `meta.page_source` (and `a11y summary`'s `meta.source`) carries.
///
/// Borrowed from `a11y`'s vocabulary so the two accessibility surfaces read the
/// same way. `js-fallback` is literally true of this collector: the summary is
/// assembled by in-page JavaScript, not by Firefox's accessibility actor —
/// which is exactly what the word means on `a11y`.
///
/// Not to be confused with the view's own `source` key, which iter-219 adds:
/// that one says how the *excerpt* was extracted (`readability` or
/// `innertext`), a different question from how the *view* was produced.
pub(crate) const PAGE_SOURCE_JS_FALLBACK: &str = "js-fallback";

/// A collected page view.
pub struct PageView {
    /// `{headings, interactive, …}` — plus `landmarks` when the caller asked
    /// for them, the `interactive_total`/`interactive_truncated` pair when the
    /// cap bit, and the reader keys (`excerpt`, `readerable`, `source`,
    /// `zone` per entry) when the reader pass ran.
    pub view: Value,
    /// Whether the `ref` handles in `view.interactive` are backed by the
    /// daemon and therefore usable with `--ref`. When `false`, no entry
    /// carries a `ref` at all (an inert handle is worse than none).
    pub refs_registered: bool,
    /// How the view was produced — see [`PAGE_SOURCE_JS_FALLBACK`].
    pub source: &'static str,
    /// Whether `document.readyState` reached `complete` before collection.
    /// `false` means the wait timed out and the view describes a still-loading
    /// document — reported rather than swallowed.
    pub ready: bool,
    /// Milliseconds the in-page clone-and-parse took, as measured by
    /// `performance.now()` inside the content process. `None` when the reader
    /// pass did not run.
    pub parse_ms: Option<f64>,
    /// Whether this call had to ship the ~32 KB Readability bundle, or found
    /// it already parked on the document from an earlier `--with-page`.
    pub readability_injected: bool,
}

/// The reader half of [`CollectOptions`] — present exactly when the caller
/// wants zones and an excerpt.
///
/// `a11y summary` passes `None`: it is the accessibility surface, it keeps
/// `landmarks`, and it has no use for an article excerpt — so it must not pay
/// for a clone-and-parse on every call.
pub struct ReaderOptions {
    /// `--page-chars`: excerpt budget in characters. `0` collects zones and
    /// `readerable` but returns no `excerpt` at all — the "structure only"
    /// knob.
    pub page_chars: usize,
    /// `--query` / `--query-regex`: narrows the excerpt to the match window
    /// and filters `interactive` by name/href, exactly as it does on
    /// `page-text` and `a11y summary`.
    pub query: QueryFilter,
    /// Lines of context either side of a `--query` match in the excerpt.
    pub context: usize,
}

/// Inputs to [`collect`].
pub struct CollectOptions {
    /// Keep at most this many `interactive` entries. `None` keeps all.
    pub interactive_limit: Option<usize>,
    /// Wait up to this many ms for `document.readyState == "complete"` before
    /// evaluating. `None` collects immediately.
    pub wait_complete_ms: Option<u64>,
    /// Collect `landmarks`. `a11y summary` does; `--with-page` does not —
    /// iter-219 Theme B dropped them from the act-and-see surface after the
    /// benchmark showed 22 entries of `{"role":"navigation","label":""}` that
    /// no trajectory ever read.
    pub landmarks: bool,
    /// Run the Readability pass. See [`ReaderOptions`].
    pub reader: Option<ReaderOptions>,
}

impl CollectOptions {
    /// The options `--with-page` uses: no landmarks, reader on.
    pub fn with_page(page_chars: usize, query: QueryFilter, context: usize) -> Self {
        Self {
            interactive_limit: Some(DEFAULT_INTERACTIVE_LIMIT),
            wait_complete_ms: None,
            landmarks: false,
            reader: Some(ReaderOptions {
                page_chars,
                query,
                context,
            }),
        }
    }
}

/// Collect the page view from the connected tab.
///
/// Ordering matters and is documented in every `--with-page` `--help`: the
/// readiness wait runs *first*, so the view describes the document the action
/// produced rather than the one it left. Ref registration runs last, against
/// the already-sorted and already-capped entry list, so exactly the handles a
/// caller can see are the handles the daemon holds.
pub fn collect(
    ctx: &mut ConnectedTab,
    console_actor: &ActorId,
    opts: &CollectOptions,
) -> Result<PageView, AppError> {
    // 1. Readiness. A timeout is not fatal: a page that never reaches
    //    `complete` (a long-polling app, a stalled subresource) still has a
    //    perfectly usable DOM, and failing the whole `click` because of it
    //    would be worse than returning the view with `ready: false`.
    let ready = match opts.wait_complete_ms {
        Some(ms) => match poll_js_condition(
            ctx,
            console_actor,
            "document.readyState === 'complete'",
            ms,
            "readyState probe threw",
            "document did not reach readyState complete",
        ) {
            Ok(_) => true,
            Err(AppError::Timeout(_)) => false,
            Err(e) => return Err(e),
        },
        None => true,
    };

    // 2. Collect. The collector reports `reader_missing` rather than injecting
    //    on its own, so the common case — a document that already carries the
    //    bundle — costs one round trip and no 32 KB payload (Theme D).
    let budget = opts.reader.as_ref().map(text_budget);
    let js = build_page_view_js(opts.landmarks, budget);
    let eval_result = eval_or_bail(ctx, console_actor, &js, "page view collection failed")?;
    let mut view = resolve_result(ctx, &eval_result.result)?;

    let mut readability_injected = false;
    if reader_missing(&view) {
        let with_bundle = format!("{};\n{js}", build_injection_js());
        let retry = eval_or_bail(
            ctx,
            console_actor,
            &with_bundle,
            "page view collection failed after injecting Readability",
        )?;
        view = resolve_result(ctx, &retry.result)?;
        readability_injected = true;
    }

    // 3. Reader post-processing: excerpt, zone sort, `--query`. Runs before the
    //    cap so the cap sees content-first order, and before refs so a ref is
    //    minted for exactly the entries the caller receives.
    let parse_ms = view.get("parse_ms").and_then(Value::as_f64);
    if let Some(reader) = opts.reader.as_ref() {
        finish_reader_view(&mut view, reader);
    }
    apply_interactive_limit(&mut view, opts.interactive_limit);

    // 4. Refs. Daemon route only, exactly as `dom` does — without a daemon
    //    there is nowhere to store the resolver, so a `ref` would be inert.
    let refs_registered = register_interactive_refs(ctx, &mut view);
    strip_resolvers(&mut view);

    Ok(PageView {
        view,
        refs_registered,
        source: PAGE_SOURCE_JS_FALLBACK,
        ready,
        parse_ms,
        readability_injected,
    })
}

/// How much article text the collector may send back.
///
/// Without a query the excerpt is the *head* of the article, so three times
/// the budget is ample slack for the boundary cut to land on a paragraph.
/// With one, the match can be anywhere, so the whole (bounded) article travels
/// and Rust selects the window.
fn text_budget(reader: &ReaderOptions) -> usize {
    if reader.query.is_active() {
        QUERY_TEXT_BUDGET
    } else if reader.page_chars == 0 {
        0
    } else {
        reader
            .page_chars
            .saturating_mul(3)
            .saturating_add(1000)
            .min(QUERY_TEXT_BUDGET)
    }
}

/// Whether the collector reported that the Readability bundle is not on this
/// document yet.
fn reader_missing(view: &Value) -> bool {
    view.get("reader_missing").and_then(Value::as_bool) == Some(true)
}

// ---------------------------------------------------------------------------
// Reader post-processing (iter-219 Themes B and C)
// ---------------------------------------------------------------------------

/// Turn the collector's raw reader keys into the published ones: build
/// `excerpt`, sort `interactive` content-first, apply `--query`, and drop the
/// internal scratch keys.
pub(crate) fn finish_reader_view(view: &mut Value, reader: &ReaderOptions) {
    let text = view
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let Some(obj) = view.as_object_mut() else {
        return;
    };
    // Scratch keys: useful to the JS, noise in the output.
    obj.remove("text");
    obj.remove("text_chars");
    obj.remove("reader_missing");
    obj.remove("parse_ms");

    if reader.page_chars > 0 {
        let excerpt = build_page_excerpt(&text, reader);
        obj.insert("excerpt".to_owned(), json!(excerpt.text));
        obj.insert("excerpt_chars".to_owned(), json!(excerpt.chars));
        obj.insert("excerpt_truncated".to_owned(), json!(excerpt.truncated));
    }

    sort_interactive_content_first(view);

    if reader.query.is_active() {
        let matches = filter_page_view(view, &reader.query);
        if let Some(obj) = view.as_object_mut() {
            obj.insert("matches".to_owned(), json!(matches));
        }
    }
}

/// The excerpt and the two numbers that describe it.
struct PageExcerpt {
    text: String,
    chars: usize,
    truncated: bool,
}

/// Build `page.excerpt` from the article text.
///
/// Three steps, in order, each of which came out of `~/devel/mdget`'s
/// reader-to-Markdown work:
///
/// 1. **Strip MediaWiki `[edit]` links.** They survive Readability (they are
///    real anchors inside the article) and turn "History[edit] In 1842 …" into
///    text an agent has to mentally filter. Harmless on every other site,
///    since the literal token appears nowhere else.
/// 2. **Skip junk lead paragraphs.** A cookie banner or "Skip to content" that
///    Readability kept as the first block would otherwise fill the excerpt on
///    exactly the pages where the excerpt matters most.
/// 3. **Cut on a boundary.** Paragraph, then sentence, then word — never
///    mid-word (see [`excerpt_at_boundary`]).
///
/// With `--query` active the selection step is `page_text::build_excerpt`
/// instead — the same ±context window semantics `page-text --query` has, so
/// one flag behaves the same way on both surfaces.
fn build_page_excerpt(text: &str, reader: &ReaderOptions) -> PageExcerpt {
    let cleaned = drop_junk_lead(&strip_edit_links(text));

    if reader.query.is_active() {
        let e = super::page_text::build_excerpt(
            &cleaned,
            &reader.query,
            reader.context,
            Some(reader.page_chars),
        );
        return PageExcerpt {
            chars: e.shown_chars,
            truncated: e.truncated,
            text: e.text,
        };
    }

    let (text, truncated) = excerpt_at_boundary(&cleaned, reader.page_chars);
    PageExcerpt {
        chars: text.chars().count(),
        truncated,
        text,
    }
}

/// Remove MediaWiki's `[edit]` / `[edit source]` anchors from article text.
pub(crate) fn strip_edit_links(text: &str) -> String {
    let mut out = text.to_owned();
    for token in ["[edit]", "[edit source]", "[ edit ]"] {
        if out.contains(token) {
            out = out.replace(token, "");
        }
    }
    out
}

/// Whether a leading block is boilerplate rather than the article's opening.
///
/// Deliberately narrow: a short line that is unmistakably navigation or a
/// consent notice. A long paragraph is never junk, however it starts —
/// mis-dropping the lede is a far worse failure than keeping one banner line.
fn looks_like_junk_lead(line: &str) -> bool {
    const JUNK_MAX_CHARS: usize = 200;
    if line.chars().count() > JUNK_MAX_CHARS {
        return false;
    }
    let lower = line.to_lowercase();
    let cookie_notice = lower.contains("cookie")
        && (lower.contains("accept") || lower.contains("consent") || lower.contains("we use"));
    cookie_notice
        || lower.starts_with("skip to ")
        || lower.starts_with("jump to ")
        || lower.contains("enable javascript")
        || lower.contains("javascript is disabled")
}

/// Drop leading junk lines, but never the last remaining one — an excerpt of
/// the banner beats an empty excerpt on a page whose only text *is* a banner.
fn drop_junk_lead(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut start = 0usize;
    while start + 1 < lines.len() && looks_like_junk_lead(lines[start]) {
        start += 1;
    }
    if start == 0 {
        return text.to_owned();
    }
    lines[start..].join("\n")
}

/// Cut `text` to at most `max_chars`, preferring a natural boundary.
///
/// Ported from `mdget`'s `truncate_output`. The order is paragraph break,
/// then sentence end, then word break, then a hard character cut; a boundary
/// is only accepted if it keeps at least [`MIN_BOUNDARY_FRACTION`] of the
/// budget, so a document with one newline near the start does not collapse the
/// excerpt to a single line.
///
/// Returns the cut text and whether anything was removed. No ellipsis is
/// appended: the value is consumed as JSON, and `excerpt_truncated` already
/// says what an ellipsis would.
pub(crate) fn excerpt_at_boundary(text: &str, max_chars: usize) -> (String, bool) {
    /// A boundary must leave at least this many tenths of the budget filled.
    ///
    /// Integer tenths rather than a float fraction so the floor is exact at
    /// every budget — a `usize as f64` round-trip is both lossy above 2^53 and
    /// a clippy denial for exactly that reason.
    const MIN_BOUNDARY_TENTHS: usize = 6;

    if max_chars == 0 {
        return (String::new(), !text.is_empty());
    }
    if text.chars().count() <= max_chars {
        return (text.to_owned(), false);
    }

    let head: String = text.chars().take(max_chars).collect();
    let floor = max_chars / 10 * MIN_BOUNDARY_TENTHS;

    // 1. Paragraph boundary — the block joins the collector emits.
    if let Some(idx) = head.rfind('\n')
        && head[..idx].chars().count() >= floor
    {
        return (head[..idx].trim_end().to_owned(), true);
    }

    // 2. Sentence end. `. ` and friends, plus CJK full stops which carry no
    //    trailing space.
    let sentence_end = head
        .char_indices()
        .filter_map(|(i, c)| {
            if !matches!(c, '.' | '!' | '?' | '。' | '！' | '？') {
                return None;
            }
            let end = i + c.len_utf8();
            let followed_by_break = match head[end..].chars().next() {
                None => true,
                Some(next) => next.is_whitespace() || matches!(next, '。' | '！' | '？'),
            };
            (followed_by_break && head[..end].chars().count() >= floor).then_some(end)
        })
        .next_back();
    if let Some(end) = sentence_end {
        return (head[..end].trim_end().to_owned(), true);
    }

    // 3. Word boundary.
    if let Some(idx) = head.rfind(char::is_whitespace)
        && head[..idx].chars().count() >= floor
    {
        return (head[..idx].trim_end().to_owned(), true);
    }

    // 4. Nothing to hold on to (one very long token) — a hard cut it is.
    (head, true)
}

/// Order `interactive` content-first, chrome after, stable within each group.
///
/// The whole reason zones exist: the 50-entry cap must fall on the navigation
/// bar, not on the article. Entries with no `zone` (the reader pass did not
/// run) sort as content so a view without zones keeps DOM order exactly.
pub(crate) fn sort_interactive_content_first(view: &mut Value) {
    let Some(Value::Array(arr)) = view.get_mut("interactive") else {
        return;
    };
    // `sort_by_key` is stable, which is what keeps DOM order inside a zone.
    arr.sort_by_key(|e| u8::from(e.get("zone").and_then(Value::as_str) == Some("chrome")));
}

/// Filter `headings`, `landmarks` and `interactive` down to the entries
/// matching `query`, returning the total number of survivors (iter-211
/// Theme A, generalised to `--with-page` by iter-219 Theme C).
///
/// Each section is judged on its own human-readable field — `text` for
/// headings, `label` for landmarks, `name` for interactive entries — plus
/// `href`, so "find the link to /pricing" works as well as "find the link
/// called Pricing". Entries are kept whole, `ref` included, so a survivor is
/// immediately usable with `click --ref`.
pub(crate) fn filter_page_view(view: &mut Value, query: &QueryFilter) -> usize {
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
            MATCH_FIELDS.iter().any(
                |field| matches!(entry.get(*field), Some(Value::String(s)) if query.matches(s)),
            )
        });
        kept += entries.len();
    }
    // `interactive_total` / `interactive_truncated` describe the pre-filter
    // collection and would misreport the filtered list, so drop them here;
    // `apply_interactive_limit` re-adds them if the cap still bites.
    obj.remove("interactive_total");
    obj.remove("interactive_truncated");
    obj.remove("chrome_omitted");
    kept
}

/// Truncate `view.interactive` to `limit`, recording `interactive_total`,
/// `interactive_truncated`, and — when the entries carry zones —
/// `chrome_omitted`.
///
/// `chrome_omitted` is the honest counterpart to sorting content first: an
/// agent that needs a nav link ("Log in") can see the navigation exists and
/// that `--query` will reach it, instead of concluding the page has none.
///
/// It counts chrome entries only, by design: content sorts first, so content
/// entries land in the truncated tail only when a page has more than `limit`
/// content links, and that case is deliberately left to `interactive_total`
/// (the full pre-cap count) rather than a second counter — documented in
/// `--help`'s act-and-see block so the asymmetry is contract, not accident.
pub(crate) fn apply_interactive_limit(view: &mut Value, limit: Option<usize>) {
    let Some(limit) = limit else { return };
    let Some(Value::Array(arr)) = view.get_mut("interactive") else {
        return;
    };
    let total = arr.len();
    if total <= limit {
        return;
    }
    let zoned = arr.iter().any(|e| e.get("zone").is_some());
    let chrome_omitted = arr
        .iter()
        .skip(limit)
        .filter(|e| e.get("zone").and_then(Value::as_str) == Some("chrome"))
        .count();
    arr.truncate(limit);
    if let Some(obj) = view.as_object_mut() {
        obj.insert("interactive_total".to_owned(), json!(total));
        obj.insert("interactive_truncated".to_owned(), json!(true));
        if zoned {
            obj.insert("chrome_omitted".to_owned(), json!(chrome_omitted));
        }
    }
}

/// Allocate and register a `ref` for every interactive entry that carries a
/// `__resolver`, returning whether the registration succeeded.
///
/// On any failure (no daemon, allocation refused, the page navigated between
/// alloc and register) no `ref` field is added at all — the same fail-closed
/// rule `dom` applies, because a handle that cannot resolve is a trap.
fn register_interactive_refs(ctx: &mut ConnectedTab, view: &mut Value) -> bool {
    if !ctx.via_daemon {
        return false;
    }
    let count = interactive_entries(view)
        .filter(|e| e.get("__resolver").and_then(Value::as_str).is_some())
        .count();
    if count == 0 {
        return false;
    }

    let Ok((start, nav_gen)) = crate::daemon::client::alloc_refs(ctx.transport_mut(), count as u64)
    else {
        return false;
    };

    let mut entries: Vec<crate::daemon::client::RefEntry> = Vec::with_capacity(count);
    let mut next = start;
    if let Some(Value::Array(arr)) = view.get_mut("interactive") {
        for node in arr.iter_mut() {
            let Some(map) = node.as_object_mut() else {
                continue;
            };
            let Some(resolver) = map.get("__resolver").and_then(Value::as_str) else {
                continue;
            };
            let id = format!("e{next}");
            next += 1;
            entries.push(crate::daemon::client::RefEntry {
                id: id.clone(),
                resolver: resolver.to_owned(),
            });
            map.insert("ref".to_owned(), json!(id));
        }
    }

    if crate::daemon::client::register_refs(ctx.transport_mut(), nav_gen, &entries).is_ok() {
        true
    } else {
        strip_ref_fields(view);
        false
    }
}

/// Iterate the `interactive` entries of a page view.
fn interactive_entries(view: &Value) -> impl Iterator<Item = &Value> {
    view.get("interactive")
        .and_then(Value::as_array)
        .map_or_else(|| [].iter(), |a| a.iter())
}

fn strip_resolvers(view: &mut Value) {
    if let Some(Value::Array(arr)) = view.get_mut("interactive") {
        for node in arr.iter_mut() {
            if let Some(map) = node.as_object_mut() {
                map.remove("__resolver");
            }
        }
    }
}

fn strip_ref_fields(view: &mut Value) {
    if let Some(Value::Array(arr)) = view.get_mut("interactive") {
        for node in arr.iter_mut() {
            if let Some(map) = node.as_object_mut() {
                map.remove("ref");
            }
        }
    }
}

/// Collect a page view and attach it to a command's `results` (iter-210
/// Theme A).
///
/// The one call site every `--with-page` command uses, and it must run while
/// the command still owns its connection. Adds two keys:
///
/// - `results.page` — the view itself.
/// - `results.page_meta` — `{source, ready, refs_registered, parse_ms,
///   readability_injected}`, which [`lift_meta`] moves into the envelope's
///   `meta` before printing. It rides in `results` only because the commands
///   that collect the page (`navigate`/`click`/`type`) build their envelope in
///   a *different* function from the one holding the connection — the same
///   reason `settle_method` already travels this way.
pub(crate) fn attach(
    ctx: &mut ConnectedTab,
    results: &mut Value,
    wait_complete_ms: Option<u64>,
    args: &crate::cli::args::PageViewArgs,
) -> Result<(), AppError> {
    let mut opts = CollectOptions::with_page(
        args.page_chars,
        QueryFilter::from_query_args(&args.query),
        args.context,
    );
    opts.wait_complete_ms = wait_complete_ms;
    // iter-211 Theme A, applied to `--with-page`: with a `--query` the cap has
    // to fall *after* the filter, or the entry the caller asked for is exactly
    // the one hidden by it.
    let limit = opts.interactive_limit;
    if args.query.query.is_some() || args.query.query_regex.is_some() {
        opts.interactive_limit = None;
    }
    let mut page = collect_settled(ctx, &opts)?;
    if opts.interactive_limit.is_none() {
        apply_interactive_limit(&mut page.view, limit);
    }
    insert_page(results, page);
    Ok(())
}

/// How long [`collect_settled`] will wait for a navigation the action started
/// to hand over a new document before giving up and collecting whatever the
/// tab reports (iter-220).
///
/// Deliberately far below the default `--timeout` of 10 s: the wait is a
/// *poll of `getTarget`*, which the parent process answers in a millisecond or
/// two even while the content process is busy, so 3 s is many times what a
/// commit needs. Exceeding it means the destination is genuinely slow, and a
/// view of the outgoing page beats no view at all.
const NAV_SETTLE_BUDGET_MS: u64 = 3_000;

/// Interval between `getTarget` polls while waiting for a navigation to commit.
const NAV_SETTLE_POLL_MS: u64 = 25;

/// How many times [`collect_settled`] re-resolves the target and collects again
/// after Firefox reported the document it was talking to is going away.
///
/// Two is enough for the observed shape (outgoing document → destination); a
/// third pass would only help a page that navigates twice in a row, and each
/// pass costs a full collection.
const NAV_COLLECT_ATTEMPTS: usize = 3;

/// Collect the page view against the document the action actually produced.
///
/// # The bug this exists for (iter-220)
///
/// `click --ref <link> --with-page` on `en.wikipedia.org/wiki/Ada_Lovelace`
/// timed out — `phase: recv`, the full `--timeout` budget, 3 runs out of 3 —
/// from iter-210 until this function landed. The wire traces say why, and it is
/// not what `refresh_target`'s doc comment assumes:
///
/// 1. The click dispatches and Firefox pushes
///    `tabNavigated {state: "start", url: <destination>}`.
/// 2. `getTarget` is called immediately afterwards and hands back **the
///    outgoing document** — same `innerWindowId`, same `url`, merely re-forwarded
///    under a fresh `childN/` prefix. Refreshing the target does not escape the
///    doomed docshell, because as far as the tab descriptor is concerned the
///    navigation has not committed yet.
/// 3. The collector then evaluates against that docshell. Sometimes it gets as
///    far as a 250 KB long-string fetch; then the docshell is torn down and
///    Firefox simply stops answering. No error, no `noSuchActor` — the request
///    is dropped, and the client sits on the socket until `--timeout` expires.
///
/// So the fix cannot be "refresh and hope". It has to *wait for the navigation
/// the action started*, and it needs a positive signal that one is under way,
/// which [`RdpTransport::take_navigation_started`] provides.
///
/// # What it does
///
/// - No navigation announced → collect straight away, exactly as before. A
///   plain `click --with-page` on a button pays nothing.
/// - A navigation announced → poll `getTarget` (a parent-process call, ~1 ms)
///   until the target reports a different `innerWindowId` or the destination
///   URL, then collect against that.
/// - Mid-collection teardown → [`ConnectedTab::set_target_guard`] turns
///   Firefox's silence into [`AppError::RdpActorDestroyed`] within tens of
///   milliseconds instead of a `--timeout`-long stall, and the next attempt
///   settles and collects again.
///
/// [`RdpTransport::take_navigation_started`]: ff_rdp_core::RdpTransport::take_navigation_started
fn collect_settled(ctx: &mut ConnectedTab, opts: &CollectOptions) -> Result<PageView, AppError> {
    let mut pending = ctx.take_navigation_started();
    let mut last_err = None;

    for _ in 0..NAV_COLLECT_ATTEMPTS {
        // The document the action ran against. When a navigation is under way
        // this is precisely the docshell that must NOT be collected from.
        let before = ctx.target.inner_window_id;
        if let Some(dest) = pending.as_deref() {
            settle_after_navigation(ctx, dest, before);
        } else {
            ctx.refresh_target();
        }

        let console_actor = ctx.target.console_actor.clone();
        // Arm the guard for the collection only. Anything else on this
        // connection keeps the pre-iter-220 behaviour of waiting for its reply.
        ctx.set_target_guard(ctx.target.inner_window_id);
        let outcome = collect(ctx, &console_actor, opts);
        ctx.set_target_guard(None);

        match outcome {
            Ok(page) => return Ok(page),
            Err(e @ AppError::RdpActorDestroyed { .. }) => {
                // Another navigation landed while we were collecting. Take its
                // destination (`recv` latched it) and go round again.
                pending = ctx.take_navigation_started().or(pending);
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        AppError::Internal(anyhow::anyhow!(
            "page view collection made no attempt — NAV_COLLECT_ATTEMPTS must be non-zero"
        ))
    }))
}

/// Poll `getTarget` until the tab stops reporting the document the action was
/// performed on, or [`NAV_SETTLE_BUDGET_MS`] elapses.
///
/// Two independent exit conditions, because neither alone covers both kinds of
/// navigation:
///
/// - **`innerWindowId` changed** — a cross-document load committed. This is the
///   Wikipedia case.
/// - **the target's URL is the announced destination** — a *same-document*
///   navigation (a `#fragment` link) flips the URL and never changes
///   `innerWindowId`, so waiting on the id alone would burn the whole budget on
///   a page that was ready immediately.
///
/// Returning after the budget is not an error: the caller collects whatever the
/// tab reports and the view says which document it describes.
fn settle_after_navigation(ctx: &mut ConnectedTab, destination: &str, before: Option<u64>) {
    let deadline = Instant::now() + Duration::from_millis(NAV_SETTLE_BUDGET_MS);
    loop {
        ctx.refresh_target();
        let changed_document = before.is_some() && ctx.target.inner_window_id != before;
        let at_destination =
            !destination.is_empty() && ctx.target.url.as_deref() == Some(destination);
        if changed_document || at_destination || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(NAV_SETTLE_POLL_MS));
    }
}

/// Write `page` and `page_meta` into `results`.
///
/// Split out of [`attach`] so the shape contract can be tested without a live
/// Firefox: the `page` object must be the collected view **verbatim**. Nothing
/// describing how the view was produced may be added to it — that all belongs
/// in `page_meta`, which [`lift_meta`] moves into the envelope's `meta`.
fn insert_page(results: &mut Value, page: PageView) {
    let Some(obj) = results.as_object_mut() else {
        return;
    };
    obj.insert("page".to_owned(), page.view);
    obj.insert(
        "page_meta".to_owned(),
        json!({
            "source": page.source,
            "ready": page.ready,
            "refs_registered": page.refs_registered,
            "parse_ms": page.parse_ms,
            "readability_injected": page.readability_injected,
        }),
    );
}

/// Move [`attach`]'s `results.page_meta` into the envelope's `meta` as
/// `page_source` / `page_ready` / `page_refs_registered` / `page_parse_ms` /
/// `page_readability_injected`, and — in `--format text` without `--jq` — take
/// `results.page` out so the caller can print it with [`render_text_section`]
/// beneath its own line.
///
/// Text mode has to remove it: the generic text renderer falls back to
/// pretty-printed JSON for any `results` object with a nested value, so
/// leaving `page` in place would replace the command's own key-value line with
/// a wall of JSON.
///
/// A no-op when `--with-page` was not passed.
pub(crate) fn lift_meta(cli: &Cli, results: &mut Value, meta: &mut Value) -> Option<Value> {
    let page_meta = results.as_object_mut()?.remove("page_meta")?;
    if let Some(obj) = meta.as_object_mut() {
        for (from, to) in [
            ("source", "page_source"),
            ("ready", "page_ready"),
            ("refs_registered", "page_refs_registered"),
            ("parse_ms", "page_parse_ms"),
            ("readability_injected", "page_readability_injected"),
        ] {
            match page_meta.get(from) {
                // A null `parse_ms` means the reader pass did not run; an
                // explicit null in `meta` would read as "it ran and reported
                // nothing", so leave the key out entirely.
                Some(Value::Null) | None => {}
                Some(v) => {
                    obj.insert(to.to_owned(), v.clone());
                }
            }
        }
    }
    if cli.format == "text" && cli.jq.is_none() {
        return results.as_object_mut()?.remove("page");
    }
    None
}

/// Print the page view beneath a command's own `--format text` output, plus
/// the one hint line that tells an agent what to do with the refs it just got.
pub(crate) fn render_text_section(page: Option<&Value>) {
    let Some(page) = page else { return };
    println!();
    render_text(page);
    if first_ref(page).is_some() {
        println!("-> ff-rdp click --ref <ref>  # act on an element above");
    }
}

/// The first usable `ref` in a page view, if any.
fn first_ref(page: &Value) -> Option<&str> {
    interactive_entries(page).find_map(|e| e.get("ref").and_then(Value::as_str))
}

/// Render a page view as human-readable text.
///
/// Shared by `a11y summary` (its whole output) and by `--with-page` (the
/// section beneath the command's own line), so the two can never drift.
pub(crate) fn render_text(results: &Value) {
    // Landmarks
    if let Some(landmarks) = results.get("landmarks").and_then(Value::as_array)
        && !landmarks.is_empty()
    {
        println!("LANDMARKS");
        for lm in landmarks {
            let role = lm.get("role").and_then(Value::as_str).unwrap_or("?");
            let tag = lm.get("tag").and_then(Value::as_str).unwrap_or("");
            let label = lm.get("label").and_then(Value::as_str).unwrap_or("");
            if label.is_empty() {
                println!("  {role} <{tag}>");
            } else {
                println!("  {role} <{tag}> \"{label}\"");
            }
        }
        println!();
    }

    // Headings
    if let Some(headings) = results.get("headings").and_then(Value::as_array)
        && !headings.is_empty()
    {
        println!("HEADINGS");
        for h in headings {
            let level = h.get("level").and_then(Value::as_u64).unwrap_or(0);
            let text = h.get("text").and_then(Value::as_str).unwrap_or("");
            let indent = "  ".repeat(usize::try_from(level).unwrap_or(0));
            println!("{indent}h{level} {text}");
        }
        println!();
    }

    // Excerpt (iter-219 Theme C). Printed before the interactive list because
    // "what does the page say" is the question an agent asks first, and the
    // list can be 50 lines long.
    if let Some(excerpt) = results.get("excerpt").and_then(Value::as_str)
        && !excerpt.is_empty()
    {
        let source = results.get("source").and_then(Value::as_str).unwrap_or("");
        let truncated = results.get("excerpt_truncated").and_then(Value::as_bool) == Some(true);
        let suffix = if truncated { ", truncated" } else { "" };
        println!(
            "EXCERPT ({} chars{suffix}) [{source}]",
            excerpt.chars().count()
        );
        println!("{excerpt}");
        println!();
    }

    // Interactive
    if let Some(interactive) = results.get("interactive").and_then(Value::as_array)
        && !interactive.is_empty()
    {
        println!("INTERACTIVE ({} elements)", interactive.len());
        for el in interactive {
            let role = el.get("role").and_then(Value::as_str).unwrap_or("?");
            let name = el.get("name").and_then(Value::as_str).unwrap_or("");
            // iter-210 Theme B: the ref is the whole point of the line now —
            // lead with it so an agent scanning the block can copy one token.
            let prefix = match el.get("ref").and_then(Value::as_str) {
                Some(r) => format!("  [{r}] "),
                None => "  ".to_owned(),
            };
            // iter-219 Theme B: `chrome` is the noteworthy half — content is
            // what the agent expects to be reading, so only nav is marked.
            let zone = match el.get("zone").and_then(Value::as_str) {
                Some("chrome") => " (chrome)",
                _ => "",
            };
            match role {
                "link" => {
                    let href = el.get("href").and_then(Value::as_str).unwrap_or("");
                    println!("{prefix}link \"{name}\" -> {href}{zone}");
                }
                "button" => {
                    println!("{prefix}button \"{name}\"{zone}");
                }
                "input" => {
                    let itype = el.get("type").and_then(Value::as_str).unwrap_or("text");
                    println!("{prefix}input[{itype}] \"{name}\"{zone}");
                }
                _ => {
                    println!("{prefix}{role} \"{name}\"{zone}");
                }
            }
        }
        if let Some(true) = results
            .get("interactive_truncated")
            .and_then(Value::as_bool)
        {
            let total = usize::try_from(
                results
                    .get("interactive_total")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            let omitted = results.get("chrome_omitted").and_then(Value::as_u64);
            match omitted {
                Some(n) => println!(
                    "  ... and {} more ({n} chrome) — use --query <text> or --all",
                    total.saturating_sub(interactive.len())
                ),
                None => println!(
                    "  ... and {} more (use --all for complete list)",
                    total.saturating_sub(interactive.len())
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::QueryArgs;
    use clap::Parser as _;

    /// Parse a `Cli` from argv for the flag-reading helpers under test.
    fn test_cli(argv: &[&str]) -> Cli {
        Cli::parse_from(argv)
    }

    fn no_query() -> QueryFilter {
        QueryFilter::from_query_args(&QueryArgs::default())
    }

    fn query(text: &str) -> QueryFilter {
        QueryFilter::from_query_args(&QueryArgs {
            query: Some(text.to_owned()),
            query_regex: None,
        })
    }

    fn reader(page_chars: usize, q: QueryFilter) -> ReaderOptions {
        ReaderOptions {
            page_chars,
            query: q,
            context: 2,
        }
    }

    // ── iter-219 Theme C: excerpt boundaries ────────────────────────────────

    #[test]
    fn unit_219_short_text_is_returned_whole() {
        let (text, truncated) = excerpt_at_boundary("a short article", 100);
        assert_eq!(text, "a short article");
        assert!(!truncated);
    }

    #[test]
    fn unit_219_cut_prefers_a_paragraph_boundary() {
        let doc = format!("{}\n{}", "p".repeat(80), "q".repeat(80));
        let (text, truncated) = excerpt_at_boundary(&doc, 100);
        assert_eq!(text, "p".repeat(80), "must cut at the newline");
        assert!(truncated);
    }

    #[test]
    fn unit_219_cut_falls_back_to_a_sentence_boundary() {
        let doc = "First sentence here. Second sentence runs on and on and on and on and on.";
        // 32 leaves the full stop past the 60%-of-budget floor, so the
        // sentence boundary wins over the later word boundary.
        let (text, truncated) = excerpt_at_boundary(doc, 32);
        assert_eq!(text, "First sentence here.");
        assert!(truncated);
    }

    #[test]
    fn unit_219_cut_never_lands_mid_word() {
        let doc = "alpha bravo charlie delta echo foxtrot golf hotel india juliett kilo lima";
        let (text, truncated) = excerpt_at_boundary(doc, 30);
        assert!(truncated);
        assert!(
            doc.starts_with(&text),
            "the excerpt must be a prefix: {text:?}"
        );
        assert!(
            !text.ends_with("charli") && doc[text.len()..].starts_with(' '),
            "cut landed mid-word: {text:?}"
        );
    }

    /// One unbroken token longer than the budget still has to be cut — the
    /// alternative is returning more than the caller asked for.
    #[test]
    fn unit_219_a_single_long_token_is_hard_cut() {
        let doc = "x".repeat(500);
        let (text, truncated) = excerpt_at_boundary(&doc, 100);
        assert_eq!(text.chars().count(), 100);
        assert!(truncated);
    }

    #[test]
    fn unit_219_zero_budget_yields_nothing_but_reports_truncation() {
        let (text, truncated) = excerpt_at_boundary("some text", 0);
        assert!(text.is_empty());
        assert!(truncated);
    }

    #[test]
    fn unit_219_multibyte_text_cuts_on_character_boundaries() {
        let doc = "Ada Lovelace était une mathématicienne. Elle a écrit le premier programme.";
        let (text, truncated) = excerpt_at_boundary(doc, 45);
        assert!(truncated);
        assert_eq!(text, "Ada Lovelace était une mathématicienne.");
    }

    #[test]
    fn unit_219_edit_links_are_stripped() {
        assert_eq!(
            strip_edit_links("History[edit] In 1842 she wrote[edit source] a note"),
            "History In 1842 she wrote a note"
        );
        assert_eq!(strip_edit_links("no links here"), "no links here");
    }

    #[test]
    fn unit_219_junk_lead_paragraphs_are_skipped() {
        let doc = "Skip to content\nWe use cookies to improve your experience. Accept?\n\
                   Augusta Ada King was an English mathematician.";
        assert_eq!(
            drop_junk_lead(doc),
            "Augusta Ada King was an English mathematician."
        );
    }

    /// A page whose only text is a banner keeps the banner: an empty excerpt
    /// would be a worse answer than an honest one.
    #[test]
    fn unit_219_junk_only_page_keeps_its_last_line() {
        assert_eq!(drop_junk_lead("Skip to content"), "Skip to content");
    }

    /// A long opening paragraph is never junk, whatever words it contains —
    /// dropping a real lede is the expensive mistake.
    #[test]
    fn unit_219_a_long_lead_is_never_junk() {
        let lead = format!(
            "We use cookies in the historical sense: {}",
            "word ".repeat(60)
        );
        assert!(!looks_like_junk_lead(&lead));
    }

    // ── iter-219 Theme B: zones, sorting, the cap ───────────────────────────

    fn zoned_view(entries: &[(&str, &str)]) -> Value {
        json!({
            "headings": [],
            "interactive": entries.iter().map(|(name, zone)| json!({
                "role": "link", "name": name, "href": format!("/{name}"), "zone": zone
            })).collect::<Vec<_>>()
        })
    }

    #[test]
    fn unit_219_content_sorts_before_chrome_and_keeps_dom_order_within_a_zone() {
        let mut view = zoned_view(&[
            ("nav-a", "chrome"),
            ("body-1", "content"),
            ("nav-b", "chrome"),
            ("body-2", "content"),
        ]);
        sort_interactive_content_first(&mut view);
        let names: Vec<&str> = view["interactive"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|e| e["name"].as_str())
            .collect();
        assert_eq!(names, ["body-1", "body-2", "nav-a", "nav-b"]);
    }

    /// AC: the article's link survives the 50-cap on a page with 1 600 nav
    /// links, and the count of dropped chrome is reported.
    #[test]
    fn unit_219_cap_keeps_content_and_reports_chrome_omitted() {
        let mut entries: Vec<(String, &str)> =
            (0..60).map(|i| (format!("nav-{i}"), "chrome")).collect();
        entries.push(("Charles Babbage".to_owned(), "content"));
        let pairs: Vec<(&str, &str)> = entries.iter().map(|(n, z)| (n.as_str(), *z)).collect();
        let mut view = zoned_view(&pairs);

        sort_interactive_content_first(&mut view);
        apply_interactive_limit(&mut view, Some(50));

        let names: Vec<&str> = view["interactive"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|e| e["name"].as_str())
            .collect();
        assert_eq!(names.len(), 50);
        assert_eq!(
            names[0], "Charles Babbage",
            "the content link must survive the cap"
        );
        assert_eq!(view["interactive_total"], json!(61));
        assert_eq!(view["chrome_omitted"], json!(11));
    }

    /// Without zones — `a11y summary`'s shape — nothing about the cap changes,
    /// and no `chrome_omitted` key appears to confuse a caller.
    #[test]
    fn unit_219_unzoned_cap_behaves_exactly_as_before() {
        let mut view = json!({
            "landmarks": [], "headings": [],
            "interactive": (0..5).map(|i| json!({"role": "link", "name": i.to_string()}))
                .collect::<Vec<_>>()
        });
        apply_interactive_limit(&mut view, Some(2));
        assert_eq!(view["interactive"].as_array().map(Vec::len), Some(2));
        assert_eq!(view["interactive_total"], json!(5));
        assert_eq!(view["interactive_truncated"], json!(true));
        assert!(view.get("chrome_omitted").is_none());
    }

    #[test]
    fn interactive_limit_below_cap_adds_no_truncation_keys() {
        let mut view = json!({
            "landmarks": [], "headings": [],
            "interactive": [{"role": "link", "name": "a"}]
        });
        apply_interactive_limit(&mut view, Some(50));
        assert!(view.get("interactive_total").is_none());
        assert!(view.get("interactive_truncated").is_none());
    }

    #[test]
    fn interactive_limit_none_keeps_everything() {
        let mut view = json!({
            "landmarks": [], "headings": [],
            "interactive": (0..80).map(|i| json!({"role": "link", "name": i.to_string()}))
                .collect::<Vec<_>>()
        });
        apply_interactive_limit(&mut view, None);
        assert_eq!(view["interactive"].as_array().map(Vec::len), Some(80));
    }

    // ── iter-219: the collector's raw output becomes the published view ─────

    /// The shape the JS hands back, before Rust touches it.
    fn raw_reader_view() -> Value {
        json!({
            "headings": [{"level": 1, "text": "Ada Lovelace"}],
            "interactive": [
                {"role": "link", "name": "Jump to content", "href": "#main", "zone": "chrome"},
                {"role": "link", "name": "Charles Babbage", "href": "/babbage", "zone": "content"}
            ],
            "source": "readability",
            "readerable": true,
            "parse_ms": 12.5,
            "text": "Augusta Ada King, Countess of Lovelace was an English mathematician.\n\
                     She worked with Charles Babbage, born in 1791, on the Analytical Engine.",
            "text_chars": 141
        })
    }

    #[test]
    fn unit_219_finish_builds_the_excerpt_and_drops_scratch_keys() {
        let mut view = raw_reader_view();
        finish_reader_view(&mut view, &reader(DEFAULT_PAGE_CHARS, no_query()));

        assert!(
            view["excerpt"]
                .as_str()
                .unwrap_or_default()
                .starts_with("Augusta Ada King"),
            "the excerpt must open with the lede: {view}"
        );
        assert_eq!(view["excerpt_truncated"], json!(false));
        assert_eq!(view["source"], json!("readability"));
        assert_eq!(view["readerable"], json!(true));
        for scratch in ["text", "text_chars", "parse_ms", "reader_missing"] {
            assert!(
                view.get(scratch).is_none(),
                "{scratch} is internal and must not be published: {view}"
            );
        }
        // …and the content link is now first.
        assert_eq!(view["interactive"][0]["name"], json!("Charles Babbage"));
    }

    /// `--page-chars 0` is the documented "structure only" knob: zones and
    /// `readerable` stay, the excerpt goes away entirely.
    #[test]
    fn unit_219_page_chars_zero_returns_no_excerpt() {
        let mut view = raw_reader_view();
        finish_reader_view(&mut view, &reader(0, no_query()));
        assert!(view.get("excerpt").is_none(), "{view}");
        assert!(view.get("excerpt_chars").is_none());
        assert_eq!(view["readerable"], json!(true));
        assert_eq!(view["interactive"][0]["zone"], json!("content"));
    }

    /// `--query` narrows the excerpt to the match window *and* the interactive
    /// list, and reports how many entries matched.
    #[test]
    fn unit_219_query_narrows_the_excerpt_and_the_interactive_list() {
        let mut view = raw_reader_view();
        finish_reader_view(&mut view, &reader(DEFAULT_PAGE_CHARS, query("Babbage")));

        let excerpt = view["excerpt"].as_str().unwrap_or_default();
        assert!(
            excerpt.contains("1791"),
            "excerpt must be the match window: {excerpt:?}"
        );
        let names: Vec<&str> = view["interactive"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|e| e["name"].as_str())
            .collect();
        assert_eq!(names, ["Charles Babbage"]);
        assert_eq!(view["matches"], json!(1));
    }

    #[test]
    fn unit_219_text_budget_scales_with_page_chars_and_opens_up_for_a_query() {
        assert_eq!(text_budget(&reader(1500, no_query())), 5500);
        assert_eq!(text_budget(&reader(0, no_query())), 0);
        assert_eq!(text_budget(&reader(1500, query("x"))), QUERY_TEXT_BUDGET);
        // A wild --page-chars cannot ask the page for an unbounded string.
        assert_eq!(
            text_budget(&reader(usize::MAX, no_query())),
            QUERY_TEXT_BUDGET
        );
    }

    #[test]
    fn unit_219_reader_missing_is_detected() {
        assert!(reader_missing(&json!({"reader_missing": true})));
        assert!(!reader_missing(&json!({"headings": []})));
    }

    // ── unchanged iter-210 contracts ────────────────────────────────────────

    #[test]
    fn strip_helpers_remove_their_fields() {
        let mut view = json!({
            "interactive": [{"role": "link", "ref": "e1", "__resolver": "a:nth-child(1)"}]
        });
        strip_resolvers(&mut view);
        assert!(view["interactive"][0].get("__resolver").is_none());
        strip_ref_fields(&mut view);
        assert!(view["interactive"][0].get("ref").is_none());
    }

    #[test]
    fn first_ref_finds_the_leading_handle() {
        let page = json!({"interactive": [{"role": "link"}, {"role": "button", "ref": "e7"}]});
        assert_eq!(first_ref(&page), Some("e7"));
        let none = json!({"interactive": [{"role": "link"}]});
        assert_eq!(first_ref(&none), None);
    }

    #[test]
    fn render_text_covers_every_role_and_the_truncation_note() {
        let results = json!({
            "landmarks": [
                {"role": "banner", "tag": "header", "label": ""},
                {"role": "main", "tag": "main", "label": "Content"}
            ],
            "headings": [{"level": 1, "text": "Page Title"}, {"level": 2, "text": "Section"}],
            "excerpt": "The article opens here.",
            "excerpt_truncated": true,
            "source": "readability",
            "interactive": [
                {"role": "link", "name": "Home", "href": "/", "ref": "e1", "zone": "chrome"},
                {"role": "button", "name": "Submit", "zone": "content"},
                {"role": "input", "name": "Email", "type": "email"},
                {"role": "unknown", "name": "Widget"}
            ],
            "interactive_total": 10,
            "interactive_truncated": true,
            "chrome_omitted": 5
        });
        // Exercises every branch; the assertion is that it does not panic.
        render_text(&results);
        render_text_section(Some(&results));
        render_text_section(None);
    }

    /// A view shaped like what `collect` returns on the fixture pages.
    fn sample_view() -> Value {
        json!({
            "headings": [{"level": 1, "text": "Ada Lovelace"}],
            "interactive": [{"role": "link", "name": "Charles Babbage",
                             "href": "/babbage", "ref": "e1", "zone": "content"}],
            "excerpt": "Augusta Ada King was an English mathematician.",
            "source": "readability",
            "readerable": true,
        })
    }

    fn sample_page() -> PageView {
        PageView {
            view: sample_view(),
            refs_registered: true,
            source: PAGE_SOURCE_JS_FALLBACK,
            ready: true,
            parse_ms: Some(9.5),
            readability_injected: true,
        }
    }

    /// `results.page` is the collected view verbatim: the realistic regression
    /// is someone folding `source`/`ready`/`refs_registered` into `page`
    /// because it is convenient, which would put provenance in the payload.
    #[test]
    fn insert_page_publishes_the_view_verbatim() {
        let page = sample_page();
        let collected = page.view.clone();

        let mut results = json!({"clicked": true});
        insert_page(&mut results, page);

        assert_eq!(
            results["page"], collected,
            "results.page must be the collected view verbatim"
        );
    }

    /// The provenance keys ride in `results.page_meta` and end up in `meta`,
    /// never inside `page`.
    #[test]
    fn lift_meta_moves_provenance_out_of_results() {
        let mut results = json!({"clicked": true});
        insert_page(&mut results, sample_page());
        let mut meta = json!({});
        let cli = test_cli(&["ff-rdp", "tabs"]);

        let text_section = lift_meta(&cli, &mut results, &mut meta);

        assert!(
            text_section.is_none(),
            "JSON mode keeps the page in results"
        );
        assert!(
            results.get("page_meta").is_none(),
            "page_meta must not survive into the printed results: {results}"
        );
        assert_eq!(meta["page_source"], json!(PAGE_SOURCE_JS_FALLBACK));
        assert_eq!(meta["page_ready"], json!(true));
        assert_eq!(meta["page_refs_registered"], json!(true));
        assert_eq!(meta["page_parse_ms"], json!(9.5));
        assert_eq!(meta["page_readability_injected"], json!(true));
        assert!(
            results.get("page").is_some(),
            "JSON mode keeps results.page"
        );
    }

    /// A view collected without the reader pass reports no timing at all —
    /// `page_parse_ms: null` would read as "it ran and measured nothing".
    #[test]
    fn lift_meta_omits_parse_ms_when_the_reader_did_not_run() {
        let mut results = json!({"clicked": true});
        insert_page(
            &mut results,
            PageView {
                parse_ms: None,
                readability_injected: false,
                ..sample_page()
            },
        );
        let mut meta = json!({});
        let cli = test_cli(&["ff-rdp", "tabs"]);
        lift_meta(&cli, &mut results, &mut meta);
        assert!(meta.get("page_parse_ms").is_none(), "{meta}");
        assert_eq!(meta["page_readability_injected"], json!(false));
    }

    /// `--format text` takes the page OUT of `results` (the generic renderer
    /// would pretty-print the whole envelope as JSON otherwise) and hands it
    /// back for [`render_text_section`].
    #[test]
    fn lift_meta_takes_the_page_out_in_text_mode() {
        let mut results = json!({"clicked": true});
        insert_page(&mut results, sample_page());
        let mut meta = json!({});
        let cli = test_cli(&["ff-rdp", "--format", "text", "tabs"]);

        let text_section = lift_meta(&cli, &mut results, &mut meta);

        assert!(
            results.get("page").is_none(),
            "text mode must remove results.page: {results}"
        );
        assert_eq!(
            text_section
                .as_ref()
                .map(|p| p["headings"][0]["text"].clone()),
            Some(json!("Ada Lovelace"))
        );
        assert_eq!(meta["page_source"], json!(PAGE_SOURCE_JS_FALLBACK));
    }

    /// `lift_meta` is a no-op when `--with-page` was not passed.
    #[test]
    fn lift_meta_without_with_page_changes_nothing() {
        let mut results = json!({"clicked": true});
        let mut meta = json!({"selector": "a"});
        let cli = test_cli(&["ff-rdp", "tabs"]);

        assert!(lift_meta(&cli, &mut results, &mut meta).is_none());
        assert_eq!(results, json!({"clicked": true}));
        assert_eq!(meta, json!({"selector": "a"}));
    }

    #[test]
    fn render_text_empty_sections_do_not_panic() {
        render_text(&json!({"landmarks": [], "headings": [], "interactive": []}));
    }
}
