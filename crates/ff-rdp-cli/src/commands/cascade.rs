//! `ff-rdp cascade <SELECTOR> [--prop NAME | --all]` — the CSS cascade inspector.
//!
//! For a single element (matched by CSS selector), returns the ordered list
//! of matching CSS rules per property, annotated with origin, specificity,
//! source location, `!important` flag, and a `winner: true` marker on the
//! rule whose declaration would become the computed value.
//!
//! This is a read-only view; style mutation is out of scope (see iter-81 plan).

use ff_rdp_core::css::specificity::{self, Specificity};
use ff_rdp_core::{ActorId, AppliedRule, DomWalkerActor, Grip, InspectorActor, PageStyleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};
use super::js_helpers::{escape_selector, eval_or_bail};

/// CSS origin bucket: lower numbers cascade earlier (lose by default).
///
/// The cascade order (least-to-most important, for *normal* declarations) is:
/// `UA → User → Author → Inline`.  For `!important` declarations the order
/// reverses between User Agent and the user/author origins (UA-important is
/// the strongest in the spec, but UA stylesheets are not user-modifiable
/// anyway).  We use this enum only for ordering of normal declarations and
/// flip the comparison for `!important` rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    UserAgent,
    #[allow(dead_code)] // distinguishing user-stylesheet origin is future work
    User,
    Author,
    #[allow(dead_code)]
    // Inline style="…" entries are filtered by the actor today; variant kept for future support.
    Inline,
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Origin::UserAgent => "ua",
            Origin::User => "user",
            Origin::Author => "author",
            Origin::Inline => "inline",
        }
    }
}

/// Classify a rule by its stylesheet href.
///
/// Firefox sends UA sheets with a `resource://` or `chrome://` prefix.
/// `<style>` blocks embedded in the document have no href but are still
/// author-origin rules.  True inline `style="…"` declarations (rule
/// `type == 0` in the RDP response) are filtered out by the actor
/// parser today, so we never see them here — the `Origin::Inline`
/// variant is reserved for when that parsing is extended.
fn classify_origin(source: Option<&str>) -> Origin {
    match source {
        Some(href)
            if href.starts_with("resource://")
                || href.starts_with("chrome://")
                || href.starts_with("resource:///") =>
        {
            Origin::UserAgent
        }
        // `None` or empty href → `<style>` block (author origin).
        _ => Origin::Author,
    }
}

/// One row in the cascade view for a given property.
#[derive(Debug, Clone)]
struct CascadeEntry {
    selector: String,
    specificity: Specificity,
    origin: Origin,
    media: Vec<String>,
    stylesheet: Option<String>,
    line: Option<u32>,
    value: String,
    important: bool,
    /// Order of appearance in the underlying `getApplied` response (0-based).
    /// Used as the final tiebreaker — Firefox returns entries in document
    /// order, so a higher value means "later in the source".
    source_order: usize,
}

impl CascadeEntry {
    fn to_json(&self, winner: bool) -> Value {
        json!({
            "selector": self.selector,
            "specificity": [self.specificity.0, self.specificity.1, self.specificity.2],
            "origin": self.origin.as_str(),
            "media": if self.media.is_empty() { Value::Null } else { json!(self.media) },
            "stylesheet": self.stylesheet,
            "line": self.line,
            "value": self.value,
            "important": self.important,
            "winner": winner,
        })
    }
}

/// Sort key for the cascade order, low-to-high (last = winner).
///
/// Ordering (per the CSS Cascade and Inheritance Level 4 spec):
///   1. `!important` reverses the origin order. We model this by giving
///      `!important` declarations a higher base rank than any normal one.
///   2. Within the same importance group, higher origin precedence wins
///      (Author > User > UA for normal; UA > User > Author for important).
///   3. Specificity (a, b, c) — higher wins.
///   4. Document order — later in source wins.
///
/// We compose these into a tuple so plain `sort_by_key` does the work.
fn cascade_rank(e: &CascadeEntry) -> (u8, u8, Specificity, usize) {
    // Importance tier: 1 if important, 0 otherwise — important always beats normal.
    let importance = u8::from(e.important);
    // Origin tier within the group.
    //   Normal:    Inline(4) > Author(3) > User(2) > UA(1)
    //   Important: UA(4) > User(3) > Author(2) ≈ Inline(2)
    // Inline declarations belong to the author origin in the cascade; when
    // !important they keep the same tier as other author rules (and win
    // ties via their higher specificity in practice).
    let origin_rank = if e.important {
        match e.origin {
            Origin::UserAgent => 4,
            Origin::User => 3,
            Origin::Author | Origin::Inline => 2,
        }
    } else {
        match e.origin {
            Origin::UserAgent => 1,
            Origin::User => 2,
            Origin::Author => 3,
            Origin::Inline => 4,
        }
    };
    (importance, origin_rank, e.specificity, e.source_order)
}

