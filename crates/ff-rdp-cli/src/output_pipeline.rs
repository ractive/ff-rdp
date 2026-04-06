use serde_json::Value;

use crate::output;

pub struct OutputPipeline {
    jq_filter: Option<String>,
}

impl OutputPipeline {
    pub fn new(jq_filter: Option<String>) -> Self {
        Self { jq_filter }
    }

    /// Apply the pipeline to a JSON envelope and print to stdout.
    ///
    /// If a jq filter is set, apply it to the `.results` value extracted from
    /// the envelope so that users write `.[].url` rather than
    /// `.results[].url`. When the envelope has no `.results` key the filter
    /// falls back to the full envelope. Otherwise pretty-print the envelope
    /// as-is.
    pub fn finalize(&self, envelope: &Value) -> anyhow::Result<()> {
        match &self.jq_filter {
            Some(filter) => {
                // Auto-unwrap .results so callers write `.[].url` not `.results[].url`.
                let target = envelope.get("results").unwrap_or(envelope);
                let output = output::apply_jq_filter(target, filter)?;
                for value in output {
                    println!("{}", serde_json::to_string(&value)?);
                }
            }
            None => {
                println!("{}", serde_json::to_string_pretty(envelope)?);
            }
        }
        Ok(())
    }
}
