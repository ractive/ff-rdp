//! Script format: types, deserialization, and parser-level validation.
//!
//! The canonical format is JSON (`draft-2020-12` schema at
//! `schemas/script.schema.json`), but YAML is accepted as input.
//! Both parse to the same in-memory representation.

use std::path::Path;

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Top-level script
// ---------------------------------------------------------------------------

/// A runnable ff-rdp script.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// JSON Schema discriminator. Must be the literal
    /// `"https://ff-rdp.dev/schemas/script/v1.json"` when present.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Format version — must be `1`.
    pub version: u32,

    /// Human-readable script name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Base URL prepended to relative URLs in `navigate` steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Default variable values (overridable via `--vars`).
    #[serde(default)]
    pub vars: std::collections::HashMap<String, String>,

    /// Optional metadata (opaque, not interpreted by the runner).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,

    /// Default timeout in milliseconds for steps that have a `timeout` field
    /// but do not set one explicitly.  Overrides the CLI `--timeout` default
    /// only for this script's steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_timeout_ms: Option<u64>,

    /// Steps to execute, in order.
    pub steps: Vec<Step>,
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

/// A single script step.  Each step is one verb with its arguments.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Navigate(NavigateStep),
    Click(ElementStep),
    Type(TypeStep),
    Wait(WaitStep),
    AssertText(AssertTextStep),
    AssertUrl(AssertUrlStep),
    AssertNoConsoleErrors(AssertNoConsoleErrorsStep),
    AssertNetwork(AssertNetworkStep),
    Screenshot(ScreenshotStep),
    Eval(EvalStep),
    Run(RunStep),
}

impl Step {
    /// Return the verb name for use in output.
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Navigate(_) => "navigate",
            Self::Click(_) => "click",
            Self::Type(_) => "type",
            Self::Wait(_) => "wait",
            Self::AssertText(_) => "assert_text",
            Self::AssertUrl(_) => "assert_url",
            Self::AssertNoConsoleErrors(_) => "assert_no_console_errors",
            Self::AssertNetwork(_) => "assert_network",
            Self::Screenshot(_) => "screenshot",
            Self::Eval(_) => "eval",
            Self::Run(_) => "run",
        }
    }
}

// ---------------------------------------------------------------------------
// Target selection — at most one of selector / ref / page_map / field
// ---------------------------------------------------------------------------

/// Element target for steps that act on a DOM element.
///
/// Exactly one of the four fields must be set; the parser enforces this.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElementTarget {
    /// Raw CSS selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Iter-60 runtime ref ID (e.g. `"e23"`).
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,

    /// Page-map path (iter-62, deferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_map: Option<String>,

    /// Page-map field shorthand (iter-62, deferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl ElementTarget {
    /// Count of set targeting fields.
    fn set_count(&self) -> usize {
        usize::from(self.selector.is_some())
            + usize::from(self.ref_id.is_some())
            + usize::from(self.page_map.is_some())
            + usize::from(self.field.is_some())
    }

    /// Return the targeting field name(s) that are set.
    fn set_fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.selector.is_some() {
            fields.push("selector");
        }
        if self.ref_id.is_some() {
            fields.push("ref");
        }
        if self.page_map.is_some() {
            fields.push("page_map");
        }
        if self.field.is_some() {
            fields.push("field");
        }
        fields
    }

    /// Validate that exactly one targeting field is set.
    pub fn validate(&self) -> anyhow::Result<()> {
        match self.set_count() {
            1 => Ok(()),
            0 => bail!(
                "step must specify exactly one of: selector, ref, page_map, field — none given"
            ),
            _ => bail!(
                "step must specify exactly one of: selector, ref, page_map, field — got multiple: {}",
                self.set_fields().join(", ")
            ),
        }
    }

    /// Check if a page-map or field target is used (requires a loaded PageMap to resolve).
    #[allow(dead_code)]
    pub fn uses_deferred_iter62(&self) -> bool {
        self.page_map.is_some() || self.field.is_some()
    }
}

// ---------------------------------------------------------------------------
// Step types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NavigateStep {
    pub url: String,
    /// Optional: wait for this text to appear after navigation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_text: Option<String>,
    /// Optional: wait for this selector to appear after navigation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_selector: Option<String>,
}

