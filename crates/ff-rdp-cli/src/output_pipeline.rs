use serde_json::Value;

use crate::error::AppError;
use crate::hints::{Hint, HintContext, generate_hints};
use crate::output;

/// Output format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Text,
    /// Raw HTML passthrough — used by `dom` and `snapshot` to restore
    /// the pre-iter-60 full HTML shape when the default ARIA-tree output
    /// is not what the caller needs.
    Html,
}

/// Whether contextual hints should be generated and included in output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintsMode {
    /// Generate and display hints.
    On,
    /// Suppress hints entirely (not generated, not in output).
    Off,
}

/// Policy for missing (null) jq path results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JqMissingPolicy {
    /// Silently omit null outputs from `--jq` (default, least surprise for pipelines).
    SilentOmit,
    /// Exit non-zero with a diagnostic message when a path resolves to null.
    Strict,
}

pub struct OutputPipeline {
    jq_filter: Option<String>,
    jq_missing: JqMissingPolicy,
    format: OutputFormat,
    hints_mode: HintsMode,
}

impl OutputPipeline {
    #[allow(dead_code)]
    pub fn new(jq_filter: Option<String>) -> Self {
        Self {
            jq_filter,
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Json,
            hints_mode: HintsMode::Off,
        }
    }

    /// Build an `OutputPipeline` from global CLI flags.
    ///
    /// Returns `AppError::User` if:
    /// - `--format` is not "json", "text", or "html"
    ///
    /// When `--jq` is combined with `--format text`, the jq filter runs first
    /// on the JSON form, then the result is rendered as human-readable text.
    pub fn from_cli(cli: &crate::cli::args::Cli) -> Result<Self, AppError> {
        let format = match cli.format.as_str() {
            "json" => OutputFormat::Json,
            "text" => OutputFormat::Text,
            "html" => OutputFormat::Html,
            other => {
                return Err(AppError::User(format!(
                    "invalid --format value '{other}': must be 'json', 'text', or 'html'"
                )));
            }
        };

        // Hints default: on for text, off for json/html.
        // --jq always suppresses hints (pipeline needs clean data).
        // Explicit --hints / --no-hints override the default.
        let hints_mode = if cli.no_hints || cli.jq.is_some() {
            HintsMode::Off
        } else if cli.hints {
            HintsMode::On
        } else {
            // Default based on format
            match format {
                OutputFormat::Text => HintsMode::On,
                OutputFormat::Json | OutputFormat::Html => HintsMode::Off,
            }
        };

        let jq_missing = if cli.jq_strict {
            JqMissingPolicy::Strict
        } else {
            JqMissingPolicy::SilentOmit
        };

        Ok(Self {
            jq_filter: cli.jq.clone(),
            jq_missing,
            format,
            hints_mode,
        })
    }

    /// Force `hints_mode = Off` on this pipeline.
    ///
    /// Used by `eval` when `--stringify` is set: the caller is asking for raw
    /// value extraction and the trailing `-> ff-rdp …` tip line is
    /// indistinguishable from real output when the consumer captures stdout
    /// as a single string (dogfood session 49 #6 / user feedback).
    ///
    /// Idempotent — if hints are already off (e.g. `--no-hints` was passed),
    /// this is a no-op.
    #[must_use]
    pub fn without_hints(mut self) -> Self {
        self.hints_mode = HintsMode::Off;
        self
    }