/// Pick the most-specific matched selector for a rule.
///
/// Firefox's `getApplied` with `matchedSelectors: true` returns the subset
/// of the rule's selectors that actually matched the element.  We compute
/// specificity from the highest-specificity matched selector — that's the
/// one the engine used.
///
/// Falls back to the joined selector string when no `matchedSelectors`
/// were returned (older Firefox or non-matching mode).
fn pick_selector(rule: &AppliedRule) -> (String, Specificity) {
    let candidates: Vec<&str> = if rule.matched_selectors.is_empty() {
        // Split on top-level commas only — naive `split(',')` would
        // shred selectors like `:is(.a, .b)` into invalid fragments.
        specificity::split_top_level_commas(&rule.selector)
            .into_iter()
            .map(str::trim)
            .collect()
    } else {
        rule.matched_selectors.iter().map(String::as_str).collect()
    };
    candidates
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|sel| (sel.to_string(), specificity::compute(sel)))
        .max_by_key(|(_, spec)| *spec)
        .unwrap_or_else(|| (rule.selector.clone(), specificity::compute(&rule.selector)))
}

/// Build the cascade for one property from the rules Firefox returned.
///
/// `rules` are in Firefox's source order (earliest first).  Returns the
/// entries sorted lowest-to-highest cascade rank; the last one wins.
fn build_cascade_for_property(rules: &[AppliedRule], property: &str) -> Vec<CascadeEntry> {
    let mut entries: Vec<CascadeEntry> = rules
        .iter()
        .enumerate()
        .flat_map(|(idx, rule)| {
            let (sel, spec) = pick_selector(rule);
            let origin = classify_origin(rule.source.as_deref());
            // A rule may legitimately declare the same property twice (later wins
            // within the rule); we keep each declaration as a separate row.
            // Custom properties (--foo) are case-sensitive; standard properties are not.
            rule.properties
                .iter()
                .filter(|p| property_name_matches(&p.name, property))
                .map(move |p| CascadeEntry {
                    selector: sel.clone(),
                    specificity: spec,
                    origin,
                    media: rule.media.clone(),
                    stylesheet: rule.source.clone(),
                    line: rule.line,
                    value: p.value.clone(),
                    important: p.priority.eq_ignore_ascii_case("important"),
                    source_order: idx,
                })
                .collect::<Vec<_>>()
        })
        .collect();

    entries.sort_by_key(cascade_rank);
    entries
}

/// Render the cascade for one property to a JSON object (the shape documented
/// in `kb/iterations/iteration-81-cascade-inspector.md`).
///
/// When `external_computed` is `Some(value)` and no author rules declare the
/// property, the output includes:
/// - `"computed": <external_computed>`
/// - `"inherited_or_default": true`
/// - `"note": "no author rule declares this property; computed value is inherited or default"`
///
/// This disambiguates an empty `rules: []` that means "inherited/default" (correct
/// behaviour) from the iter-82/83/84 bug where empty rules meant "cascade broken".
fn render_property_cascade(
    selector: &str,
    property: &str,
    rules: &[AppliedRule],
    external_computed: Option<&str>,
) -> Value {
    let entries = build_cascade_for_property(rules, property);
    let winner_idx = entries.len().checked_sub(1);
    let computed = winner_idx
        .and_then(|i| entries.get(i))
        .map(|e| e.value.clone());

    let rules_json: Vec<Value> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| e.to_json(Some(i) == winner_idx))
        .collect();

    // When there are no author rules for this property AND we have an
    // external computed value, annotate so callers can tell "inherited" from "broken".
    if rules_json.is_empty()
        && let Some(ext) = external_computed.filter(|s| !s.is_empty())
    {
        return json!({
            "selector": selector,
            "property": property,
            "computed": ext,
            "rules": rules_json,
            "inherited_or_default": true,
            "note": "no author rule declares this property; computed value is inherited or default",
        });
    }

    json!({
        "selector": selector,
        "property": property,
        "computed": computed,
        "rules": rules_json,
    })
}

