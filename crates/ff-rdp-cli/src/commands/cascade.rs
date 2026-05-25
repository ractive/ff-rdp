//! `ff-rdp cascade <SELECTOR> [--prop NAME | --all]` — the CSS cascade inspector.
//!
//! For a single element (matched by CSS selector), returns the ordered list
//! of matching CSS rules per property, annotated with origin, specificity,
//! source location, `!important` flag, and a `winner: true` marker on the
//! rule whose declaration would become the computed value.
//!
//! This is a read-only view; style mutation is out of scope (see iter-81 plan).

use ff_rdp_core::css::specificity::{self, Specificity};
use ff_rdp_core::{ActorId, AppliedRule, DomWalkerActor, InspectorActor, PageStyleActor};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::hints::{HintContext, HintSource};
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::{ConnectedTab, connect_and_get_target};

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
/// Inline `style="…"` rules have no href.  Everything else is treated as
/// an author rule.  This is a heuristic; full origin info would require
/// querying each parent stylesheet separately (out of scope for iter-81).
fn classify_origin(source: Option<&str>) -> Origin {
    match source {
        None | Some("") => Origin::Inline,
        Some(href)
            if href.starts_with("resource://")
                || href.starts_with("chrome://")
                || href.starts_with("resource:///") =>
        {
            Origin::UserAgent
        }
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
    // Origin tier within the group. For normal: Inline(3) > Author(2) > User(1) > UA(0).
    // For important: UA(3) > User(2) > Author(1) > Inline(0) — flip the order.
    let normal_rank = match e.origin {
        Origin::UserAgent => 0,
        Origin::User => 1,
        Origin::Author => 2,
        Origin::Inline => 3,
    };
    let origin_rank = if e.important {
        3 - normal_rank
    } else {
        normal_rank
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
        rule.selector.split(',').map(str::trim).collect()
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
            rule.properties
                .iter()
                .filter(|p| p.name.eq_ignore_ascii_case(property))
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
fn render_property_cascade(selector: &str, property: &str, rules: &[AppliedRule]) -> Value {
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

    json!({
        "selector": selector,
        "property": property,
        "computed": computed,
        "rules": rules_json,
    })
}

/// Set of properties declared anywhere in the applied rules.
fn declared_properties(rules: &[AppliedRule]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for rule in rules {
        for prop in &rule.properties {
            seen.insert(prop.name.to_ascii_lowercase());
        }
    }
    seen.into_iter().collect()
}

/// Shared setup: connect, find the node, fetch its applied rules.
///
/// Returns the connected tab (transport owner) and the applied rules in the
/// order Firefox returned them.
fn fetch_applied(cli: &Cli, selector: &str) -> Result<(ConnectedTab, Vec<AppliedRule>), AppError> {
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

    let applied = PageStyleActor::get_applied(ctx.transport_mut(), &page_style_actor, &node_actor)
        .map_err(map_cascade_error)?;

    Ok((ctx, applied))
}

/// CLI entry point for `ff-rdp cascade <SEL> [--prop NAME | --all]`.
pub fn run(cli: &Cli, selector: &str, prop: Option<&str>, all: bool) -> Result<(), AppError> {
    let (_ctx, applied) = fetch_applied(cli, selector)?;

    let _ = all; // --all is the default; flag is accepted for clarity.
    let properties: Vec<String> = match prop {
        Some(name) => vec![name.to_ascii_lowercase()],
        None => declared_properties(&applied),
    };

    let results: Vec<Value> = properties
        .iter()
        .map(|p| render_property_cascade(selector, p, &applied))
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

        let out = render_property_cascade("dialog#lightbox", "display", &rules);
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

        let out = render_property_cascade("dialog#lightbox", "display", &rules);
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
        let out = render_property_cascade(".title", "color", &[r]);
        let arr = out["rules"].as_array().unwrap();
        assert_eq!(arr[0]["specificity"], json!([0, 1, 0]));
        assert_eq!(arr[0]["selector"], ".title");
    }

    #[test]
    fn cascade_classifies_inline_and_ua_origins() {
        // Inline (no href).
        let inline = rule("dialog", None, 0, &[("color", "red", "")]);
        assert_eq!(classify_origin(inline.source.as_deref()), Origin::Inline);
        // UA stylesheet.
        assert_eq!(
            classify_origin(Some("resource://gre-resources/ua.css")),
            Origin::UserAgent
        );
        assert_eq!(
            classify_origin(Some("chrome://global/skin/global.css")),
            Origin::UserAgent
        );
        // Author.
        assert_eq!(
            classify_origin(Some("https://example.com/site.css")),
            Origin::Author
        );
        // Empty href → inline.
        assert_eq!(classify_origin(Some("")), Origin::Inline);
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
        let out = render_property_cascade("dialog", "display", &rules);
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
        let out = render_property_cascade("dialog", "display", &rules);
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
        let out = render_property_cascade(".a", "color", &rules);
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
                        "selectors": ["dialog"],
                        "matchedSelectors": ["dialog"]
                    },
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
                        "selectors": ["dialog#lightbox"],
                        "matchedSelectors": ["dialog#lightbox"]
                    },
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
        let out = render_property_cascade("dialog#lightbox", "display", &rules);
        assert_eq!(out["computed"], "flex");
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
            let matched_selectors: Vec<String> = rule
                .get("matchedSelectors")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
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
            })
        }
    }
}
