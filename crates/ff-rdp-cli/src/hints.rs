//! Contextual hints — "what next?" suggestions produced after every command.
//!
//! Each command builds a [`HintContext`] describing its inputs and outcome,
//! then calls [`generate_hints`] to obtain up to [`MAX_HINTS`] follow-up
//! suggestions shown to the user (or consumed by an AI agent).

use serde::Serialize;

/// Maximum number of hints any command can produce.
pub const MAX_HINTS: usize = 5;

/// A single contextual hint suggesting a follow-up command.
#[derive(Debug, Clone, Serialize)]
pub struct Hint {
    /// Human-readable description of what the command does.
    pub(crate) description: String,
    /// The full ff-rdp command to run (copy-pasteable).
    pub(crate) cmd: String,
}

impl Hint {
    fn new(description: impl Into<String>, cmd: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            cmd: cmd.into(),
        }
    }
}

/// Identifies which command produced the output so [`generate_hints`] can
/// dispatch to the right generator.
pub enum HintSource {
    Launch,
    Navigate,
    Tabs,
    Reload,
    Back,
    Forward,
    Dom,
    DomStats,
    DomTree,
    Click,
    TypeText,
    Wait,
    Console,
    Network,
    Perf,
    PerfVitals,
    PerfSummary,
    PerfAudit,
    Screenshot,
    Snapshot,
    A11y,
    A11yContrast,
    A11ySummary,
    Styles,
    Computed,
    Geometry,
    Responsive,
    Cookies,
    Storage,
    Sources,
    PageText,
    Eval,
    Inspect,
}

/// Context gathered by a command for hint generation.
///
/// Commands populate the relevant fields; [`generate_hints`] uses them to
/// build contextual suggestions.
pub struct HintContext {
    pub(crate) source: HintSource,
    /// CSS selector used by the command (`dom`, `click`, `styles`, etc.).
    pub(crate) selector: Option<String>,
    /// Whether the command output contained errors (e.g., console errors).
    pub(crate) has_errors: bool,
    /// Whether `--detail` mode was used.
    pub(crate) detail: bool,
    /// Whether `--fail-only` was used (a11y contrast).
    pub(crate) fail_only: bool,
    /// Specific storage type used (e.g., `"local"`, `"session"`).
    pub(crate) storage_type: Option<String>,
}

impl HintContext {
    pub fn new(source: HintSource) -> Self {
        Self {
            source,
            selector: None,
            has_errors: false,
            detail: false,
            fail_only: false,
            storage_type: None,
        }
    }

    pub fn with_selector(mut self, sel: impl Into<String>) -> Self {
        self.selector = Some(sel.into());
        self
    }

    pub fn with_has_errors(mut self, has: bool) -> Self {
        self.has_errors = has;
        self
    }

    pub fn with_detail(mut self, detail: bool) -> Self {
        self.detail = detail;
        self
    }

    pub fn with_fail_only(mut self, fail_only: bool) -> Self {
        self.fail_only = fail_only;
        self
    }

    pub fn with_storage_type(mut self, st: impl Into<String>) -> Self {
        self.storage_type = Some(st.into());
        self
    }
}