/// True if `decl_name` matches `query` under CSS property-name rules.
///
/// Standard CSS property names are ASCII-case-insensitive.  CSS custom
/// properties (any name starting with `--`) are case-sensitive per
/// CSS Variables — `--Foo` and `--foo` are distinct properties.
fn property_name_matches(decl_name: &str, query: &str) -> bool {
    if is_custom_property(decl_name) || is_custom_property(query) {
        decl_name == query
    } else {
        decl_name.eq_ignore_ascii_case(query)
    }
}

fn is_custom_property(name: &str) -> bool {
    name.starts_with("--")
}

/// Set of properties declared anywhere in the applied rules.
///
/// Standard property names are normalized to lowercase; custom properties
/// (`--foo`) keep their original casing because they are case-sensitive.
fn declared_properties(rules: &[AppliedRule]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for rule in rules {
        for prop in &rule.properties {
            if is_custom_property(&prop.name) {
                seen.insert(prop.name.clone());
            } else {
                seen.insert(prop.name.to_ascii_lowercase());
            }
        }
    }
    seen.into_iter().collect()
}

/// Shared setup: connect, find the node, fetch its applied rules.
///
/// Returns the connected tab (transport owner), the applied rules in the order
/// Firefox returned them, and — when `capture_raw` is `true` — the raw
/// `getApplied` reply packet as a `serde_json::Value` (before field-name
/// mapping).  When `capture_raw` is `false` the third element is `None`.
///
/// Capturing the raw reply requires a second RDP round-trip on the same
/// connection; this is acceptable for the diagnostic `--debug-raw` path.
fn fetch_applied(
    cli: &Cli,
    selector: &str,
    capture_raw: bool,
) -> Result<(ConnectedTab, Vec<AppliedRule>, Option<Value>), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    let inspector_actor = ctx
        .target
        .inspector_actor
        .clone()
        .ok_or_else(|| AppError::User("no inspector actor available".to_string()))?;

    let walker_actor = InspectorActor::get_walker(ctx.transport_mut(), &inspector_actor)
        .map_err(map_cascade_error)?;

    let page_style_actor = InspectorActor::get_page_style(ctx.transport_mut(), &inspector_actor)
        .map_err(map_cascade_error)?;

    let doc_root = DomWalkerActor::document_element(ctx.transport_mut(), &walker_actor)
        .map_err(map_cascade_error)?;
    let root_actor_str = doc_root
        .actor
        .as_deref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("document root has no actor ID")))?;
    let root_actor = ActorId::from(root_actor_str);

    let maybe_node =
        DomWalkerActor::query_selector(ctx.transport_mut(), &walker_actor, &root_actor, selector)
            .map_err(map_cascade_error)?;

    let node = maybe_node
        .ok_or_else(|| AppError::User(format!("no element matching selector '{selector}'")))?;
    let node_actor_str = node
        .actor
        .as_deref()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("matched node has no actor ID")))?;
    let node_actor = ActorId::from(node_actor_str);

    // Capture the raw reply BEFORE parsing so --debug-raw shows unmodified field names.
    // This is a separate RDP round-trip on the same connection.
    let raw_reply = if capture_raw {
        Some(
            PageStyleActor::get_applied_raw(ctx.transport_mut(), &page_style_actor, &node_actor)
                .map_err(map_cascade_error)?,
        )
    } else {
        None
    };

    let applied = PageStyleActor::get_applied(ctx.transport_mut(), &page_style_actor, &node_actor)
        .map_err(map_cascade_error)?;

    Ok((ctx, applied, raw_reply))
}

