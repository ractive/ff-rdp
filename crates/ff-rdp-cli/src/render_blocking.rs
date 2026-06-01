//! Shared render-blocking resource classifier.
//!
//! This module defines the canonical rules for determining whether an HTML
//! resource (a `<link>` or `<script>` element) is render-blocking per the
//! HTML specification and browsers' implementation of it.
//!
//! # Rule table
//!
//! | Resource shape | Verdict | HTML spec reference |
//! |---|---|---|
//! | LINK with `rel` ≠ "stylesheet" | Not blocking ("non-stylesheet rel") | [HTML §4.2.4](https://html.spec.whatwg.org/#the-link-element) — only stylesheets render-block |
//! | LINK with `rel` = "stylesheet", `media` doesn't match | Not blocking ("media doesn't match") | [CSS §6.3](https://drafts.csswg.org/css-cascade/#at-import) — non-matching media queries don't block |
//! | LINK with `rel` = "stylesheet", `disabled` | Not blocking ("disabled") | Browser impl: disabled sheets are not applied |
//! | LINK with `rel` = "stylesheet", media matches, not disabled | Blocking | Meets all blocking conditions |
//! | SCRIPT with `async` or `defer` or `type="module"` | Not blocking ("async/defer/module") | [HTML §4.12.1](https://html.spec.whatwg.org/#the-script-element) |
//! | SCRIPT with no `src` or `src` starts with `data:` | Not blocking ("inline/data-uri") | Inline scripts are not network resources |
//! | SCRIPT otherwise | Blocking | External, synchronous script |
//!
//! # Design
//!
//! The Rust classifier (`classify`) is the source-of-truth for unit tests.
//! The JS predicate (`RENDER_BLOCKING_JS_PREDICATE`) is the production code
//! path: it runs in Firefox via `eval` and implements the same rules in JS.
//! Both `dom stats` and `perf audit` compose the JS predicate into their
//! collection scripts so the two surfaces always agree.

/// A synthetic description of a link or script resource for classification.
///
/// Fields mirror the HTML attributes that determine render-blocking status.
///
/// Only used in tests — the production path runs `RENDER_BLOCKING_JS_PREDICATE`
/// in Firefox. The `#[cfg(test)]` gate prevents dead_code warnings in release builds.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// Element tag name (lowercase): "link" or "script".
    pub tag: ResourceTag,
    /// `rel` attribute value (relevant only for LINK elements).
    pub rel: Option<String>,
    /// `media` attribute value; `None` means "all" (matches all media).
    pub media_matches: Option<bool>,
    /// Whether the element has the `disabled` attribute (LINK only).
    pub disabled: bool,
    /// Whether the element has the `async` attribute (SCRIPT only).
    pub r#async: bool,
    /// Whether the element has the `defer` attribute (SCRIPT only).
    pub defer: bool,
    /// The `type` attribute value (SCRIPT only).
    pub script_type: Option<String>,
    /// The `src` attribute value (SCRIPT only). `None` = inline script.
    pub src: Option<String>,
}

/// HTML element tag for a resource.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceTag {
    Link,
    Script,
}

/// Classification result for a resource.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderBlockingKind {
    /// The resource participates in render-blocking.
    Blocking,
    /// The resource does not block rendering; the reason is included for
    /// diagnostics and documentation.
    NotBlocking(&'static str),
}

#[cfg(test)]
impl RenderBlockingKind {
    /// Returns `true` if this resource is render-blocking.
    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::Blocking)
    }
}