/// Steps that target a single DOM element (click, wait).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElementStep {
    #[serde(flatten)]
    pub target: ElementTarget,
    /// Optional: wait for this text after the action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_for_text: Option<String>,
    /// Optional: wait for this selector after the action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_for_selector: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypeStep {
    #[serde(flatten)]
    pub target: ElementTarget,
    /// Text to type.
    pub text: String,
    /// Clear the field before typing.
    #[serde(default)]
    pub clear: bool,
    /// Treat this field as a secret (suppress in output).
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WaitStep {
    /// Wait for a CSS selector to appear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Wait for text to appear in the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Wait for a JS expression to become truthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval: Option<String>,
    /// Timeout in ms (default: 5000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssertTextStep {
    /// CSS selector of the element to check.
    pub selector: String,
    /// Assert that the text **contains** this substring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contains: Option<String>,
    /// Assert that the text **equals** this exact string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equals: Option<String>,
    /// Negate the assertion.
    #[serde(default)]
    pub not: bool,
    /// Timeout in ms to poll for the condition (default: 5000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssertUrlStep {
    /// Assert URL matches this regex.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matches: Option<String>,
    /// Assert URL equals this exact string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equals: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssertNoConsoleErrorsStep {
    /// Patterns (substrings or regex) to ignore in console errors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_patterns: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssertNetworkStep {
    /// URL substring to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_contains: Option<String>,
    /// HTTP status to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// HTTP method to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Page-map API route ref (iter-62 deferred).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_route: Option<String>,
    /// Drain timeout in ms for direct-mode event collection (default: 500).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

impl AssertNetworkStep {
    /// Validate: `api_route` and `url_contains`/`method` are mutually exclusive.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.api_route.is_some() && (self.url_contains.is_some() || self.method.is_some()) {
            bail!(
                "assert_network: api_route and url_contains/method are mutually exclusive — \
                 use api_route alone (iter-62 page-map) or url_contains/method alone"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotStep {
    /// Path to save the screenshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Capture as base64 instead of writing a file.
    #[serde(default)]
    pub base64: bool,
    /// Capture the full page.
    #[serde(default)]
    pub full_page: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvalStep {
    /// JavaScript expression to evaluate.
    pub script: String,
    /// Wrap in JSON.stringify().
    #[serde(default)]
    pub stringify: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunStep {
    /// Path to the sub-script.
    pub path: String,
    /// Variable overrides for the sub-script.
    #[serde(default)]
    pub with: std::collections::HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Detect the file format from the extension, defaulting to JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFormat {
    Json,
    Yaml,
}

impl ScriptFormat {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("yaml" | "yml") => Self::Yaml,
            _ => Self::Json,
        }
    }

    pub fn from_str_hint(s: &str) -> Option<Self> {
        match s {
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

/// Parse a script from a file path, detecting format from extension.
pub fn parse_script_file(
    path: &Path,
    format_override: Option<ScriptFormat>,
) -> anyhow::Result<Script> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading script file '{}'", path.display()))?;
    let fmt = format_override.unwrap_or_else(|| ScriptFormat::from_path(path));
    parse_script_str(&content, fmt).with_context(|| format!("parsing script '{}'", path.display()))
}

/// Parse a script from a string with the given format.
/// Parse a script from a string with the given format.
///
/// YAML input is converted to a `serde_json::Value` first, then parsed with
/// `serde_json`, so both formats use the same JSON-style enum representation.
/// This means both JSON `{"navigate": {"url": "..."}}` and YAML
/// `navigate:\n  url: ...` parse to the same in-memory `Step::Navigate`.
pub fn parse_script_str(content: &str, fmt: ScriptFormat) -> anyhow::Result<Script> {
    let script: Script = match fmt {
        ScriptFormat::Json => serde_json::from_str(content).context("JSON parse error")?,
        ScriptFormat::Yaml => {
            // Parse YAML → serde_json::Value → Script.
            let value: serde_json::Value =
                serde_yaml::from_str(content).context("YAML parse error")?;
            serde_json::from_value(value).context("YAML→JSON conversion")?
        }
    };
    validate_script(&script)?;
    Ok(script)
}

/// Validate parser-level constraints.
pub fn validate_script(script: &Script) -> anyhow::Result<()> {
    if script.version != 1 {
        bail!(
            "unsupported script version {}; only version 1 is supported",
            script.version
        );
    }
    for (i, step) in script.steps.iter().enumerate() {
        let step_num = i + 1;
        validate_step(step_num, step)?;
    }
    Ok(())
}

fn validate_step(step_num: usize, step: &Step) -> anyhow::Result<()> {
    match step {
        Step::Click(s) => s
            .target
            .validate()
            .with_context(|| format!("step {step_num} (click)"))?,
        Step::Type(s) => s
            .target
            .validate()
            .with_context(|| format!("step {step_num} (type)"))?,
        Step::AssertText(s) => {
            if s.contains.is_none() && s.equals.is_none() {
                bail!("step {step_num} (assert_text): specify at least one of: contains, equals");
            }
            if s.contains.is_some() && s.equals.is_some() {
                bail!("step {step_num} (assert_text): contains and equals are mutually exclusive");
            }
        }
        Step::AssertUrl(s) => {
            if s.matches.is_none() && s.equals.is_none() {
                bail!("step {step_num} (assert_url): specify at least one of: matches, equals");
            }
            if s.matches.is_some() && s.equals.is_some() {
                bail!("step {step_num} (assert_url): matches and equals are mutually exclusive");
            }
        }
        Step::AssertNetwork(s) => s
            .validate()
            .with_context(|| format!("step {step_num} (assert_network)"))?,
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_json_script() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"navigate": {"url": "https://example.com"}}
            ]
        }"#;
        let script = parse_script_str(json, ScriptFormat::Json).unwrap();
        assert_eq!(script.version, 1);
        assert_eq!(script.steps.len(), 1);
    }

    #[test]
    fn parse_minimal_yaml_script() {
        let yaml = "version: 1\nsteps:\n  - navigate:\n      url: https://example.com\n";
        let script = parse_script_str(yaml, ScriptFormat::Yaml).unwrap();
        assert_eq!(script.steps.len(), 1);
    }

    #[test]
    fn rejects_unsupported_version() {
        let json = r#"{"version": 99, "steps": []}"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(err.to_string().contains("version 99"), "{err}");
    }

    /// Format the full anyhow error chain for assertions.
    fn full_err(e: &anyhow::Error) -> String {
        format!("{e:#}")
    }

    #[test]
    fn element_target_rejects_multiple_fields() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"click": {"selector": "button", "ref": "e1"}}
            ]
        }"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(full_err(&err).contains("exactly one"), "{err:#}");
    }

    #[test]
    fn element_target_rejects_no_fields() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"click": {}}
            ]
        }"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(full_err(&err).contains("none given"), "{err:#}");
    }

    #[test]
    fn assert_network_api_route_and_url_contains_are_mutually_exclusive() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"assert_network": {"api_route": "auth.login", "url_contains": "/api/login"}}
            ]
        }"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(full_err(&err).contains("mutually exclusive"), "{err:#}");
    }

    #[test]
    fn assert_text_requires_contains_or_equals() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"assert_text": {"selector": "h1"}}
            ]
        }"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(full_err(&err).contains("at least one"), "{err:#}");
    }

    #[test]
    fn assert_text_rejects_both_contains_and_equals() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"assert_text": {"selector": "h1", "contains": "foo", "equals": "foo"}}
            ]
        }"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(full_err(&err).contains("mutually exclusive"), "{err:#}");
    }

    #[test]
    fn format_detection_json() {
        assert_eq!(
            ScriptFormat::from_path(Path::new("test.json")),
            ScriptFormat::Json
        );
    }

    #[test]
    fn format_detection_yaml() {
        assert_eq!(
            ScriptFormat::from_path(Path::new("test.yaml")),
            ScriptFormat::Yaml
        );
        assert_eq!(
            ScriptFormat::from_path(Path::new("test.yml")),
            ScriptFormat::Yaml
        );
    }

    #[test]
    fn format_detection_unknown_defaults_to_json() {
        assert_eq!(
            ScriptFormat::from_path(Path::new("test.txt")),
            ScriptFormat::Json
        );
    }

    #[test]
    fn deny_unknown_fields_navigate() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"navigate": {"urll": "https://example.com"}}
            ]
        }"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(
            full_err(&err).contains("urll") || full_err(&err).contains("unknown field"),
            "expected unknown-field error, got: {err:#}"
        );
    }

    #[test]
    fn deny_unknown_fields_click() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"click": {"selector": "button", "typo_field": true}}
            ]
        }"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(
            full_err(&err).contains("typo_field") || full_err(&err).contains("unknown field"),
            "expected unknown-field error, got: {err:#}"
        );
    }

    #[test]
    fn deny_unknown_fields_script_top_level() {
        let json = r#"{
            "version": 1,
            "steps": [],
            "unknown_top_level": true
        }"#;
        let err = parse_script_str(json, ScriptFormat::Json).unwrap_err();
        assert!(
            full_err(&err).contains("unknown_top_level")
                || full_err(&err).contains("unknown field"),
            "expected unknown-field error, got: {err:#}"
        );
    }

    #[test]
    fn page_map_and_field_accepted_in_schema_but_deferred() {
        let json = r#"{
            "version": 1,
            "steps": [
                {"click": {"page_map": "pages.login.submit_button"}}
            ]
        }"#;
        // Parsing and validation should succeed — iter-62 deferrals are caught at runtime.
        let script = parse_script_str(json, ScriptFormat::Json).unwrap();
        assert_eq!(script.steps.len(), 1);
        if let Step::Click(s) = &script.steps[0] {
            assert!(s.target.uses_deferred_iter62());
        } else {
            panic!("expected click step");
        }
    }
}