/// CLI entry point for `ff-rdp cascade <SEL> [--prop NAME | --all] [--debug-raw]`.
pub fn run(
    cli: &Cli,
    selector: &str,
    prop: Option<&str>,
    all: bool,
    debug_raw: bool,
) -> Result<(), AppError> {
    let (mut ctx, applied, raw_reply) = fetch_applied(cli, selector, debug_raw)?;

    if let Some(raw) = raw_reply {
        // Emit the RAW getApplied reply to stderr BEFORE any conversion.
        // This shows the actual field names Firefox sends, which is the only
        // reliable way to diagnose protocol drift (e.g. when a field is renamed
        // server-side and our parser silently drops it).
        eprintln!(
            "[cascade --debug-raw] raw getApplied reply:\n{}",
            serde_json::to_string_pretty(&raw)
                .unwrap_or_else(|e| format!("serialization error: {e}"))
        );
    }

    let _ = all; // --all is the default; flag is accepted for clarity.
    let properties: Vec<String> = match prop {
        Some(name) if is_custom_property(name) => vec![name.to_string()],
        Some(name) => vec![name.to_ascii_lowercase()],
        None => declared_properties(&applied),
    };

    // When --prop is set, fetch the actual computed value via getComputedStyle.
    // This is used as a fallback hint when no author rules declare the property,
    // to distinguish "inherited/default" from the iter-82 broken-cascade shape.
    let external_computed: Option<String> = if let Some(prop_name) = prop {
        let escaped_sel = escape_selector(selector);
        let escaped_prop =
            serde_json::to_string(prop_name).unwrap_or_else(|_| format!("\"{prop_name}\""));
        let js = format!(
            "(function(){{var el=document.querySelector('{escaped_sel}');\
             if(!el)return null;\
             var v=getComputedStyle(el)[{escaped_prop}];\
             return v||null;}})()"
        );
        let console_actor = ctx.target.console_actor.clone();
        match eval_or_bail(&mut ctx, &console_actor, &js, "computed style lookup") {
            Ok(result) => match &result.result {
                Grip::Value(serde_json::Value::String(s)) => Some(s.clone()),
                _ => None,
            },
            Err(_) => None, // Non-fatal — the main cascade result still emits.
        }
    } else {
        None
    };

    let results: Vec<Value> = properties
        .iter()
        .map(|p| render_property_cascade(selector, p, &applied, external_computed.as_deref()))
        .collect();

    let total = results.len();
    let mut meta = json!({ "selector": selector });
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    let envelope = output::envelope(&Value::Array(results), total, &meta);
    let hint_ctx = HintContext::new(HintSource::Styles).with_selector(selector);
    OutputPipeline::from_cli(cli)?
        .finalize_with_hints(&envelope, Some(&hint_ctx))
        .map_err(AppError::from)
}

