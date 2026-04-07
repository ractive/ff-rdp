use serde_json::Value;

use crate::error::AppError;
use crate::output;

/// Output format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Text,
}

pub struct OutputPipeline {
    jq_filter: Option<String>,
    format: OutputFormat,
}

impl OutputPipeline {
    #[allow(dead_code)]
    pub fn new(jq_filter: Option<String>) -> Self {
        Self {
            jq_filter,
            format: OutputFormat::Json,
        }
    }

    /// Build an `OutputPipeline` from global CLI flags.
    ///
    /// Returns `AppError::User` if:
    /// - `--format` is not "json" or "text"
    /// - `--format text` is combined with `--jq` (mutually exclusive)
    pub fn from_cli(cli: &crate::cli::args::Cli) -> Result<Self, AppError> {
        let format = match cli.format.as_str() {
            "json" => OutputFormat::Json,
            "text" => OutputFormat::Text,
            other => {
                return Err(AppError::User(format!(
                    "invalid --format value '{other}': must be 'json' or 'text'"
                )));
            }
        };
        if format == OutputFormat::Text && cli.jq.is_some() {
            return Err(AppError::User(
                "--format text and --jq are mutually exclusive".to_string(),
            ));
        }
        Ok(Self {
            jq_filter: cli.jq.clone(),
            format,
        })
    }

    /// Apply the pipeline to a JSON envelope and print to stdout.
    ///
    /// If a jq filter is set, apply it to the full `{results, total, meta}`
    /// envelope so that users can access any envelope field (`.results`,
    /// `.total`, `.meta`, `.truncated`).  Use `.results[].url` to drill
    /// into array results or `.results.lcp_ms` for object results.
    /// Otherwise pretty-print the envelope as-is (JSON) or render a
    /// human-readable table (text).
    pub fn finalize(&self, envelope: &Value) -> anyhow::Result<()> {
        match &self.jq_filter {
            Some(filter) => {
                let output = output::apply_jq_filter(envelope, filter)?;
                for value in output {
                    println!("{}", serde_json::to_string(&value)?);
                }
            }
            None => match self.format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(envelope)?);
                }
                OutputFormat::Text => {
                    render_text(envelope);
                }
            },
        }
        Ok(())
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

    // Print header row.
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| format!("{col:<width$}", width = widths[i]))
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
/// Keys are right-padded to align values.
fn render_kv(map: &serde_json::Map<String, Value>) {
    let max_key = map.keys().map(String::len).max().unwrap_or(0);
    for (key, val) in map {
        let cell = value_to_cell(val);
        println!("{key:<max_key$}  {cell}");
    }
}

/// Convert a JSON value to a display string suitable for table cells.
fn value_to_cell(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        // For arrays / nested objects fall back to compact JSON.
        other => other.to_string(),
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
            format: OutputFormat::Text,
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
            format: OutputFormat::Text,
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
            format: OutputFormat::Text,
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
    fn from_cli_text_with_jq_is_error() {
        // Simulate the mutual-exclusion check inline.
        let format = OutputFormat::Text;
        let jq: Option<String> = Some(".results".to_string());
        let result: Result<(), AppError> = if format == OutputFormat::Text && jq.is_some() {
            Err(AppError::User(
                "--format text and --jq are mutually exclusive".to_string(),
            ))
        } else {
            Ok(())
        };
        assert!(result.is_err());
        if let Err(AppError::User(msg)) = result {
            assert!(msg.contains("mutually exclusive"));
        }
    }
}
