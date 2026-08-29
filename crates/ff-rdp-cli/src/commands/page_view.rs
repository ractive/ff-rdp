//! The shared "page view" collector (iter-210 Theme A).
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
//! The view is deliberately the *summary* (headings, landmarks, interactive
//! elements), not the DOM tree `snapshot` returns: the benchmark's cost
//! advantage for ff-rdp came from small outputs, and a few hundred tokens per
//! action is what keeps that.

use ff_rdp_core::ActorId;
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;

use super::connect_tab::ConnectedTab;
use super::js_helpers::{
    UNIQUE_SELECTOR_JS_FN, acc_name_js_fn, eval_or_bail, poll_js_condition, resolve_result,
};

/// Default cap on `interactive` entries, matching `a11y summary`'s own
/// pre-iter-210 default.
///
/// `a11y summary` lets `--all` lift it and `--limit N` override it.
/// `--with-page` always uses it: an embedded page view rides along with every
/// click, and an uncapped one on a link-heavy page would undo the token
/// advantage the whole feature is meant to preserve.
pub(crate) const DEFAULT_INTERACTIVE_LIMIT: usize = 50;

/// JS that collects the page view.
///
/// `__UNIQUE_SELECTOR_FN__` is replaced with [`UNIQUE_SELECTOR_JS_FN`]; every
/// interactive entry carries a `__resolver` (a genuinely unique CSS selector)
/// which Rust strips after registering it with the daemon as a `--ref` handle.
/// Landmarks and headings get no resolver — they are not click targets, and a
/// resolver per heading is pure payload.
const PAGE_VIEW_JS_TEMPLATE: &str = r#"(function() {
  __UNIQUE_SELECTOR_FN__
  __ACC_NAME_FN__
  var result = {landmarks: [], headings: [], interactive: []};

  // Landmarks
  var landmarkRoles = ['banner','navigation','main','contentinfo','complementary','search','form'];
  var landmarkTags = {HEADER:'banner',NAV:'navigation',MAIN:'main',FOOTER:'contentinfo',ASIDE:'complementary'};

  // Check role attributes
  landmarkRoles.forEach(function(role) {
    var els = document.querySelectorAll('[role="' + role + '"]');
    for (var i = 0; i < els.length; i++) {
      var label = els[i].getAttribute('aria-label') || '';
      result.landmarks.push({role: role, label: label, tag: els[i].tagName.toLowerCase()});
    }
  });
  // Check semantic HTML (only if no explicit role already captured)
  Object.keys(landmarkTags).forEach(function(tag) {
    var els = document.getElementsByTagName(tag);
    for (var i = 0; i < els.length; i++) {
      if (!els[i].getAttribute('role')) {
        var label = els[i].getAttribute('aria-label') || '';
        result.landmarks.push({role: landmarkTags[tag], label: label, tag: tag.toLowerCase()});
      }
    }
  });

  // Headings
  for (var level = 1; level <= 6; level++) {
    var headings = document.querySelectorAll('h' + level);
    for (var j = 0; j < headings.length; j++) {
      var text = __ffrdpAccName(headings[j]);
      result.headings.push({level: level, text: text});
    }
  }

  // Interactive: links
  var links = document.querySelectorAll('a[href]');
  for (var k = 0; k < links.length; k++) {
    var linkText = __ffrdpAccName(links[k]);
    result.interactive.push({role: 'link', name: linkText, href: links[k].getAttribute('href'),
      __resolver: __ffrdpUniqueSelector(links[k])});
  }

  // Interactive: buttons
  var buttons = document.querySelectorAll('button, [role="button"], input[type="button"], input[type="submit"]');
  for (var m = 0; m < buttons.length; m++) {
    var btnText = __ffrdpAccName(buttons[m]);
    result.interactive.push({role: 'button', name: btnText,
      __resolver: __ffrdpUniqueSelector(buttons[m])});
  }

  // Interactive: inputs (text, email, password, etc.)
  var inputs = document.querySelectorAll('input:not([type="button"]):not([type="submit"]):not([type="hidden"]), textarea, select');
  for (var n = 0; n < inputs.length; n++) {
    var inp = inputs[n];
    var inputName = __ffrdpAccName(inp) || inp.getAttribute('name') || '';
    var inputType = inp.getAttribute('type') || inp.tagName.toLowerCase();
    result.interactive.push({role: 'input', name: inputName, type: inputType,
      __resolver: __ffrdpUniqueSelector(inp)});
  }

  return '__FF_RDP_JSON__' + JSON.stringify(result);
})()"#;

/// Build the page-view JS with the unique-selector helper spliced in.
pub(crate) fn build_page_view_js() -> String {
    // iter-211 Theme C: names come from the shared `__ffrdpAccName` helper —
    // the same one `dom` uses — rather than four hand-rolled
    // `textContent.trim().slice(0, 100)` variants that disagreed with each
    // other and cut real titles mid-word.
    PAGE_VIEW_JS_TEMPLATE
        .replace("__UNIQUE_SELECTOR_FN__", UNIQUE_SELECTOR_JS_FN)
        .replace("__ACC_NAME_FN__", &acc_name_js_fn())
}