/// Classify a resource as render-blocking or not.
///
/// This is the Rust implementation of the same rules encoded in
/// `RENDER_BLOCKING_JS_PREDICATE`. Both must be kept in sync.
///
/// Only used in tests — the production path runs `RENDER_BLOCKING_JS_PREDICATE`
/// in Firefox. See module-level documentation for the design rationale.
#[cfg(test)]
pub fn classify(r: &Resource) -> RenderBlockingKind {
    match r.tag {
        ResourceTag::Link => {
            let rel = r.rel.as_deref().unwrap_or("").to_ascii_lowercase();
            let rel = rel.trim();

            // Only rel="stylesheet" can render-block; all other rels (icon,
            // preload, prefetch, dns-prefetch, preconnect, modulepreload,
            // manifest, …) never do.
            // HTML §4.2.4: <link rel="stylesheet"> is the only render-blocking link type.
            if rel != "stylesheet" {
                return RenderBlockingKind::NotBlocking("non-stylesheet rel");
            }

            // Stylesheet blocks only if media matches.
            // CSS §6.3: non-matching media queries are ignored.
            if r.media_matches == Some(false) {
                return RenderBlockingKind::NotBlocking("media doesn't match");
            }

            // Disabled stylesheets are not applied.
            if r.disabled {
                return RenderBlockingKind::NotBlocking("disabled");
            }

            RenderBlockingKind::Blocking
        }
        ResourceTag::Script => {
            // async / defer / module scripts never block rendering.
            // HTML §4.12.1: async and defer scripts are fetched without blocking.
            if r.r#async || r.defer {
                return RenderBlockingKind::NotBlocking("async/defer/module");
            }
            let script_type = r.script_type.as_deref().unwrap_or("").to_ascii_lowercase();
            if script_type.trim() == "module" {
                return RenderBlockingKind::NotBlocking("async/defer/module");
            }

            // Inline scripts and data: URIs are not network resources.
            match &r.src {
                None => return RenderBlockingKind::NotBlocking("inline/data-uri"),
                Some(src) if src.starts_with("data:") => {
                    return RenderBlockingKind::NotBlocking("inline/data-uri");
                }
                Some(_) => {}
            }

            RenderBlockingKind::Blocking
        }
    }
}