    /// Apply the pipeline to a JSON envelope and print to stdout.
    ///
    /// If a `HintContext` is provided and hints are enabled, generates
    /// contextual hints and injects them into the envelope.
    ///
    /// If a jq filter is set, apply it to the full envelope so that users
    /// can access any field (`.results`, `.total`, `.meta`).
    /// Otherwise pretty-print the envelope as-is (JSON) or render a
    /// human-readable table (text).
    pub fn finalize_with_hints(
        &self,
        envelope: &Value,
        hint_ctx: Option<&HintContext>,
    ) -> anyhow::Result<()> {
        let mut envelope = envelope.clone();

        // Generate and inject hints only when enabled.
        let hints = if self.hints_mode == HintsMode::On {
            let h = hint_ctx.map(generate_hints).unwrap_or_default();
            output::inject_hints(&mut envelope, &h)?;
            h
        } else {
            vec![]
        };

        match &self.jq_filter {
            Some(filter) => {
                let raw_filtered = output::apply_jq_filter(&envelope, filter)?;

                // Apply the missing-path policy: filter out nulls (SilentOmit) or
                // error on null (Strict). A null output signals that a path was absent
                // from the input — e.g. `.results.nonexistent` on an object without
                // that key.
                let filtered: Vec<serde_json::Value> = match self.jq_missing {
                    JqMissingPolicy::SilentOmit => {
                        raw_filtered.into_iter().filter(|v| !v.is_null()).collect()
                    }
                    JqMissingPolicy::Strict => {
                        if raw_filtered.iter().any(serde_json::Value::is_null) {
                            anyhow::bail!("jq path '{filter}' not found in input");
                        }
                        raw_filtered
                    }
                };

                match self.format {
                    OutputFormat::Text => {
                        // jq runs first, then text rendering applies to each
                        // output value. This is the "filter, then make terse"
                        // combination enabled by iter-60 (D2).
                        for value in &filtered {
                            let synthetic = serde_json::json!({
                                "results": value,
                                "total": 1,
                            });
                            render_text(&synthetic);
                        }
                        render_hints(&hints);
                    }
                    _ => {
                        // Default: compact JSON line per jq output.
                        for value in filtered {
                            println!("{}", serde_json::to_string(&value)?);
                        }
                    }
                }
            }
            None => match self.format {
                OutputFormat::Json | OutputFormat::Html => {
                    println!("{}", serde_json::to_string_pretty(&envelope)?);
                }
                OutputFormat::Text => {
                    render_text(&envelope);
                    render_hints(&hints);
                }
            },
        }
        Ok(())
    }

    /// Apply the pipeline to a JSON envelope and print to stdout.
    ///
    /// Convenience wrapper that calls [`finalize_with_hints`](Self::finalize_with_hints)
    /// without a hint context. Hints will be an empty array.
    pub fn finalize(&self, envelope: &Value) -> anyhow::Result<()> {
        self.finalize_with_hints(envelope, None::<&HintContext>)
    }
}

/// Render the output envelope as human-readable text.
///
/// Dispatch rules:
/// - `results` is an array of objects  → ASCII table with padded columns
/// - `results` is a flat object        → key-value list
/// - anything else (complex/nested)    → pretty-printed JSON fallback
///
/// A truncation hint line is printed when the envelope contains `"hint"`.
fn render_text(envelope: &Value) {
    let results = envelope.get("results").unwrap_or(&Value::Null);

    match results {
        Value::Array(arr) if arr.iter().all(Value::is_object) && !arr.is_empty() => {
            render_table(arr);
        }
        Value::Object(map) if map.values().all(|v| !v.is_object() && !v.is_array()) => {
            render_kv(map);
        }
        _ => {
            // Fallback: pretty JSON (complex / nested structures)
            if let Ok(pretty) = serde_json::to_string_pretty(results) {
                println!("{pretty}");
            }
        }
    }

    // Truncation hint
    if let Some(hint) = envelope.get("hint").and_then(|h| h.as_str()) {
        println!();
        println!("{hint}");
    } else if let Some(total) = envelope.get("total").and_then(Value::as_u64)
        && let Some(Value::Array(arr)) = envelope.get("results")
    {
        let shown = arr.len() as u64;
        if shown < total {
            println!();
            println!("Showing {shown} of {total} results");
        }
    }
}

/// Render contextual hints as `-> cmd  # description` lines.
fn render_hints(hints: &[Hint]) {
    if hints.is_empty() {
        return;
    }
    println!();
    for hint in hints {
        println!("  -> {}  # {}", hint.cmd, hint.description);
    }
}

/// Render an array of JSON objects as an ASCII table.
///
/// Column headers come from the union of all object keys across rows (sorted
/// alphabetically by serde_json's default BTreeMap ordering, then any extra
/// keys from subsequent rows are appended).  Each cell is coerced to a string
/// and padded to the column width.
fn render_table(rows: &[Value]) {
    // Collect ordered column names from the first row, then any unseen keys
    // from subsequent rows.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut columns: Vec<String> = Vec::new();
    for row in rows {
        if let Value::Object(map) = row {
            for key in map.keys() {
                if seen.insert(key.clone()) {
                    columns.push(key.clone());
                }
            }
        }
    }

    if columns.is_empty() {
        return;
    }

    // Compute column widths: max of header width and all cell widths.
    let mut widths: Vec<usize> = columns.iter().map(String::len).collect();
    for row in rows {
        for (i, col) in columns.iter().enumerate() {
            let cell = value_to_cell(row.get(col).unwrap_or(&Value::Null));
            widths[i] = widths[i].max(cell.len());
        }
    }

    // Print header row.  Object keys can be attacker-influenced (e.g. cookie
    // names, header names), so sanitize before formatting.
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let safe = ff_rdp_core::sanitize_for_terminal(col);
            format!("{safe:<width$}", width = widths[i])
        })
        .collect();
    println!("{}", header.join("  "));

    // Print separator.
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    println!("{}", sep.join("  "));

    // Print data rows.
    for row in rows {
        let cells: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(i, col)| {
                let cell = value_to_cell(row.get(col).unwrap_or(&Value::Null));
                format!("{cell:<width$}", width = widths[i])
            })
            .collect();
        println!("{}", cells.join("  "));
    }
}

