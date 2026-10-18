//! Per-command page emulation: `screenshot --color-scheme/--media` and
//! `navigate --user-agent`.
//!
//! The settings go through the tab watcher's target-configuration actor and
//! live exactly as long as the RDP connection that set them — Firefox restores
//! them when the actor is destroyed on disconnect. ff-rdp opens one connection
//! per command, so each setting covers only the command that applied it.

use ff_rdp_core::{ActorId, Grip, TargetConfiguration, TargetConfigurationActor};

use crate::error::AppError;

use super::connect_tab::ConnectedTab;
use super::js_helpers::{eval_or_bail, poll_js_condition};

/// How long to wait for the page's `matchMedia` to report a simulated media
/// feature. The parent process writes the override to the top-level browsing
/// context and the content process picks it up asynchronously.
const MEDIA_SETTLE_TIMEOUT_MS: u64 = 3000;

/// How long to wait for the CSS transitions and animations a media change
/// started to finish before capturing anyway (and reporting `settled: false`).
const ANIMATIONS_SETTLE_TIMEOUT_MS: u64 = 2000;

/// Reads the document timeline's current time, taken before the media change
/// so the settle wait can tell the animations it started from older ones.
const TIMELINE_NOW_JS: &str = "document.timeline.currentTime";

/// The JS predicate that is true once no finite CSS transition or animation
/// started at or after `since` (a `document.timeline` time; `None` waits on
/// every one) is running on the page.
///
/// Transitions start on the style flush after the media change, so the
/// predicate flushes style and layout before reading `getAnimations()` (which
/// flushes style itself as well). A just-created animation is still pending
/// (`startTime === null`) and counts as new. Infinite animations
/// (`iterations === Infinity`, e.g. a spinner) never finish and are ignored;
/// paused and finished ones do not change what the capture paints;
/// script-driven `Element.animate()` animations and animations that were
/// already running before the change were not started by it and are ignored
/// too, so a long page intro does not hold the capture.
fn animations_settled_js(since: Option<f64>) -> String {
    let since = since
        .filter(|t| t.is_finite())
        .map_or_else(|| "-Infinity".to_owned(), |t| t.to_string());
    format!(
        "(() => {{ const root = document.documentElement; \
         getComputedStyle(root).color; root.getBoundingClientRect(); \
         return !document.getAnimations().some(a => \
         (a instanceof CSSTransition || a instanceof CSSAnimation) && \
         a.playState === 'running' && (a.startTime === null || a.startTime >= {since}) && \
         a.effect !== null && a.effect.getComputedTiming().iterations !== Infinity); }})()"
    )
}

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

/// The page's `document.timeline` time, read before the media change and
/// passed to [`wait_for_animations`]. `None` when the page throws or the
/// timeline is inactive; the settle wait then considers every animation.
pub(crate) fn timeline_now(ctx: &mut ConnectedTab) -> Result<Option<f64>, AppError> {
    let console_actor = ctx.target().console_actor.clone();
    match eval_or_bail(ctx, &console_actor, TIMELINE_NOW_JS, "timeline") {
        Ok(result) => Ok(match result.result {
            Grip::Value(v) => v.as_f64(),
            _ => None,
        }),
        Err(AppError::User(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Wait until the CSS transitions and animations started since `since` (from
/// [`timeline_now`]) have finished, so the capture shows the settled page.
///
/// The wait is advisory: it returns `false` when they were still running after
/// [`ANIMATIONS_SETTLE_TIMEOUT_MS`], or when the page made the check throw
/// (e.g. a shim replaced `document.getAnimations`). The caller captures anyway
/// and reports it, rather than failing the command.
pub(crate) fn wait_for_animations(
    ctx: &mut ConnectedTab,
    since: Option<f64>,
) -> Result<bool, AppError> {
    let console_actor = ctx.target().console_actor.clone();
    let js = animations_settled_js(since);
    settled_from(poll_js_condition(
        ctx,
        &console_actor,
        &js,
        ANIMATIONS_SETTLE_TIMEOUT_MS,
        "emulation: checking the page's running animations failed",
        "emulation: CSS transitions still running",
    ))
}

/// Map the settle poll's outcome to `settled`: a timeout or an in-page
/// exception means "not known to be settled", only transport and protocol
/// failures fail the command.
fn settled_from(poll: Result<u64, AppError>) -> Result<bool, AppError> {
    match poll {
        Ok(_) => Ok(true),
        Err(AppError::Timeout(_) | AppError::User(_)) => Ok(false),
        Err(e) => Err(e),
    }
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

    #[test]
    fn animations_settled_js_ignores_infinite_and_non_css_animations() {
        let js = animations_settled_js(None);
        assert!(js.starts_with("(() =>") && js.ends_with("})()"), "{js}");
        assert!(js.contains("a.startTime >= -Infinity"), "{js}");
        // Flush style before reading the animations, or freshly started
        // transitions are not there yet.
        let flush = js.find("getComputedStyle(root)").unwrap();
        assert!(flush < js.find("document.getAnimations()").unwrap(), "{js}");
        assert!(
            js.contains("a instanceof CSSTransition || a instanceof CSSAnimation"),
            "{js}"
        );
        assert!(js.contains("a.playState === 'running'"), "{js}");
        assert!(js.contains("iterations !== Infinity"), "{js}");
        assert!(
            js.contains("return !document.getAnimations().some("),
            "{js}"
        );
    }

    #[test]
    fn animations_settled_js_waits_only_on_animations_since_the_change() {
        let js = animations_settled_js(Some(1234.5));
        // Pending (just-created) animations have no start time yet and count.
        assert!(
            js.contains("(a.startTime === null || a.startTime >= 1234.5)"),
            "{js}"
        );
        // A non-finite timeline time cannot be embedded; fall back to all.
        assert!(animations_settled_js(Some(f64::NAN)).contains(">= -Infinity"));
    }

    #[test]
    fn settled_from_is_advisory_except_for_transport_failures() {
        assert!(settled_from(Ok(120)).unwrap());
        assert!(!settled_from(Err(AppError::Timeout("t".into()))).unwrap());
        assert!(!settled_from(Err(AppError::User("page threw".into()))).unwrap());
        assert!(settled_from(Err(AppError::Connection("gone".into()))).is_err());
    }
}