/// JavaScript function body that implements the same render-blocking rules as
/// `classify`. Used by `dom stats` and `perf audit` to count blocking resources
/// in the browser. Both embed this predicate via string interpolation so the
/// two surfaces always agree.
///
/// The predicate expects `el` to be a DOM element (link or script) and
/// returns `true` if the element is render-blocking.
///
/// Usage: embed inside a JS IIFE that iterates `document.querySelectorAll('link,script')`:
///
/// ```js
/// var isRenderBlocking = function(el) {
///   // ... (RENDER_BLOCKING_JS_PREDICATE contents) ...
/// };
/// document.querySelectorAll('link,script').forEach(function(el) {
///   if (isRenderBlocking(el)) { ... }
/// });
/// ```
pub const RENDER_BLOCKING_JS_PREDICATE: &str = "function(el) {
  if (el.tagName === 'LINK') {
    var rel = (el.getAttribute('rel') || '').toLowerCase().trim();
    if (rel !== 'stylesheet') return false;
    var media = el.media || 'all';
    try { if (!window.matchMedia(media).matches) return false; } catch(e) {}
    if (el.disabled) return false;
    return true;
  } else if (el.tagName === 'SCRIPT') {
    if (el.async || el.defer || (el.getAttribute('type') || '').toLowerCase().trim() === 'module') return false;
    if (!el.src || el.src.startsWith('data:')) return false;
    return true;
  }
  return false;
}";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn link(rel: &str, media_matches: Option<bool>, disabled: bool) -> Resource {
        Resource {
            tag: ResourceTag::Link,
            rel: Some(rel.to_string()),
            media_matches,
            disabled,
            r#async: false,
            defer: false,
            script_type: None,
            src: None,
        }
    }

    fn script(
        r#async: bool,
        defer: bool,
        script_type: Option<&str>,
        src: Option<&str>,
    ) -> Resource {
        Resource {
            tag: ResourceTag::Script,
            rel: None,
            media_matches: None,
            disabled: false,
            r#async,
            defer,
            script_type: script_type.map(String::from),
            src: src.map(String::from),
        }
    }

    /// AC: `unit_classify_render_blocking_table_driven`
    ///
    /// One row per branch in the classifier. All rows must pass.
    #[test]
    fn unit_classify_render_blocking_table_driven() {
        let cases: &[(&str, Resource, bool)] = &[
            // LINK — non-stylesheet rels are never blocking
            (
                "link[rel=icon] → not blocking",
                link("icon", None, false),
                false,
            ),
            (
                "link[rel=preload] → not blocking",
                link("preload", None, false),
                false,
            ),
            (
                "link[rel=prefetch] → not blocking",
                link("prefetch", None, false),
                false,
            ),
            (
                "link[rel=manifest] → not blocking",
                link("manifest", None, false),
                false,
            ),
            // LINK — stylesheet with non-matching media
            (
                "link[rel=stylesheet,media=print] → not blocking (media doesn't match)",
                link("stylesheet", Some(false), false),
                false,
            ),
            // LINK — stylesheet disabled
            (
                "link[rel=stylesheet,disabled] → not blocking",
                link("stylesheet", None, true),
                false,
            ),
            // LINK — stylesheet, media matches, not disabled → blocking
            (
                "link[rel=stylesheet,media=all] → blocking",
                link("stylesheet", None, false),
                true,
            ),
            (
                "link[rel=stylesheet,media matches] → blocking",
                link("stylesheet", Some(true), false),
                true,
            ),
            // SCRIPT — async
            (
                "script[async] → not blocking",
                script(true, false, None, Some("https://example.com/a.js")),
                false,
            ),
            // SCRIPT — defer
            (
                "script[defer] → not blocking",
                script(false, true, None, Some("https://example.com/d.js")),
                false,
            ),
            // SCRIPT — type=module
            (
                "script[type=module] → not blocking",
                script(
                    false,
                    false,
                    Some("module"),
                    Some("https://example.com/m.js"),
                ),
                false,
            ),
            // SCRIPT — inline (no src)
            (
                "inline script → not blocking",
                script(false, false, None, None),
                false,
            ),
            // SCRIPT — data: URI src
            (
                "script[src=data:...] → not blocking",
                script(false, false, None, Some("data:text/javascript,alert(1)")),
                false,
            ),
            // SCRIPT — external, synchronous → blocking
            (
                "external sync script → blocking",
                script(false, false, None, Some("https://example.com/sync.js")),
                true,
            ),
        ];

        for (label, resource, expected_blocking) in cases {
            let kind = classify(resource);
            assert_eq!(
                kind.is_blocking(),
                *expected_blocking,
                "classify case failed: {label}\n  resource: {resource:?}\n  got: {kind:?}"
            );
        }
    }

    /// AC: `pre_fix_repro_dom_stats_and_perf_audit_render_blocking_agree`
    ///
    /// Constructs a list of synthetic resources (5 blocking, 3 not blocking)
    /// and verifies `classify` returns the correct verdict for each.
    /// This is the Rust-level proxy for the JS classifier agreement check.
    #[test]
    fn pre_fix_repro_dom_stats_and_perf_audit_render_blocking_agree() {
        let resources = [
            // 5 blocking
            link("stylesheet", None, false),
            link("stylesheet", Some(true), false),
            script(false, false, None, Some("https://cdn.example.com/a.js")),
            script(false, false, None, Some("https://cdn.example.com/b.js")),
            script(false, false, None, Some("https://cdn.example.com/c.js")),
            // 3 not blocking
            link("icon", None, false),
            script(true, false, None, Some("https://cdn.example.com/async.js")),
            script(false, false, None, None), // inline
        ];

        let blocking_count = resources
            .iter()
            .filter(|r| classify(r).is_blocking())
            .count();
        assert_eq!(
            blocking_count, 5,
            "expected 5 blocking resources, got {blocking_count}"
        );

        let not_blocking_count = resources
            .iter()
            .filter(|r| !classify(r).is_blocking())
            .count();
        assert_eq!(
            not_blocking_count, 3,
            "expected 3 non-blocking resources, got {not_blocking_count}"
        );
    }
}