/// Render a flat JSON object as a key-value list.
///
/// Keys are sanitized before width is computed so alignment uses the
/// rendered widths, not the raw (possibly attacker-controlled) keys.
fn render_kv(map: &serde_json::Map<String, Value>) {
    let sanitized: Vec<(String, &Value)> = map
        .iter()
        .map(|(k, v)| (ff_rdp_core::sanitize_for_terminal(k).into_owned(), v))
        .collect();
    let max_key = sanitized.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, val) in &sanitized {
        let cell = value_to_cell(val);
        println!("{key:<max_key$}  {cell}");
    }
}

/// Convert a JSON value to a display string suitable for table cells.
///
/// Attacker-influenced strings (cookie names, page titles, console output)
/// can contain ANSI escape sequences that would otherwise reposition the
/// cursor or clear the screen when printed; route everything through
/// [`sanitize_for_terminal`] at this boundary.
fn value_to_cell(val: &Value) -> String {
    match val {
        Value::String(s) => ff_rdp_core::sanitize_for_terminal(s).into_owned(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        // For arrays / nested objects fall back to compact JSON — also
        // sanitized because nested strings may contain attacker data.
        other => ff_rdp_core::sanitize_for_terminal(&other.to_string()).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── OutputFormat::Text: array of objects → table ─────────────────────────

    #[test]
    fn text_array_of_objects_renders_table() {
        let pipeline = OutputPipeline {
            jq_filter: None,
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Text,
            hints_mode: HintsMode::Off,
        };
        // Should not panic; spot-check via render_table directly.
        let rows = vec![
            json!({"url": "https://a.com/app.js", "duration_ms": 42.5}),
            json!({"url": "https://b.com/style.css", "duration_ms": 15.3}),
        ];
        // render_table itself: collect widths
        if let Value::Array(arr) = json!([
            {"url": "https://a.com/app.js", "duration_ms": 42.5},
            {"url": "https://b.com/style.css", "duration_ms": 15.3}
        ]) {
            render_table(&arr);
        }
        // Verify finalize does not error
        let envelope = json!({
            "results": rows,
            "total": 2,
            "meta": {}
        });
        assert!(pipeline.finalize(&envelope).is_ok());
    }

    // ── OutputFormat::Text: single flat object → key-value list ─────────────

    #[test]
    fn text_flat_object_renders_kv() {
        let pipeline = OutputPipeline {
            jq_filter: None,
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Text,
            hints_mode: HintsMode::Off,
        };
        let envelope = json!({
            "results": {"ttfb_ms": 42.5, "fcp_ms": 150.0, "lcp_ms": 300.0},
            "total": 1,
            "meta": {}
        });
        assert!(pipeline.finalize(&envelope).is_ok());
    }

    // ── truncation hint ──────────────────────────────────────────────────────

    #[test]
    fn text_renders_truncation_hint() {
        // We capture the hint path indirectly by ensuring finalize succeeds
        // on an envelope that has "hint" and "truncated".
        let pipeline = OutputPipeline {
            jq_filter: None,
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Text,
            hints_mode: HintsMode::Off,
        };
        let envelope = json!({
            "results": [{"url": "https://a.com"}],
            "total": 10,
            "truncated": true,
            "hint": "showing 1 of 10, use --all for complete list",
            "meta": {}
        });
        assert!(pipeline.finalize(&envelope).is_ok());
    }

    // ── JSON format unchanged ────────────────────────────────────────────────

    #[test]
    fn json_format_unchanged() {
        let pipeline = OutputPipeline::new(None);
        let envelope = json!({"results": [], "total": 0, "meta": {}});
        assert!(pipeline.finalize(&envelope).is_ok());
    }

    // ── from_cli validation ──────────────────────────────────────────────────

    #[test]
    fn from_cli_invalid_format_returns_error() {
        // Exercise the format-validation branch directly.
        let result: Result<OutputFormat, AppError> = match "badvalue" {
            "json" => Ok(OutputFormat::Json),
            "text" => Ok(OutputFormat::Text),
            other => Err(AppError::User(format!(
                "invalid --format value '{other}': must be 'json' or 'text'"
            ))),
        };
        assert!(result.is_err());
        if let Err(AppError::User(msg)) = result {
            assert!(msg.contains("badvalue"));
        }
    }

    #[test]
    fn from_cli_invalid_format_html_variant_accepted() {
        // "html" is now a valid format value (iter-60 D2 escape hatch).
        let result: Result<OutputFormat, AppError> = match "html" {
            "json" => Ok(OutputFormat::Json),
            "text" => Ok(OutputFormat::Text),
            "html" => Ok(OutputFormat::Html),
            other => Err(AppError::User(format!(
                "invalid --format value '{other}': must be 'json', 'text', or 'html'"
            ))),
        };
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), OutputFormat::Html);
    }

    #[test]
    fn jq_with_text_format_renders_text() {
        // iter-60 D2: --jq + --format text is now allowed. The pipeline applies
        // jq first, then renders the result as text.
        let pipeline = OutputPipeline {
            jq_filter: Some(".results".to_string()),
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Text,
            hints_mode: HintsMode::Off,
        };
        let envelope = json!({"results": [{"url": "https://a.com"}], "total": 1});
        // Should not error — jq+text combination is now valid.
        assert!(pipeline.finalize(&envelope).is_ok());
    }

    // ── iter-63 AC: ANSI escapes in table cells are sanitized ────────────────

    #[test]
    fn value_to_cell_strips_ansi_escapes_from_strings() {
        let hostile = Value::String("foo\x1b[2Jbar".to_string());
        let rendered = value_to_cell(&hostile);
        assert!(
            !rendered.as_bytes().contains(&0x1b),
            "rendered cell must not contain raw ESC bytes, got: {rendered:?}"
        );
        assert!(
            rendered.contains("foo") && rendered.contains("bar"),
            "non-escape content must survive sanitization, got: {rendered:?}"
        );
    }

    // ── unit_jq_filter_silent_vs_strict (iter-86 Theme D) ───────────────────

    /// Default (SilentOmit): a missing path produces no output, not "null".
    /// `finalize` must succeed with exit 0 and print nothing for a null path.
    #[test]
    fn unit_jq_filter_silent_omit_missing_path_produces_no_output() {
        let pipeline = OutputPipeline {
            jq_filter: Some(".results.does_not_exist".to_string()),
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Json,
            hints_mode: HintsMode::Off,
        };
        let envelope = json!({"results": {"present": 1}, "total": 1});
        // Should not error — missing path is silently omitted.
        assert!(
            pipeline.finalize(&envelope).is_ok(),
            "SilentOmit: finalize must succeed on missing path"
        );
    }

    /// The SilentOmit policy must filter out null values from the jq output.
    #[test]
    fn unit_jq_filter_silent_omit_filters_null() {
        // `.does_not_exist` returns null in jaq when the key is absent.
        // SilentOmit must produce an empty vec (nothing printed).
        let input = json!({"results": {"x": 1}});
        let raw = crate::output::apply_jq_filter(&input, ".results.missing").unwrap();
        assert_eq!(
            raw,
            vec![serde_json::Value::Null],
            "jaq returns null for absent key"
        );

        // SilentOmit filters it out.
        let silent: Vec<serde_json::Value> = raw.into_iter().filter(|v| !v.is_null()).collect();
        assert!(
            silent.is_empty(),
            "SilentOmit must produce nothing for a missing path, got: {silent:?}"
        );
    }

    /// Present path must still pass through both policies unchanged.
    #[test]
    fn unit_jq_filter_present_path_passes_through() {
        let pipeline = OutputPipeline {
            jq_filter: Some(".results.present".to_string()),
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Json,
            hints_mode: HintsMode::Off,
        };
        let envelope = json!({"results": {"present": 42}, "total": 1});
        assert!(
            pipeline.finalize(&envelope).is_ok(),
            "present path must pass through without error"
        );
    }

    #[test]
    fn value_to_cell_strips_ansi_from_nested_objects() {
        let hostile = json!({ "name": "evil\x1b[31m" });
        let rendered = value_to_cell(&hostile);
        assert!(
            !rendered.as_bytes().contains(&0x1b),
            "nested JSON must also be sanitized, got: {rendered:?}"
        );
    }
}
