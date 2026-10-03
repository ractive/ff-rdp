//! Per-command page emulation: `screenshot --color-scheme/--media` and
//! `navigate --user-agent`.
//!
//! The settings go through the tab watcher's target-configuration actor and
//! live exactly as long as the RDP connection that set them — Firefox restores
//! them when the actor is destroyed on disconnect. ff-rdp opens one connection
//! per command, so each setting covers only the command that applied it.

use ff_rdp_core::{ActorId, TargetConfiguration, TargetConfigurationActor};

use crate::error::AppError;

use super::connect_tab::ConnectedTab;
use super::js_helpers::poll_js_condition;

/// How long to wait for the page's `matchMedia` to report a simulated media
/// feature. The parent process writes the override to the top-level browsing
/// context and the content process picks it up asynchronously.
const MEDIA_SETTLE_TIMEOUT_MS: u64 = 3000;

/// Apply `configuration` on `watcher`'s target-configuration actor.
///
/// `watcher` must be the watcher the command already uses (a tab descriptor
/// creates one watcher per connection, with the options of the first
/// `getWatcher` call).
pub(crate) fn apply(
    ctx: &mut ConnectedTab,
    watcher: &ActorId,
    configuration: &TargetConfiguration,
) -> Result<(), AppError> {
    let actor = TargetConfigurationActor::for_watcher(ctx.transport_mut(), watcher)
        .map_err(AppError::from)?;
    TargetConfigurationActor::update_configuration(ctx.transport_mut(), &actor, configuration)
        .map_err(AppError::from)?;
    Ok(())
}

/// The JS predicate that is true once the page reports every simulated media
/// feature in `configuration`, or `None` when nothing media-related was set.
///
/// Reading `matchMedia` flushes the pending style change, and the trailing
/// `getBoundingClientRect` flushes layout, so the next capture paints the
/// emulated styles.
fn media_predicate(configuration: &TargetConfiguration) -> Option<String> {
    let mut queries = Vec::new();
    if let Some(scheme) = configuration.color_scheme_simulation.as_deref() {
        queries.push(format!("(prefers-color-scheme: {scheme})"));
    }
    if configuration.print_simulation_enabled == Some(true) {
        queries.push("print".to_owned());
    }
    if queries.is_empty() {
        return None;
    }
    let checks = queries
        .iter()
        .map(|q| {
            format!(
                "matchMedia({}).matches",
                serde_json::Value::from(q.as_str())
            )
        })
        .collect::<Vec<_>>()
        .join(" && ");
    Some(format!(
        "(() => {{ const ok = {checks}; \
         document.documentElement.getBoundingClientRect(); return ok; }})()"
    ))
}

/// Wait until the page's `matchMedia` reports the simulated media features in
/// `configuration`. A no-op when none were set.
pub(crate) fn wait_for_media(
    ctx: &mut ConnectedTab,
    configuration: &TargetConfiguration,
) -> Result<(), AppError> {
    let Some(js) = media_predicate(configuration) else {
        return Ok(());
    };
    let console_actor = ctx.target().console_actor.clone();
    poll_js_condition(
        ctx,
        &console_actor,
        &js,
        MEDIA_SETTLE_TIMEOUT_MS,
        "emulation: checking the simulated media failed",
        "emulation: the page did not report the simulated media within 3000ms",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_predicate_none_without_media_settings() {
        let ua_only = TargetConfiguration {
            custom_user_agent: Some("UA".into()),
            ..Default::default()
        };
        assert_eq!(media_predicate(&ua_only), None);
        let screen = TargetConfiguration {
            print_simulation_enabled: Some(false),
            ..Default::default()
        };
        assert_eq!(media_predicate(&screen), None);
    }

    #[test]
    fn media_predicate_checks_every_simulated_feature() {
        let both = TargetConfiguration {
            color_scheme_simulation: Some("dark".into()),
            print_simulation_enabled: Some(true),
            ..Default::default()
        };
        let js = media_predicate(&both).unwrap();
        assert!(
            js.contains(r#"matchMedia("(prefers-color-scheme: dark)").matches"#),
            "{js}"
        );
        assert!(js.contains(r#"matchMedia("print").matches"#), "{js}");
        assert!(js.contains(" && "), "{js}");
    }
}
