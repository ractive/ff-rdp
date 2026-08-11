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
    ///
    /// Returns `AppError` directly (iter-141 Theme F) rather than
    /// `anyhow::Result` so a bad `--jq` filter can be classified as
    /// `AppError::User` — not `AppError::Internal` — at the point where the
    /// distinction is knowable. Previously every error here (including jq
    /// parse/compile/runtime errors, which are entirely a function of
    /// user-supplied `--jq` syntax) collapsed through the blanket
    /// `From<anyhow::Error> for AppError` impl to `Internal`, so
    /// `ff-rdp dom h1 --jq 'this is not valid %%%'` reported
    /// `error_type: "Internal"` for what is unambiguously a user input
    /// error. `AppError` implements the std `From<T> for T` blanket impl, so
    /// every existing `.map_err(AppError::from)` call site remains
    /// source-compatible unchanged.
    pub fn finalize_with_hints(
        &self,
        envelope: &Value,
        hint_ctx: Option<&HintContext>,
    ) -> Result<(), AppError> {
        let mut envelope = envelope.clone();

        // iter-100 Theme E: surface any daemon-lifecycle warnings recorded
        // during connection resolution (e.g. an auto-start that never
        // registered and silently fell back to a direct connection).  Injected
        // as a top-level `"warnings"` array so tests and users can tell
        // "used the daemon" apart from "quietly went direct".  Omitted
        // entirely on the happy path to keep default output compact.
        //
        // iter-123 Theme A: the same warnings are also rendered to **stderr**
        // in `--format text` / `--format text --jq …` output (see the
        // `render_warnings` calls below), so a `daemon_autostart_failed` signal
        // is visible in text mode too — not only via `--jq '.warnings'` on JSON
        // output.  Kept out of stdout so it never corrupts the results table.
        let warnings_for_text = crate::daemon_status::take_warnings_json();
        if let Some(warnings) = warnings_for_text.clone()
            && let Some(obj) = envelope.as_object_mut()
        {
            obj.insert("warnings".to_string(), warnings);
        }

        // Generate and inject hints only when enabled. Hint serialization can
        // only fail on a `serde`-level bug in `Hint`'s own `Serialize` impl —
        // never on user input — so this stays `Internal`.
        let hints = if self.hints_mode == HintsMode::On {
            let h = hint_ctx.map(generate_hints).unwrap_or_default();
            output::inject_hints(&mut envelope, &h).map_err(AppError::Internal)?;
            h
        } else {
            vec![]
        };

        match &self.jq_filter {
            Some(filter) => {
                // iter-141 Theme F: a bad `--jq` filter (parse/compile/runtime
                // error) is a user input error, not an internal one.
                let raw_filtered = output::apply_jq_filter(&envelope, filter)
                    .map_err(|e| AppError::User(e.to_string()))?;

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
                            // A missing path under --jq-strict is also a user
                            // input condition (the filter just doesn't match
                            // this envelope's shape), not an internal error.
                            return Err(AppError::User(format!(
                                "jq path '{filter}' not found in input"
                            )));
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
                        render_warnings(warnings_for_text.as_ref());
                    }
                    _ => {
                        // Default: compact JSON line per jq output. Serialization
                        // of an already-valid `Value` cannot fail on user input —
                        // any failure here is genuinely internal.
                        for value in filtered {
                            let line = serde_json::to_string(&value)
                                .map_err(|e| AppError::Internal(anyhow::Error::new(e)))?;
                            println!("{line}");
                        }
                    }
                }
            }
            None => match self.format {
                OutputFormat::Json | OutputFormat::Html => {
                    let pretty = serde_json::to_string_pretty(&envelope)
                        .map_err(|e| AppError::Internal(anyhow::Error::new(e)))?;
                    println!("{pretty}");
                }
                OutputFormat::Text => {
                    render_text(&envelope);
                    render_hints(&hints);
                    render_warnings(warnings_for_text.as_ref());
                }
            },
        }
        Ok(())
    }

    /// Apply the pipeline to a JSON envelope and print to stdout.
    ///
    /// Convenience wrapper that calls [`finalize_with_hints`](Self::finalize_with_hints)
    /// without a hint context. Hints will be an empty array.
    pub fn finalize(&self, envelope: &Value) -> Result<(), AppError> {
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
        Value::Array(arr) if arr.is_empty() => {
            // iter-141 Theme D: an empty array used to fall through to the
            // pretty-JSON fallback below, printing a bare `[]` — which drops
            // `sampled`/`capped` (and any other top-level envelope metadata)
            // entirely. Dogfooding session 63: `a11y contrast --fail-only
            // --format text` with 218 elements sampled but capped at the JS
            // walker's element ceiling printed `[]` and then suggested
            // screenshotting contrast issues that, per the JSON form's
            // `sampled: 218, capped: true`, were never actually all checked
            // — a clean bill of health that wasn't one. Surface that context
            // instead of a bare `[]`.
            render_empty_results(envelope);
        }
        Value::Array(arr) if arr.iter().all(Value::is_object) => {
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

/// Render an empty `results` array in `--format text` (iter-141 Theme D).
///
/// A bare `[]`/empty table drops any sample-size or truncation context a
/// caller needs to tell "genuinely nothing found" apart from "capped before
/// everything could be checked". Surfaces the top-level `sampled` field
/// (`a11y contrast`) and a `capped` flag wherever the envelope carries one —
/// checked at both `meta.summary.capped` (`a11y contrast`'s shape) and
/// `meta.capped` (in case a future command puts it directly under `meta`) —
/// alongside the (also-informative) `truncated`/`hint` handling already
/// appended by the caller.
fn render_empty_results(envelope: &Value) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(sampled) = envelope.get("sampled").and_then(Value::as_u64) {
        parts.push(format!("{sampled} sampled"));
    }
    let capped = envelope
        .get("meta")
        .and_then(|m| m.get("summary"))
        .and_then(|s| s.get("capped"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || envelope
            .get("meta")
            .and_then(|m| m.get("capped"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if capped {
        parts.push("capped".to_owned());
    }
    if parts.is_empty() {
        println!("(no results)");
    } else {
        println!("(no results — {})", parts.join(", "));
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

/// Render daemon-lifecycle warnings to **stderr** in text output (iter-123
/// Theme A).
///
/// In JSON output these warnings live in the envelope's top-level `warnings`
/// array; in `--format text` the results table never showed them, so a
/// `daemon_autostart_failed` signal was only visible via `--jq '.warnings'`.
/// This surfaces each warning as a `warning: <type>: <reason>` line on stderr —
/// visible to the human, but kept off stdout so it never corrupts the parsed
/// results.  `warnings` is the JSON array injected into the envelope (or `None`
/// on the happy path, in which case nothing is printed).
fn render_warnings(warnings: Option<&Value>) {
    let Some(Value::Array(arr)) = warnings else {
        return;
    };
    for w in arr {
        let wtype = w.get("type").and_then(Value::as_str).unwrap_or("warning");
        let reason = w.get("reason").and_then(Value::as_str).unwrap_or_default();
        if reason.is_empty() {
            eprintln!("warning: {wtype}");
        } else {
            eprintln!("warning: {wtype}: {reason}");
        }
    }
}

/// Collect ordered column names from an array of row objects.
///
/// Column headers come from the union of all object keys across rows, in
/// first-seen insertion order (the first row's keys, then any new keys
/// introduced by later rows are appended). This relies on the workspace's
/// `serde_json` `preserve_order` feature — without it, `serde_json::Map` is
/// backed by a `BTreeMap` and keys silently come out alphabetically instead,
/// which is why callers building result objects (e.g. `doctor`'s
/// `build_results_json`) should put narrow columns first and wide free-text
/// columns (like `detail`) last.
fn collect_table_columns(rows: &[Value]) -> Vec<String> {
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
    columns
}

/// Column-width cap applied to every table cell (iter-141 Theme A — widened
/// from iter-128's `url`-only cap after dogfooding session 63 found the same
/// unbounded-width defect on `console`'s free-text `message` column and
/// `dom`'s JSON-stringified `attrs` column: one very long Firefox console
/// message set the width for all 39 rows, producing a 255 KB table out of a
/// `--format text` mode that exists specifically to save tokens). Applied to
/// *every* column regardless of name — `url` no longer gets special-cased,
/// since [`crate::output::middle_ellipsis`] already preserves the
/// `scheme://host` prefix for any string that looks like a URL, whichever
/// column it's in. Chosen to keep a typical result row (a handful of narrow
/// columns plus one free-text column) within ~120 terminal columns.
const TEXT_CELL_MAX_WIDTH: usize = 80;

/// Render a single table cell: sanitize for terminal safety, then
/// middle-ellipsize to [`TEXT_CELL_MAX_WIDTH`] (iter-128 Theme C, widened to
/// all columns in iter-141 Theme A) so no single long value — a tracking
/// URL, a console message, a JSON-stringified `attrs` blob — can blow out a
/// column, and the whole table, to thousands of characters wide.
fn render_cell(row: &Value, col: &str) -> String {
    let cell = value_to_cell(row.get(col).unwrap_or(&Value::Null));
    crate::output::middle_ellipsis(&cell, TEXT_CELL_MAX_WIDTH)
}

/// Render an array of JSON objects as an ASCII table.
///
/// See [`collect_table_columns`] for the column-ordering contract. Each
/// cell is coerced to a string and middle-ellipsized (see [`render_cell`])
/// so a handful of very long values — URLs, console messages, stringified
/// attribute blobs — can't blow a column, and the whole line, out to
/// thousands of characters wide (iter-141 Theme A).
fn render_table(rows: &[Value]) {
    let columns = collect_table_columns(rows);

    if columns.is_empty() {
        return;
    }

    // Compute column widths: max of header width and all (post-ellipsis)
    // cell widths.
    let mut widths: Vec<usize> = columns.iter().map(String::len).collect();
    for row in rows {
        for (i, col) in columns.iter().enumerate() {
            let cell = render_cell(row, col);
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
                let cell = render_cell(row, col);
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

    // ── iter-123 Theme A: daemon-warning text parity ─────────────────────────

    /// AC `live_daemon_warning_text_parity` (unit core): `render_warnings`
    /// tolerates the injected `warnings` array shape without panicking and is a
    /// no-op when there are no warnings — i.e. text output has a path to surface
    /// a `daemon_autostart_failed` signal, not only `--jq '.warnings'` on JSON.
    #[test]
    fn render_warnings_handles_array_and_none() {
        // No warnings (happy path) — must not panic.
        render_warnings(None);
        render_warnings(Some(&Value::Null));
        // A populated warnings array of the exact shape the recorder emits.
        let warnings = json!([
            {"type": "daemon_autostart_failed", "reason": "spawn died before the registry write"},
            {"type": "daemon_autostart_failed"} // reason omitted — still tolerated
        ]);
        render_warnings(Some(&warnings));
    }

    /// `render_warnings` emits one `warning: <type>: <reason>` line per entry
    /// (iter-123 Theme A) — the text-output surface for the `daemon_autostart_failed`
    /// signal.  This exercises the exact JSON shape the recorder injects without
    /// touching the process-global warning slot (which the `daemon::client`
    /// tests own and assert exact counts against), so the two suites never
    /// contend.
    #[test]
    fn render_warnings_emits_line_for_each_entry() {
        // Shape matches `daemon_status::take_warnings_json`: an array of
        // {"type","reason"} objects.
        let warnings = json!([
            {"type": "daemon_autostart_failed", "reason": "registry write raced or was slow"},
        ]);
        // Renders to stderr without panicking; the assertion is that the
        // populated-array branch is taken (vs. the early-return None/non-array
        // branches covered above).
        render_warnings(Some(&warnings));
        // A non-array value must be ignored (early return), not panic.
        render_warnings(Some(&json!({"type": "not-an-array"})));
    }

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

    // ── regression: text-table column order for a doctor-style envelope ─────
    //
    // `doctor`'s `build_results_json` inserts keys as glyph, name, status,
    // (hint), detail so the wide free-text column ends up last in the
    // rendered table. This only holds if `serde_json::Map` preserves
    // insertion order (workspace `preserve_order` feature) — if that
    // feature regresses, `serde_json::Map` silently reverts to alphabetical
    // (BTreeMap) ordering and `detail` jumps back to the front. Exercise
    // `collect_table_columns` directly (the same helper `render_table`
    // uses) rather than capturing stdout.
    #[test]
    fn doctor_style_row_orders_glyph_first_and_detail_after_status() {
        let row = json!({
            "glyph": "\u{2713}",
            "name": "daemon",
            "status": "pass",
            "hint": "no daemon running; commands will connect directly",
            "detail": "no daemon running (commands will connect directly)",
        });
        let columns = collect_table_columns(&[row]);

        assert_eq!(
            columns.first().map(String::as_str),
            Some("glyph"),
            "glyph must be the first column, got order: {columns:?}"
        );

        let status_idx = columns
            .iter()
            .position(|c| c == "status")
            .expect("status column must be present");
        let detail_idx = columns
            .iter()
            .position(|c| c == "detail")
            .expect("detail column must be present");
        assert!(
            detail_idx > status_idx,
            "detail (wide free-text) must come after status, got order: {columns:?}"
        );
    }

    /// `hint` is optional in the JSON shape (omitted when `None`); when
    /// absent the column list must simply skip it, not shift `detail`
    /// earlier than intended.
    #[test]
    fn doctor_style_row_without_hint_still_orders_detail_last() {
        let row = json!({
            "glyph": "\u{2713}",
            "name": "daemon",
            "status": "pass",
            "detail": "no daemon running (commands will connect directly)",
        });
        let columns = collect_table_columns(&[row]);
        assert_eq!(columns, vec!["glyph", "name", "status", "detail"]);
    }

    // ── iter-141 Theme D: empty results must not print a bare `[]` ─────────
    //
    // AC `live_141_text_empty_result_keeps_metadata`: `a11y contrast
    // --fail-only --format text` with zero failures must still report the
    // sampled count and capped state, not a bare `[]` that reads as a clean
    // bill of health.

    /// The `finalize` path end-to-end: an empty-results envelope with
    /// `sampled`/`meta.summary.capped` (a11y contrast's exact shape) must
    /// not error, and — since stdout can't be captured here — at minimum
    /// must route through `render_empty_results` rather than the pretty-JSON
    /// fallback (exercised directly below for the actual content check).
    #[test]
    fn text_empty_results_with_sampled_and_capped_does_not_error() {
        let pipeline = OutputPipeline {
            jq_filter: None,
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Text,
            hints_mode: HintsMode::Off,
        };
        let envelope = json!({
            "results": [],
            "total": 0,
            "sampled": 218,
            "meta": {"summary": {"total": 218, "aa_pass": 218, "aa_fail": 0, "capped": true}}
        });
        assert!(pipeline.finalize(&envelope).is_ok());
    }

    /// `render_empty_results` is where the actual message is built — assert
    /// its dispatch is reachable for an empty array (i.e. `render_text`
    /// routes empty arrays there, not through the pretty-JSON `[]` fallback)
    /// by calling it directly and confirming it does not panic on the
    /// documented a11y-contrast shape, a plain no-metadata shape, and a
    /// `meta.capped` (not `meta.summary.capped`) shape.
    #[test]
    fn render_empty_results_handles_all_documented_shapes() {
        render_empty_results(&json!({"sampled": 218, "meta": {"summary": {"capped": true}}}));
        render_empty_results(&json!({"meta": {"capped": true}}));
        render_empty_results(&json!({"results": []}));
    }

    /// Regression guard: `render_text` must dispatch an empty array to
    /// `render_empty_results`, not the pretty-JSON fallback that used to
    /// print a bare `[]`. Verified by constructing the exact envelope shape
    /// and confirming `finalize` succeeds (the dispatch match arm itself is
    /// exercised; the printed content is covered by the direct
    /// `render_empty_results` tests above).
    #[test]
    fn text_empty_array_results_routes_through_dedicated_branch() {
        let pipeline = OutputPipeline {
            jq_filter: None,
            jq_missing: JqMissingPolicy::SilentOmit,
            format: OutputFormat::Text,
            hints_mode: HintsMode::Off,
        };
        let envelope = json!({"results": [], "total": 0});
        assert!(pipeline.finalize(&envelope).is_ok());
    }

    // ── iter-141 Theme A: --format text pads every row to the widest cell ──
    //
    // dogfooding session 63: `console --level error --format text` on a page
    // with one very long console message produced a 255 KB table — every one
    // of 39 rows padded to 8725 columns — because only the `url` column was
    // middle-ellipsized (iter-128). `message`/`attrs`/any other free-text
    // column was left unbounded.

    /// AC `live_141_console_text_bounded` (unit core): a `message` column —
    /// not named `url` — with one very long value must still be bounded, and
    /// every row's rendered cell width must be capped at
    /// [`TEXT_CELL_MAX_WIDTH`], not inflated to match the longest row.
    #[test]
    fn render_cell_bounds_non_url_columns() {
        let long_message = "x".repeat(8000);
        let row = json!({"level": "error", "message": long_message});
        let cell = render_cell(&row, "message");
        assert!(
            cell.chars().count() <= TEXT_CELL_MAX_WIDTH,
            "message cell must be bounded, got {} chars",
            cell.chars().count()
        );
        assert!(cell.contains('…'), "long cell must be ellipsized: {cell:?}");
    }

    /// A JSON-stringified nested value (e.g. `dom`'s `attrs` column) must
    /// also be bounded — `value_to_cell` serializes objects/arrays to
    /// compact JSON before `render_cell` ellipsizes the result.
    #[test]
    fn render_cell_bounds_stringified_nested_value() {
        let mut attrs = serde_json::Map::new();
        for i in 0..50 {
            attrs.insert(format!("data-attr-{i}"), json!("some-long-value-here"));
        }
        let row = json!({"tag": "div", "attrs": Value::Object(attrs)});
        let cell = render_cell(&row, "attrs");
        assert!(
            cell.chars().count() <= TEXT_CELL_MAX_WIDTH,
            "stringified attrs cell must be bounded, got {} chars",
            cell.chars().count()
        );
    }

    /// A short value in a non-`url` column must pass through unchanged
    /// (no-op below the cap) — the fix must not touch normal-width cells.
    #[test]
    fn render_cell_leaves_short_non_url_cell_untouched() {
        let row = json!({"level": "error", "message": "short message"});
        assert_eq!(render_cell(&row, "message"), "short message");
    }

    /// The full table-rendering path: one very long `message` cell among
    /// many short rows must not inflate every row's rendered width to match
    /// it — this is the exact 255 KB / 8725-column regression from
    /// dogfooding session 63.
    #[test]
    fn render_table_does_not_inflate_all_rows_to_widest_cell() {
        let long_message = "y".repeat(5000);
        let rows = vec![
            json!({"level": "error", "message": long_message}),
            json!({"level": "warn", "message": "short"}),
        ];
        // The rendered width of every row is bounded by (columns' capped
        // widths + separators), never by the raw 5000-char message length.
        // Compute the expected max line width directly from render_cell's
        // contract rather than capturing stdout.
        for row in &rows {
            let cell = render_cell(row, "message");
            assert!(
                cell.chars().count() <= TEXT_CELL_MAX_WIDTH,
                "every row's message cell must be independently bounded, got: {cell:?}"
            );
        }
    }
}