/// The value `meta.page_source` (and `a11y summary`'s `meta.source`) carries.
///
/// Borrowed from `a11y`'s vocabulary so the two accessibility surfaces read the
/// same way. `js-fallback` is literally true of this collector: the summary is
/// assembled by in-page JavaScript, not by Firefox's accessibility actor —
/// which is exactly what the word means on `a11y`. There is deliberately no
/// second constant here: nothing in this iteration produces a native-backed
/// page view, and a `native` variant nothing can emit would be a lie with a
/// doc comment attached.
pub(crate) const PAGE_SOURCE_JS_FALLBACK: &str = "js-fallback";

/// A collected page view.
pub struct PageView {
    /// `{landmarks, headings, interactive}` — the same key set `a11y summary`
    /// puts in `results`, plus the `interactive_total`/`interactive_truncated`
    /// pair when the cap bit.
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
}

/// Inputs to [`collect`].
pub struct CollectOptions {
    /// Keep at most this many `interactive` entries. `None` keeps all.
    pub interactive_limit: Option<usize>,
    /// Wait up to this many ms for `document.readyState == "complete"` before
    /// evaluating. `None` collects immediately.
    pub wait_complete_ms: Option<u64>,
}

/// Collect the page view from the connected tab.
///
/// Ordering matters and is documented in every `--with-page` `--help`: the
/// readiness wait runs *first*, so the view describes the document the action
/// produced rather than the one it left. Ref registration runs last, against
/// the already-capped entry list, so exactly the handles a caller can see are
/// the handles the daemon holds.
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

    // 2. Collect.
    let js = build_page_view_js();
    let eval_result = eval_or_bail(ctx, console_actor, &js, "page view collection failed")?;
    let mut view = resolve_result(ctx, &eval_result.result)?;

    // 3. Cap `interactive` before refs are allocated, so a ref is minted for
    //    exactly the entries the caller receives.
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
    })
}

/// Truncate `view.interactive` to `limit`, recording `interactive_total` and
/// `interactive_truncated` when anything was cut.
pub(crate) fn apply_interactive_limit(view: &mut Value, limit: Option<usize>) {
    let Some(limit) = limit else { return };
    let Some(Value::Array(arr)) = view.get_mut("interactive") else {
        return;
    };
    let total = arr.len();
    if total <= limit {
        return;
    }
    arr.truncate(limit);
    if let Some(obj) = view.as_object_mut() {
        obj.insert("interactive_total".to_owned(), json!(total));
        obj.insert("interactive_truncated".to_owned(), json!(true));
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
/// - `results.page` — the view itself, the same key set `a11y summary` puts in
///   its `results`.
/// - `results.page_meta` — `{source, ready, refs_registered}`, which
///   [`lift_meta`] moves into the envelope's `meta` before printing. It rides
///   in `results` only because the commands that collect the page
///   (`navigate`/`click`/`type`) build their envelope in a *different*
///   function from the one holding the connection — the same reason
///   `settle_method` already travels this way.
pub(crate) fn attach(
    ctx: &mut ConnectedTab,
    results: &mut Value,
    wait_complete_ms: Option<u64>,
) -> Result<(), AppError> {
    // An action that navigated (a `click` on a link, `type --submit`) left the
    // console actor cached in `ctx.target` bound to the *previous* docshell, so
    // every eval below would come back `noSuchActor` — or, worse, describe the
    // page the action left. Refresh first: this is precisely what lets
    // `click --ref <link> --with-page` return the destination page.
    ctx.refresh_target();
    let console_actor = ctx.target.console_actor.clone();
    let page = collect(
        ctx,
        &console_actor,
        &CollectOptions {
            interactive_limit: Some(DEFAULT_INTERACTIVE_LIMIT),
            wait_complete_ms,
        },
    )?;
    insert_page(results, page);
    Ok(())
}

/// Write `page` and `page_meta` into `results`.
///
/// Split out of [`attach`] so the shape contract can be tested without a live
/// Firefox: the `page` object must be the collected view **verbatim**. Nothing
/// describing how the view was produced may be added to it — that all belongs
/// in `page_meta`, which [`lift_meta`] moves into the envelope's `meta`. This
/// is what keeps `results.page` and `a11y summary`'s `results` the same shape,
/// so an agent can learn one key set and use it on either.
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
        }),
    );
}