/// Generate up to [`MAX_HINTS`] contextual follow-up suggestions for a command.
pub fn generate_hints(ctx: &HintContext) -> Vec<Hint> {
    let hints = match ctx.source {
        HintSource::Launch => hints_launch(ctx),
        HintSource::Navigate => hints_navigate(ctx),
        HintSource::Tabs => hints_tabs(ctx),
        HintSource::Reload => hints_reload(ctx),
        HintSource::Back => hints_back(ctx),
        HintSource::Forward => hints_forward(ctx),
        HintSource::Dom => hints_dom(ctx),
        HintSource::DomStats => hints_dom_stats(ctx),
        HintSource::DomTree => hints_dom_tree(ctx),
        HintSource::Click => hints_click(ctx),
        HintSource::TypeText => hints_type_text(ctx),
        HintSource::Wait => hints_wait(ctx),
        HintSource::Console => hints_console(ctx),
        HintSource::Network => hints_network(ctx),
        HintSource::Perf => hints_perf(ctx),
        HintSource::PerfVitals => hints_perf_vitals(ctx),
        HintSource::PerfSummary => hints_perf_summary(ctx),
        HintSource::PerfAudit => hints_perf_audit(ctx),
        HintSource::Screenshot => hints_screenshot(ctx),
        HintSource::Snapshot => hints_snapshot(ctx),
        HintSource::A11y => hints_a11y(ctx),
        HintSource::A11yContrast => hints_a11y_contrast(ctx),
        HintSource::A11ySummary => hints_a11y_summary(ctx),
        HintSource::Styles => hints_styles(ctx),
        HintSource::Computed => hints_computed(ctx),
        HintSource::Geometry => hints_geometry(ctx),
        HintSource::Responsive => hints_responsive(ctx),
        HintSource::Cookies => hints_cookies(ctx),
        HintSource::Storage => hints_storage(ctx),
        HintSource::Sources => hints_sources(ctx),
        HintSource::PageText => hints_page_text(ctx),
        HintSource::Eval => hints_eval(ctx),
        HintSource::Inspect => hints_inspect(ctx),
    };
    hints.into_iter().take(MAX_HINTS).collect()
}

// ---------------------------------------------------------------------------
// Private hint generators — one per command
// ---------------------------------------------------------------------------

/// Escape a CSS selector for use in double-quoted shell arguments.
fn shell_escape_selector(sel: &str) -> String {
    sel.replace('\\', "\\\\").replace('"', "\\\"")
}

fn hints_launch(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("List open tabs", "ff-rdp tabs"),
        Hint::new("Navigate to a URL", "ff-rdp navigate <URL>"),
    ]
}

fn hints_navigate(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new(
            "Capture DOM snapshot (3 levels deep)",
            "ff-rdp snapshot --depth 3",
        ),
        Hint::new("Check for console errors", "ff-rdp console --level error"),
        Hint::new("Take a screenshot", "ff-rdp screenshot -o page.png"),
        Hint::new("Read the page heading", r#"ff-rdp dom "h1" --text"#),
    ]
}

fn hints_tabs(_ctx: &HintContext) -> Vec<Hint> {
    vec![Hint::new(
        "Navigate to a URL in tab 1",
        "ff-rdp navigate <URL> --tab 1",
    )]
}

fn hints_reload(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new(
            "Check for console errors after reload",
            "ff-rdp console --level error",
        ),
        Hint::new("Inspect network requests after reload", "ff-rdp network"),
    ]
}

fn hints_back(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Capture DOM snapshot", "ff-rdp snapshot"),
        Hint::new("Check for console errors", "ff-rdp console --level error"),
    ]
}

fn hints_forward(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Capture DOM snapshot", "ff-rdp snapshot"),
        Hint::new("Check for console errors", "ff-rdp console --level error"),
    ]
}

fn hints_dom(ctx: &HintContext) -> Vec<Hint> {
    let sel = ctx.selector.as_deref().unwrap_or("selector");
    vec![
        Hint::new(
            format!("Click on \"{sel}\""),
            format!(r#"ff-rdp click "{}""#, shell_escape_selector(sel)),
        ),
        Hint::new(
            format!("Inspect styles of \"{sel}\""),
            format!(
                r#"ff-rdp styles "{}" --properties color,display"#,
                shell_escape_selector(sel)
            ),
        ),
        Hint::new(
            format!("Get computed color for \"{sel}\""),
            format!(
                r#"ff-rdp computed "{}" --prop color"#,
                shell_escape_selector(sel)
            ),
        ),
    ]
}

fn hints_dom_stats(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Show DOM tree (3 levels)", "ff-rdp dom tree --depth 3"),
        Hint::new("Capture DOM snapshot", "ff-rdp snapshot"),
    ]
}

