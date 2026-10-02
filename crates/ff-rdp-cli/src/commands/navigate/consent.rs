use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::commands::connect_tab::connect_and_get_target;

/// Run the iter-129 CMP-detection-and-accept flow and merge its result into
/// `result["consent"]`.
///
/// Best-effort by design (see `--auto-consent`'s long_about): a fresh
/// connection is opened (the one `run_core` used has already been dropped)
/// and any failure — connection or protocol — is reported as a stderr
/// warning plus `{"cmp": null, "action": null}` rather than failing the
/// navigate itself. The keys are always present either way, matching
/// `consent accept`'s always-present-key discipline.
pub(crate) fn merge_auto_consent(cli: &Cli, result: &mut Value) {
    let consent = detect_and_accept_best_effort(cli);
    if let Some(obj) = result.as_object_mut() {
        obj.insert("consent".to_owned(), consent);
    }
}

/// Run the CMP-detection-and-accept flow on its own connection and return the
/// `{cmp, action}` object, never failing the caller.
///
/// Both keys are always present, matching `consent accept`'s discipline; a
/// connection or protocol failure becomes a stderr warning plus two nulls.
///
/// Only for plain `navigate`, whose `run_core` connection is already dropped by
/// the time this runs. `--with-network` must reuse its live connection instead
/// — see [`detect_and_accept_on`].
pub(crate) fn detect_and_accept_best_effort(cli: &Cli) -> Value {
    match connect_and_get_target(cli)
        .and_then(|mut ctx| crate::commands::consent::detect_and_accept(&mut ctx))
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("warning: --auto-consent: consent detection failed: {e}"); // stderr-ok: (b) warn-and-continue — best-effort by design, see the doc comment above
            consent_failure_value()
        }
    }
}

/// The `{cmp, action, status}` object a *failed* consent pass reports on the
/// `--auto-consent` paths (iter-160 Theme D).
///
/// The three keys must be present here too — `--jq '.results.consent.status'`
/// has to work whether the pass ran or the connection dropped — and the status
/// must come from the same vocabulary as a successful pass, not a fourth word
/// invented on the error path. `no_cmp_detected` is the honest reading: nothing
/// was detected. The reason it was not detected is the stderr warning the
/// caller already gets.
pub(crate) fn consent_failure_value() -> Value {
    json!({"cmp": null, "action": null, "status": "no_cmp_detected"})
}

/// [`detect_and_accept_best_effort`] on an **existing** connection.
///
/// iter-159: `--with-network` runs the consent step on its own connection so
/// the resource subscription stays live across the interaction, and the
/// requests the banner dismissal unblocks are still captured.
pub(crate) fn detect_and_accept_on(ctx: &mut crate::commands::connect_tab::ConnectedTab) -> Value {
    match crate::commands::consent::detect_and_accept(ctx) {
        Ok(v) => v,
        Err(e) => {
            // stderr-ok: (b) warn-and-continue — best-effort, same contract as
            // `detect_and_accept_best_effort` above.
            eprintln!("warning: --auto-consent: consent detection failed: {e}");
            consent_failure_value()
        }
    }
}