/// Move [`attach`]'s `results.page_meta` into the envelope's `meta` as
/// `page_source` / `page_ready` / `page_refs_registered`, and — in
/// `--format text` without `--jq` — take `results.page` out so the caller can
/// print it with [`render_text_section`] beneath its own line.
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
        ] {
            if let Some(v) = page_meta.get(from) {
                obj.insert(to.to_owned(), v.clone());
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
            match role {
                "link" => {
                    let href = el.get("href").and_then(Value::as_str).unwrap_or("");
                    println!("{prefix}link \"{name}\" -> {href}");
                }
                "button" => {
                    println!("{prefix}button \"{name}\"");
                }
                "input" => {
                    let itype = el.get("type").and_then(Value::as_str).unwrap_or("text");
                    println!("{prefix}input[{itype}] \"{name}\"");
                }
                _ => {
                    println!("{prefix}{role} \"{name}\"");
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
            println!(
                "  ... and {} more (use --all for complete list)",
                total.saturating_sub(interactive.len())
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::js_helpers::JSON_SENTINEL;
    use clap::Parser as _;

    /// Parse a `Cli` from argv for the flag-reading helpers under test.
    fn test_cli(argv: &[&str]) -> Cli {
        Cli::parse_from(argv)
    }

    #[test]
    fn page_view_js_has_sentinel_and_stringify() {
        let js = build_page_view_js();
        assert!(
            js.contains(JSON_SENTINEL),
            "JS must use the sentinel prefix"
        );
        assert!(js.contains("JSON.stringify"), "JS must use JSON.stringify");
    }

    #[test]
    fn page_view_js_collects_all_sections() {
        let js = build_page_view_js();
        for section in ["landmarks", "headings", "interactive"] {
            assert!(js.contains(section), "JS must collect {section}");
        }
    }

    /// Theme B: the collector must hand back a resolver per interactive
    /// element — without it there is nothing to register as a `--ref`.
    #[test]
    fn page_view_js_emits_resolver_per_interactive_entry() {
        let js = build_page_view_js();
        assert!(
            js.contains("function __ffrdpUniqueSelector"),
            "unique-selector helper must be spliced in"
        );
        // links, buttons, inputs — three call sites.
        assert_eq!(
            js.matches("__resolver: __ffrdpUniqueSelector(").count(),
            3,
            "every interactive category must carry a resolver:\n{js}"
        );
        assert!(
            !js.contains("__UNIQUE_SELECTOR_FN__"),
            "placeholder must be substituted"
        );
    }

    #[test]
    fn interactive_limit_truncates_and_reports_total() {
        let mut view = json!({
            "landmarks": [], "headings": [],
            "interactive": (0..5).map(|i| json!({"role": "link", "name": i.to_string()}))
                .collect::<Vec<_>>()
        });
        apply_interactive_limit(&mut view, Some(2));
        assert_eq!(view["interactive"].as_array().map(Vec::len), Some(2));
        assert_eq!(view["interactive_total"], json!(5));
        assert_eq!(view["interactive_truncated"], json!(true));
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
            "interactive": [
                {"role": "link", "name": "Home", "href": "/", "ref": "e1"},
                {"role": "button", "name": "Submit"},
                {"role": "input", "name": "Email", "type": "email"},
                {"role": "unknown", "name": "Widget"}
            ],
            "interactive_total": 10,
            "interactive_truncated": true
        });
        // Exercises every branch; the assertion is that it does not panic.
        render_text(&results);
        render_text_section(Some(&results));
        render_text_section(None);
    }

    /// A view shaped like what `collect` returns on the fixture pages.
    fn sample_view() -> Value {
        json!({
            "landmarks": [{"role": "main", "tag": "main", "label": ""}],
            "headings": [{"level": 1, "text": "Ada Lovelace"}],
            "interactive": [{"role": "link", "name": "Charles Babbage",
                             "href": "/babbage", "ref": "e1"}],
        })
    }

    fn sample_page() -> PageView {
        PageView {
            view: sample_view(),
            refs_registered: true,
            source: PAGE_SOURCE_JS_FALLBACK,
            ready: true,
        }
    }

    /// AC `with_page_shape_matches_a11y_summary`.
    ///
    /// `a11y summary` publishes `PageView::view` as its `results`; `--with-page`
    /// publishes the same value as `results.page`. The test that matters is
    /// therefore not "do two collectors agree" (there is only one) but "does
    /// the embedding path leave the view alone" — the realistic regression is
    /// someone folding `source`/`ready`/`refs_registered` into `page` because
    /// it is convenient, which would give the two surfaces different key sets.
    #[test]
    fn with_page_shape_matches_a11y_summary() {
        let page = sample_page();
        let a11y_summary_results = page.view.clone();

        let mut results = json!({"clicked": true});
        insert_page(&mut results, page);

        assert_eq!(
            results["page"], a11y_summary_results,
            "results.page must be the collected view verbatim"
        );
        let embedded: Vec<&String> = results["page"]
            .as_object()
            .expect("page must be an object")
            .keys()
            .collect();
        let standalone: Vec<&String> = a11y_summary_results
            .as_object()
            .expect("a11y summary results must be an object")
            .keys()
            .collect();
        assert_eq!(
            embedded, standalone,
            "the two surfaces must serialise to the same key set"
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
        assert!(
            results.get("page").is_some(),
            "JSON mode keeps results.page"
        );
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