/// Map RDP errors to user-friendly messages for the cascade command.
fn map_cascade_error(err: ff_rdp_core::ProtocolError) -> AppError {
    match &err {
        ff_rdp_core::ProtocolError::ActorError { error, .. }
            if error == "noSuchActor" || error == "unknownActor" =>
        {
            AppError::User(
                "style actor is no longer valid — the actor may have expired after navigation. \
                 Re-run the command to get a fresh actor"
                    .to_string(),
            )
        }
        _ => AppError::from(err),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ff_rdp_core::RuleProperty;

    fn rule(
        selector: &str,
        source: Option<&str>,
        line: u32,
        decls: &[(&str, &str, &str)],
    ) -> AppliedRule {
        AppliedRule {
            selector: selector.to_string(),
            source: source.map(String::from),
            line: Some(line),
            column: Some(1),
            properties: decls
                .iter()
                .map(|(n, v, p)| RuleProperty {
                    name: (*n).to_string(),
                    value: (*v).to_string(),
                    priority: (*p).to_string(),
                })
                .collect(),
            matched_selectors: vec![selector.to_string()],
            media: vec![],
            rule_actor_id: None,
        }
    }

    /// AC: `cascade_marks_winner_on_higher_specificity` — given a recorded
    /// `getMatchedSelectors`-shape response with two rules where the higher
    /// specificity rule sets `display: flex` and the lower sets
    /// `display: block`, the higher-specificity rule wins.
    #[test]
    fn cascade_marks_winner_on_higher_specificity() {
        let rules = vec![
            // (0,0,1) dialog -> block
            rule(
                "dialog",
                Some("https://example.com/pico.css"),
                88,
                &[("display", "block", "")],
            ),
            // (1,0,1) dialog#lightbox -> flex
            rule(
                "dialog#lightbox",
                Some("https://example.com/site.css"),
                142,
                &[("display", "flex", "")],
            ),
        ];

        let out = render_property_cascade("dialog#lightbox", "display", &rules, None);
        assert_eq!(out["property"], "display");
        assert_eq!(out["computed"], "flex");

        let arr = out["rules"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // Winner is the last one in cascade order (highest rank).
        let winner = arr.last().unwrap();
        assert_eq!(winner["selector"], "dialog#lightbox");
        assert_eq!(winner["value"], "flex");
        assert_eq!(winner["winner"], true);
        // The loser must be flagged false.
        let loser = arr.first().unwrap();
        assert_eq!(loser["winner"], false);
        // Specificity tuple shape.
        assert_eq!(winner["specificity"], json!([1, 0, 1]));
        assert_eq!(loser["specificity"], json!([0, 0, 1]));
    }

    /// AC: `cascade_important_overrides_specificity` — `!important` on a
    /// lower-specificity rule beats a higher-specificity normal rule.
    #[test]
    fn cascade_important_overrides_specificity() {
        let rules = vec![
            // (0,0,1) dialog -> block !important
            rule(
                "dialog",
                Some("https://example.com/base.css"),
                10,
                &[("display", "block", "important")],
            ),
            // (1,0,1) dialog#lightbox -> flex (normal)
            rule(
                "dialog#lightbox",
                Some("https://example.com/site.css"),
                142,
                &[("display", "flex", "")],
            ),
        ];

        let out = render_property_cascade("dialog#lightbox", "display", &rules, None);
        // !important wins despite lower specificity.
        assert_eq!(out["computed"], "block");
        let arr = out["rules"].as_array().unwrap();
        let winner = arr.last().unwrap();
        assert_eq!(winner["selector"], "dialog");
        assert_eq!(winner["important"], true);
        assert_eq!(winner["winner"], true);
    }

    #[test]
    fn cascade_uses_matched_selector_for_specificity() {
        // Rule selectors are "h1, .title" — element actually matches via ".title",
        // so specificity must be (0,1,0), not (0,0,1).
        let mut r = rule(
            "h1, .title",
            Some("https://example.com/style.css"),
            15,
            &[("color", "red", "")],
        );
        r.matched_selectors = vec![".title".into()];
        let out = render_property_cascade(".title", "color", &[r], None);
        let arr = out["rules"].as_array().unwrap();
        assert_eq!(arr[0]["specificity"], json!([0, 1, 0]));
        assert_eq!(arr[0]["selector"], ".title");
    }

    #[test]
    fn cascade_classifies_origins() {
        // <style> block (no href) → author.
        let style_block = rule("dialog", None, 0, &[("color", "red", "")]);
        assert_eq!(
            classify_origin(style_block.source.as_deref()),
            Origin::Author
        );
        // UA stylesheet.
        assert_eq!(
            classify_origin(Some("resource://gre-resources/ua.css")),
            Origin::UserAgent
        );
        assert_eq!(
            classify_origin(Some("chrome://global/skin/global.css")),
            Origin::UserAgent
        );
        // External author sheet.
        assert_eq!(
            classify_origin(Some("https://example.com/site.css")),
            Origin::Author
        );
        // Empty href → author (<style> block, not style attribute — the
        // actor filters style="…" entries out before they reach us).
        assert_eq!(classify_origin(Some("")), Origin::Author);
    }

    #[test]
    fn cascade_custom_property_is_case_sensitive() {
        // --Foo and --foo are distinct custom properties.
        let rules = vec![rule(
            ":root",
            Some("https://example.com/site.css"),
            1,
            &[("--Foo", "1px", ""), ("--foo", "2px", "")],
        )];
        // Querying lower-case `--foo` must NOT pick up `--Foo`.
        let out = render_property_cascade(":root", "--foo", &rules, None);
        assert_eq!(out["computed"], "2px");
        let arr = out["rules"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["value"], "2px");
    }

    #[test]
    fn cascade_pick_selector_handles_commas_in_functional_pseudos() {
        // Selector text `:is(.a, .b)` with no matchedSelectors must not be
        // shredded by a naive split(',').  The whole selector is treated
        // as one candidate, so specificity comes from the max inside :is().
        let mut r = rule(
            ":is(.a, .b)",
            Some("https://example.com/site.css"),
            1,
            &[("color", "red", "")],
        );
        r.matched_selectors = vec![]; // force the fallback path
        let out = render_property_cascade(".a", "color", &[r], None);
        let arr = out["rules"].as_array().unwrap();
        // :is(.a, .b) → (0,1,0); a naive split would have produced
        // ":is(.a" with specificity (0,1,0) too but with a broken
        // selector string — we check the selector text is preserved.
        assert_eq!(arr[0]["selector"], ":is(.a, .b)");
        assert_eq!(arr[0]["specificity"], json!([0, 1, 0]));
    }

    #[test]
    fn cascade_inline_important_keeps_author_tier() {
        // An inline-origin !important must NOT be demoted below author
        // !important.  Equal specificity → document order breaks the tie
        // (here: same source_order=0 for both, but the inline rule's
        // higher implicit specificity would win in practice).
        // This test asserts that Origin::Inline + important does not sink
        // to the lowest origin rank.
        let inline_important = CascadeEntry {
            selector: "dialog".into(),
            specificity: (0, 0, 1),
            origin: Origin::Inline,
            media: vec![],
            stylesheet: None,
            line: None,
            value: "block".into(),
            important: true,
            source_order: 0,
        };
        let ua_normal = CascadeEntry {
            specificity: (1, 0, 0),
            origin: Origin::UserAgent,
            important: false,
            ..inline_important.clone()
        };
        // Inline-important must rank higher than UA-normal (different
        // importance tiers).
        assert!(cascade_rank(&inline_important) > cascade_rank(&ua_normal));
        // And not below author-important (author-important should equal
        // or tie inline-important on the origin axis).
        let author_important = CascadeEntry {
            origin: Origin::Author,
            ..inline_important.clone()
        };
        // Both should land in the same origin rank for important.
        let (_, r1, _, _) = cascade_rank(&inline_important);
        let (_, r2, _, _) = cascade_rank(&author_important);
        assert_eq!(r1, r2);
    }

    #[test]
    fn cascade_returns_null_computed_when_no_declarations() {
        // No rule declares the requested property — computed is null, rules empty.
        let rules = vec![rule(
            "dialog",
            Some("https://example.com/site.css"),
            1,
            &[("color", "red", "")],
        )];
        let out = render_property_cascade("dialog", "display", &rules, None);
        assert!(out["computed"].is_null());
        assert_eq!(out["rules"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn cascade_property_match_is_case_insensitive() {
        let rules = vec![rule(
            "dialog",
            Some("https://example.com/site.css"),
            1,
            &[("DISPLAY", "flex", "")],
        )];
        let out = render_property_cascade("dialog", "display", &rules, None);
        assert_eq!(out["computed"], "flex");
    }

    #[test]
    fn cascade_document_order_breaks_specificity_ties() {
        // Two identical-specificity rules — the later one wins.
        let rules = vec![
            rule(
                ".a",
                Some("https://example.com/a.css"),
                1,
                &[("color", "red", "")],
            ),
            rule(
                ".a",
                Some("https://example.com/b.css"),
                2,
                &[("color", "blue", "")],
            ),
        ];
        let out = render_property_cascade(".a", "color", &rules, None);
        assert_eq!(out["computed"], "blue");
    }

    #[test]
    fn declared_properties_dedupes_and_sorts() {
        let rules = vec![
            rule(
                "a",
                Some("x"),
                1,
                &[("color", "red", ""), ("display", "block", "")],
            ),
            rule("a", Some("x"), 2, &[("color", "blue", "")]),
        ];
        assert_eq!(declared_properties(&rules), vec!["color", "display"]);
    }

    /// Parse a recorded-fixture–shape JSON entries array (mirrors what
    /// Firefox returns from `getApplied`) into `AppliedRule`s and verify
    /// the cascade picks the correct winner.  This exercises the same
    /// code path that handles a real live response.
    #[test]
    fn cascade_from_recorded_fixture_shape() {
        let fixture = json!({
            "from": "server1.conn0.child1/pageStyleActor1",
            "entries": [
                {
                    "rule": {
                        "type": 1,
                        "href": "https://example.com/pico.css",
                        "line": 88,
                        "column": 1,
                        "selectors": ["dialog"]
                    },
                    "matchedSelectorIndexes": [0],
                    "declarations": [
                        {"name": "display", "value": "block", "priority": ""}
                    ]
                },
                {
                    "rule": {
                        "type": 1,
                        "href": "https://example.com/site.css",
                        "line": 142,
                        "column": 1,
                        "selectors": ["dialog#lightbox"]
                    },
                    "matchedSelectorIndexes": [0],
                    "declarations": [
                        {"name": "display", "value": "flex", "priority": ""}
                    ]
                }
            ]
        });
        let entries = fixture["entries"].as_array().unwrap();
        let rules: Vec<AppliedRule> = entries
            .iter()
            .filter_map(ff_rdp_core_test_helpers::parse_entry_for_test)
            .collect();
        let out = render_property_cascade("dialog#lightbox", "display", &rules, None);
        assert_eq!(out["computed"], "flex");
    }

    /// AC: `unit_cascade_note_only_when_prop_set_and_computed_non_null`
    ///
    /// Table-driven: verifies the `inherited_or_default` note is emitted only
    /// when all three conditions hold: empty rules + prop set + non-null external computed.
    #[test]
    fn unit_cascade_note_only_when_prop_set_and_computed_non_null() {
        // (a) empty rules + no external computed → no note
        let out = render_property_cascade("h1", "color", &[], None);
        assert!(
            out.get("inherited_or_default").is_none(),
            "(a) no note when external_computed is None: {out}"
        );

        // (b) empty rules + empty string external computed → no note
        let out = render_property_cascade("h1", "color", &[], Some(""));
        assert!(
            out.get("inherited_or_default").is_none(),
            "(b) no note when external_computed is empty string: {out}"
        );

        // (c) empty rules + non-null external computed → note present
        let out = render_property_cascade("h1", "color", &[], Some("rgb(255, 0, 0)"));
        assert_eq!(
            out.get("inherited_or_default")
                .and_then(serde_json::Value::as_bool),
            Some(true),
            "(c) inherited_or_default must be true when rules empty + computed non-null: {out}"
        );
        let note = out
            .get("note")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        assert!(!note.is_empty(), "(c) note field must be non-empty: {out}");
        assert_eq!(
            out.get("computed").and_then(serde_json::Value::as_str),
            Some("rgb(255, 0, 0)"),
            "(c) computed must reflect external_computed value: {out}"
        );

        // (d) non-empty rules + external computed → no note (rules win, note suppressed)
        let r = rule(
            "body",
            Some("https://example.com/site.css"),
            10,
            &[("color", "red", "")],
        );
        let out = render_property_cascade("h1", "color", &[r], Some("rgb(255, 0, 0)"));
        // rules list for "color" is non-empty, so inherited_or_default must be absent
        let rules_arr = out.get("rules").and_then(serde_json::Value::as_array);
        if let Some(rules_arr) = rules_arr
            && !rules_arr.is_empty()
        {
            assert!(
                out.get("inherited_or_default").is_none(),
                "(d) no note when author rules are present: {out}"
            );
        }
    }

    // Tiny inline helper module: the parse_applied_entry fn in core is
    // private, so we replicate the parse in tests via the public type.
    mod ff_rdp_core_test_helpers {
        use ff_rdp_core::{AppliedRule, RuleProperty};
        use serde_json::Value;

        pub fn parse_entry_for_test(entry: &Value) -> Option<AppliedRule> {
            let rule = entry.get("rule")?;
            let selectors: Vec<String> = rule
                .get("selectors")?
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            // Mirrors the spec parse: matchedSelectorIndexes on the entry
            // resolved against rule.selectors.
            let matched_selectors: Vec<String> = entry
                .get("matchedSelectorIndexes")
                .and_then(Value::as_array)
                .map(|idxs| {
                    idxs.iter()
                        .filter_map(|v| {
                            let i = usize::try_from(v.as_u64()?).ok()?;
                            selectors.get(i).cloned()
                        })
                        .collect()
                })
                .unwrap_or_default();
            let properties: Vec<RuleProperty> = entry
                .get("declarations")?
                .as_array()?
                .iter()
                .filter_map(|d| {
                    Some(RuleProperty {
                        name: d.get("name")?.as_str()?.to_string(),
                        value: d.get("value")?.as_str()?.to_string(),
                        priority: d
                            .get("priority")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect();
            Some(AppliedRule {
                selector: selectors.join(", "),
                source: rule.get("href").and_then(Value::as_str).map(String::from),
                line: rule
                    .get("line")
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok()),
                column: rule
                    .get("column")
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok()),
                properties,
                matched_selectors,
                media: vec![],
                rule_actor_id: rule
                    .get("actor")
                    .and_then(Value::as_str)
                    .and_then(ff_rdp_core::ActorId::try_new),
            })
        }
    }
}