fn hints_dom_tree(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Capture DOM snapshot", "ff-rdp snapshot"),
        Hint::new("Run accessibility summary", "ff-rdp a11y summary"),
    ]
}

fn hints_click(ctx: &HintContext) -> Vec<Hint> {
    let sel = ctx.selector.as_deref().unwrap_or("selector");
    vec![
        Hint::new(
            format!("Wait for \"{sel}\" to appear"),
            format!(r#"ff-rdp wait --selector "{}""#, shell_escape_selector(sel)),
        ),
        Hint::new(
            "Take a screenshot after clicking",
            "ff-rdp screenshot -o after-click.png",
        ),
    ]
}

fn hints_type_text(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new(
            "Submit form after typing",
            r#"ff-rdp click "button[type=submit]""#,
        ),
        Hint::new("Wait for confirmation text", r#"ff-rdp wait --text "...""#),
    ]
}

fn hints_wait(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Capture DOM snapshot", "ff-rdp snapshot"),
        Hint::new("Take a screenshot", "ff-rdp screenshot -o wait.png"),
        Hint::new("Read body text", r#"ff-rdp dom "body" --text"#),
    ]
}

fn hints_console(ctx: &HintContext) -> Vec<Hint> {
    if ctx.has_errors {
        vec![Hint::new(
            "Stream error-level console messages",
            "ff-rdp console --follow --level error",
        )]
    } else {
        vec![Hint::new(
            "Stream all console messages",
            "ff-rdp console --follow",
        )]
    }
}

fn hints_network(ctx: &HintContext) -> Vec<Hint> {
    if ctx.detail {
        vec![
            Hint::new(
                "Filter network requests with status >= 400",
                r"ff-rdp network --detail --jq '[.results[] | select(.status >= 400)]'",
            ),
            Hint::new("Run a performance audit", "ff-rdp perf audit"),
        ]
    } else {
        vec![
            Hint::new(
                "Show detailed network request info",
                "ff-rdp network --detail",
            ),
            Hint::new("Run a performance audit", "ff-rdp perf audit"),
        ]
    }
}

fn hints_perf(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Show Core Web Vitals", "ff-rdp perf vitals"),
        Hint::new("Run a performance audit", "ff-rdp perf audit"),
    ]
}

fn hints_perf_vitals(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Run a performance audit", "ff-rdp perf audit"),
        Hint::new("Show performance summary", "ff-rdp perf summary"),
    ]
}

fn hints_perf_summary(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Show Core Web Vitals", "ff-rdp perf vitals"),
        Hint::new("Run a performance audit", "ff-rdp perf audit"),
    ]
}

fn hints_perf_audit(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new(
            "Check accessibility contrast failures",
            "ff-rdp a11y contrast --fail-only",
        ),
        Hint::new("Take a screenshot", "ff-rdp screenshot -o audit.png"),
    ]
}

fn hints_screenshot(_ctx: &HintContext) -> Vec<Hint> {
    vec![Hint::new(
        "Capture DOM snapshot (3 levels deep)",
        "ff-rdp snapshot --depth 3",
    )]
}

fn hints_snapshot(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new("Run accessibility summary", "ff-rdp a11y summary"),
        Hint::new("Read body text", r#"ff-rdp dom "body" --text"#),
    ]
}

fn hints_a11y(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new(
            "Check accessibility contrast failures",
            "ff-rdp a11y contrast --fail-only",
        ),
        Hint::new("Run accessibility summary", "ff-rdp a11y summary"),
    ]
}

fn hints_a11y_contrast(ctx: &HintContext) -> Vec<Hint> {
    if ctx.fail_only {
        vec![
            Hint::new("Run accessibility summary", "ff-rdp a11y summary"),
            Hint::new(
                "Take a screenshot of contrast issues",
                "ff-rdp screenshot -o contrast.png",
            ),
        ]
    } else {
        vec![Hint::new(
            "Show only contrast failures",
            "ff-rdp a11y contrast --fail-only",
        )]
    }
}

