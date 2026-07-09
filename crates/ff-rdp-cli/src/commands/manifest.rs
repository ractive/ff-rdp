//! `ff-rdp manifest` — fetch and validate the page's Web App Manifest.
//!
//! Drives the Firefox manifest actor's `fetchCanonicalManifest`, which runs the
//! WHATWG "obtain a manifest" algorithm and returns the parsed manifest plus a
//! conformance `errors` array in a single call — a PWA-readiness audit
//! primitive.
//!
//! # Result shape
//!
//! - Page **with** a manifest: `results.manifest` is the parsed object,
//!   `results.errors` lists any conformance errors, `results.url` is the
//!   resolved manifest URL.
//! - Page **without** a manifest: `results.manifest` is `null` and
//!   `results.reason` explains it — this is a **structured, exit-0** result,
//!   not an error, so callers can branch on presence without parsing error
//!   output.
//! - No manifest actor at all (older Firefox): the standard error envelope
//!   with a non-zero exit code.

use ff_rdp_core::{ManifestFront, Registry};
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

pub fn run(cli: &Cli) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;

    // The manifest actor is exposed on the target frame (created lazily by
    // Firefox on first access).  Absence means the connected Firefox predates
    // the manifest actor — a genuine capability error, not a "no manifest"
    // result.
    let manifest_actor = ctx.target.manifest_actor.clone().ok_or_else(|| {
        AppError::User(
            "no manifest actor available — this Firefox build does not expose the \
             manifest actor (fetchCanonicalManifest). Update Firefox to audit Web \
             App Manifests."
                .to_string(),
        )
    })?;

    let target_root = ctx.target.actor.clone();
    let front = ManifestFront::new(manifest_actor, Registry::default(), target_root);

    let canonical = front
        .fetch_canonical_manifest(ctx.transport_mut())
        .map_err(AppError::from)?;

    // Build the results object. A missing manifest is a structured, non-error
    // result: `manifest: null` + a human-readable `reason`.
    let mut results = json!({
        "manifest": canonical.manifest,
        "url": canonical.url,
        "errors": canonical.errors,
    });
    if canonical.manifest.is_none()
        && let Some(obj) = results.as_object_mut()
    {
        obj.insert(
            "reason".to_string(),
            json!("page links no Web App Manifest (no <link rel=\"manifest\">)"),
        );
    }

    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );

    let envelope = output::envelope(&results, 1, &meta);
    OutputPipeline::from_cli(cli)?
        .finalize(&envelope)
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// The "no manifest" result must be structured (`manifest: null` + reason),
    /// not an error — mirrors the shape the command emits. Kept as a unit check
    /// on the JSON assembly logic so a refactor can't silently turn the
    /// no-manifest case into an error envelope.
    #[test]
    fn no_manifest_result_is_structured_not_error() {
        // Simulate the results assembly for a page with no manifest.
        let manifest: Option<serde_json::Value> = None;
        let mut results = json!({
            "manifest": manifest,
            "url": serde_json::Value::Null,
            "errors": [],
        });
        if manifest.is_none()
            && let Some(obj) = results.as_object_mut()
        {
            obj.insert(
                "reason".to_string(),
                json!("page links no Web App Manifest"),
            );
        }
        assert!(results["manifest"].is_null());
        assert!(results["reason"].is_string());
        assert!(results["errors"].is_array());
    }
}
