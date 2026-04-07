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
    /// If a jq filter is set, apply it to the full `{results, total, meta}`
    /// envelope so that users can access any envelope field (`.results`,
    /// `.total`, `.meta`, `.truncated`).  Use `.results[].url` to drill
    /// into array results or `.results.lcp_ms` for object results.
    /// Otherwise pretty-print the envelope as-is.
    pub fn finalize(&self, envelope: &Value) -> anyhow::Result<()> {
        match &self.jq_filter {
            Some(filter) => {
                let output = output::apply_jq_filter(envelope, filter)?;
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