fn hints_a11y_summary(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new(
            "Run interactive accessibility check",
            "ff-rdp a11y --interactive",
        ),
        Hint::new("Check color contrast", "ff-rdp a11y contrast"),
    ]
}

fn hints_styles(ctx: &HintContext) -> Vec<Hint> {
    let sel = ctx.selector.as_deref().unwrap_or("selector");
    vec![
        Hint::new(
            format!("Get computed color for \"{sel}\""),
            format!(
                r#"ff-rdp computed "{}" --prop color"#,
                shell_escape_selector(sel)
            ),
        ),
        Hint::new(
            format!("Show applied styles for \"{sel}\""),
            format!(
                r#"ff-rdp styles "{}" --applied"#,
                shell_escape_selector(sel)
            ),
        ),
        Hint::new(
            format!("Show layout styles for \"{sel}\""),
            format!(r#"ff-rdp styles "{}" --layout"#, shell_escape_selector(sel)),
        ),
    ]
}

fn hints_computed(ctx: &HintContext) -> Vec<Hint> {
    let sel = ctx.selector.as_deref().unwrap_or("selector");
    vec![
        Hint::new(
            format!("Show applied styles for \"{sel}\""),
            format!(
                r#"ff-rdp styles "{}" --applied"#,
                shell_escape_selector(sel)
            ),
        ),
        Hint::new(
            format!("Get box geometry for \"{sel}\""),
            format!(r#"ff-rdp geometry "{}""#, shell_escape_selector(sel)),
        ),
    ]
}

fn hints_geometry(ctx: &HintContext) -> Vec<Hint> {
    let sel = ctx.selector.as_deref().unwrap_or("selector");
    vec![Hint::new(
        format!("Test responsive layout for \"{sel}\""),
        format!(r#"ff-rdp responsive "{}""#, shell_escape_selector(sel)),
    )]
}

fn hints_responsive(_ctx: &HintContext) -> Vec<Hint> {
    vec![Hint::new(
        "Take a screenshot of the responsive view",
        "ff-rdp screenshot -o responsive.png",
    )]
}

fn hints_cookies(_ctx: &HintContext) -> Vec<Hint> {
    vec![Hint::new("Inspect local storage", "ff-rdp storage local")]
}

fn hints_storage(ctx: &HintContext) -> Vec<Hint> {
    let mut hints = Vec::new();
    match ctx.storage_type.as_deref() {
        Some("local") => hints.push(Hint::new(
            "Inspect session storage",
            "ff-rdp storage session",
        )),
        Some("session") => hints.push(Hint::new("Inspect local storage", "ff-rdp storage local")),
        _ => {
            hints.push(Hint::new("Inspect local storage", "ff-rdp storage local"));
            hints.push(Hint::new(
                "Inspect session storage",
                "ff-rdp storage session",
            ));
        }
    }
    hints.push(Hint::new("Inspect cookies", "ff-rdp cookies"));
    hints
}

fn hints_sources(_ctx: &HintContext) -> Vec<Hint> {
    vec![Hint::new(
        "Evaluate a script file",
        "ff-rdp eval --file script.js",
    )]
}

fn hints_page_text(_ctx: &HintContext) -> Vec<Hint> {
    vec![
        Hint::new(
            "Read body text with attributes",
            r#"ff-rdp dom "body" --text-attrs"#,
        ),
        Hint::new("Capture DOM snapshot", "ff-rdp snapshot"),
    ]
}

fn hints_eval(_ctx: &HintContext) -> Vec<Hint> {
    vec![Hint::new(
        "Check for console errors",
        "ff-rdp console --level error",
    )]
}

fn hints_inspect(_ctx: &HintContext) -> Vec<Hint> {
    // Terminal command — no logical follow-up to suggest.
    vec![]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_hints_never_exceeds_max() {
        // Use Navigate which has 4 hints — still within MAX_HINTS, but the
        // truncation logic is exercised for any source that could exceed it.
        let ctx = HintContext::new(HintSource::Navigate);
        let hints = generate_hints(&ctx);
        assert!(
            hints.len() <= MAX_HINTS,
            "expected at most {MAX_HINTS} hints, got {}",
            hints.len()
        );
    }

    #[test]
    fn navigate_hints_are_non_empty() {
        let ctx = HintContext::new(HintSource::Navigate);
        let hints = generate_hints(&ctx);
        assert!(!hints.is_empty(), "navigate should produce hints");
    }

    #[test]
    fn dom_hints_include_selector() {
        let sel = "#main-content";
        let ctx = HintContext::new(HintSource::Dom).with_selector(sel);
        let hints = generate_hints(&ctx);
        assert!(
            hints.iter().any(|h| h.cmd.contains(sel)),
            "dom hints should interpolate the selector into cmd strings; got: {hints:?}"
        );
    }

    #[test]
    fn console_hints_differ_on_has_errors() {
        let ctx_errors = HintContext::new(HintSource::Console).with_has_errors(true);
        let ctx_clean = HintContext::new(HintSource::Console).with_has_errors(false);

        let hints_errors = generate_hints(&ctx_errors);
        let hints_clean = generate_hints(&ctx_clean);

        // When errors are present the hint should mention --level error.
        assert!(
            hints_errors.iter().any(|h| h.cmd.contains("--level error")),
            "expected a --level error hint when has_errors=true; got: {hints_errors:?}"
        );

        // When no errors, the hint should NOT gate on --level error.
        assert!(
            !hints_clean.iter().any(|h| h.cmd.contains("--level error")),
            "did not expect a --level error hint when has_errors=false; got: {hints_clean:?}"
        );
    }

    #[test]
    fn storage_hints_context_sensitive() {
        let ctx_local = HintContext::new(HintSource::Storage).with_storage_type("local");
        let hints_local = generate_hints(&ctx_local);
        assert!(
            hints_local
                .iter()
                .any(|h| h.cmd.contains("storage session")),
            "after storage local, should hint at storage session; got: {hints_local:?}"
        );
        assert!(
            !hints_local.iter().any(|h| h.cmd.contains("storage local")),
            "should not re-suggest storage local; got: {hints_local:?}"
        );

        let ctx_session = HintContext::new(HintSource::Storage).with_storage_type("session");
        let hints_session = generate_hints(&ctx_session);
        assert!(
            hints_session
                .iter()
                .any(|h| h.cmd.contains("storage local")),
            "after storage session, should hint at storage local; got: {hints_session:?}"
        );
        assert!(
            !hints_session
                .iter()
                .any(|h| h.cmd.contains("storage session")),
            "should not re-suggest storage session; got: {hints_session:?}"
        );
    }

    #[test]
    fn inspect_hints_are_empty() {
        let ctx = HintContext::new(HintSource::Inspect);
        let hints = generate_hints(&ctx);
        assert!(
            hints.is_empty(),
            "inspect is a terminal command — no hints expected"
        );
    }

    #[test]
    fn a11y_contrast_fail_only_context_sensitive() {
        let ctx_all = HintContext::new(HintSource::A11yContrast).with_fail_only(false);
        let hints_all = generate_hints(&ctx_all);
        assert!(
            hints_all.iter().any(|h| h.cmd.contains("--fail-only")),
            "should suggest --fail-only when fail_only=false; got: {hints_all:?}"
        );

        let ctx_fail = HintContext::new(HintSource::A11yContrast).with_fail_only(true);
        let hints_fail = generate_hints(&ctx_fail);
        assert!(
            !hints_fail.iter().any(|h| h.cmd.contains("--fail-only")),
            "should not re-suggest --fail-only when already in fail_only mode; got: {hints_fail:?}"
        );
    }

    #[test]
    fn styles_hints_include_selector() {
        let sel = ".card";
        let ctx = HintContext::new(HintSource::Styles).with_selector(sel);
        let hints = generate_hints(&ctx);
        assert!(
            hints.iter().any(|h| h.cmd.contains(sel)),
            "styles hints should interpolate the selector; got: {hints:?}"
        );
    }
}
