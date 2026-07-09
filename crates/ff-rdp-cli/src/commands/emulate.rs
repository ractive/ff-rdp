//! `ff-rdp emulate` — drive the Firefox `TargetConfigurationActor`.
//!
//! Exposes the page-environment emulation surface of the target-configuration
//! actor: custom user agent, `prefers-color-scheme` simulation, device-pixel-
//! ratio override, print-media simulation, touch-event override, JavaScript-
//! disabled testing, tab-offline (PWA/offline UX), and cache-disabled
//! (cold-load perf).  Each flag maps to one nullable field of the server's
//! `SUPPORTED_OPTIONS` dict; only the flags the user passes are sent, so a call
//! patches the live configuration rather than replacing it.
//!
//! # Lifetime
//!
//! Configuration lives as long as the RDP connection that set it.  Under the
//! daemon that means "until the daemon restarts"; with `--no-daemon` the
//! one-shot process disconnects immediately after the call, so the setting dies
//! with it.  In that case the envelope carries a `lifetime_warning` so scripts
//! are not misled into thinking the emulation persists.

use ff_rdp_core::{Registry, TabActor, TargetConfigurationFront, WatcherFront};
use serde_json::{Value, json};

use crate::cli::args::{Cli, ColorScheme, EmulateArgs, OnOff};
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

/// Warning attached to the envelope when the emulation was applied over a
/// one-shot (`--no-daemon`) connection that disconnects immediately, dropping
/// the configuration on the floor.
pub(crate) const ONE_SHOT_LIFETIME_WARNING: &str = "emulation lifetime: this one-shot connection only — with --no-daemon the \
     configuration is discarded when this process disconnects; start the daemon \
     (drop --no-daemon) to keep emulation active across commands";

/// Build the `configuration` patch (applied fields) from the parsed args.
///
/// Returns the JSON object that mirrors what was sent to the actor, so the
/// command can echo the *applied* configuration back to the caller. `--reset`
/// is handled separately (it sends every default) and never combines with the
/// per-field flags at this layer.
fn build_applied(args: &EmulateArgs) -> Value {
    let mut applied = serde_json::Map::new();

    if let Some(ua) = &args.user_agent {
        applied.insert("customUserAgent".to_owned(), json!(ua));
    }
    if let Some(scheme) = args.color_scheme {
        applied.insert("colorSchemeSimulation".to_owned(), json!(scheme.as_wire()));
    }
    if let Some(dppx) = args.dppx {
        applied.insert("overrideDPPX".to_owned(), json!(dppx));
    }
    if let Some(print) = args.print {
        applied.insert("printSimulationEnabled".to_owned(), json!(print.is_on()));
    }
    if let Some(touch) = args.touch {
        // The wire value is a string enum ("enabled"/"none"); echo the same
        // bool-to-enum mapping the front uses so the envelope is truthful.
        let wire = if touch.is_on() { "enabled" } else { "none" };
        applied.insert("touchEventsOverride".to_owned(), json!(wire));
    }
    if let Some(js) = args.js {
        applied.insert("javascriptEnabled".to_owned(), json!(js.is_on()));
    }
    if let Some(offline) = args.offline {
        applied.insert("setTabOffline".to_owned(), json!(offline.is_on()));
    }
    if let Some(cache) = args.cache {
        // `--cache on|off` toggles the cache; the wire field is inverted
        // (`cacheDisabled`). `--cache off` means "disable the cache".
        applied.insert("cacheDisabled".to_owned(), json!(!cache.is_on()));
    }

    Value::Object(applied)
}

/// Return true if none of the per-field flags were supplied.
fn no_fields_set(args: &EmulateArgs) -> bool {
    args.user_agent.is_none()
        && args.color_scheme.is_none()
        && args.dppx.is_none()
        && args.print.is_none()
        && args.touch.is_none()
        && args.js.is_none()
        && args.offline.is_none()
        && args.cache.is_none()
}

pub fn run(cli: &Cli, args: &EmulateArgs) -> Result<(), AppError> {
    // Argument validation before opening any connection.
    if args.reset && !no_fields_set(args) {
        return Err(AppError::User(
            "emulate: --reset cannot be combined with other emulation flags — \
             run --reset on its own, then set the fields you want"
                .to_owned(),
        ));
    }
    if !args.reset && no_fields_set(args) {
        return Err(AppError::User(
            "emulate: no configuration flags given — pass at least one of \
             --user-agent/--color-scheme/--dppx/--print/--touch/--js/--offline/--cache, \
             or --reset to restore defaults"
                .to_owned(),
        ));
    }
    if let Some(dppx) = args.dppx
        && (!dppx.is_finite() || dppx <= 0.0)
    {
        return Err(AppError::User(format!(
            "emulate: --dppx must be a positive, finite number (got {dppx})"
        )));
    }

    let mut ctx = connect_and_get_target(cli)?;
    let via_daemon = ctx.via_daemon;
    let tab_actor = ctx.target_tab_actor().clone();

    // Resolve watcher → target-configuration actor. The plumbing mirrors the
    // live coverage in ff-rdp-core/tests/live_61u.rs.
    let watcher_actor =
        TabActor::get_watcher(ctx.transport_mut(), &tab_actor).map_err(AppError::from)?;
    let watcher_front = WatcherFront::new(
        watcher_actor.clone(),
        Registry::default(),
        Some(watcher_actor.clone()),
    );
    let config_actor = watcher_front
        .get_target_configuration_actor(ctx.transport_mut())
        .map_err(AppError::from)?;
    let config_front =
        TargetConfigurationFront::new(config_actor, Registry::default(), watcher_actor);

    // Apply the configuration. `--reset` sends every documented default in one
    // request; otherwise we patch only the fields the user named.
    let applied = if args.reset {
        config_front
            .reset(ctx.transport_mut())
            .map_err(AppError::from)?;
        // Echo the reset values the front sends (kept in sync with
        // TargetConfigurationFront::reset).
        json!({
            "cacheDisabled": false,
            "colorSchemeSimulation": "none",
            "customUserAgent": "",
            "overrideDPPX": 0,
            "printSimulationEnabled": false,
            "touchEventsOverride": "none",
            "javascriptEnabled": true,
            "setTabOffline": false,
        })
    } else {
        apply_fields(&config_front, ctx.transport_mut(), args)?;
        build_applied(args)
    };

    // Whether the JS-enabled flag changed to `false`, which makes Firefox
    // reload the document — surface a note so callers know a reload happened
    // server-side (probes must account for it).
    let js_reload = matches!(args.js, Some(OnOff::Off)) || (args.reset);

    let mut results = json!({
        "applied": applied,
        "reset": args.reset,
    });
    if let Some(obj) = results.as_object_mut() {
        if !via_daemon {
            obj.insert(
                "lifetime_warning".to_owned(),
                json!(ONE_SHOT_LIFETIME_WARNING),
            );
        }
        if js_reload {
            obj.insert(
                "note".to_owned(),
                json!(
                    "javascriptEnabled changed — Firefox reloads the document \
                     server-side; reload/re-probe to observe the effect"
                ),
            );
        }
    }

    // Keep the default envelope lean: connection metadata (including
    // `via_daemon`) is only attached under --verbose, matching the other
    // commands. The one-shot lifetime signal lives in `results.lifetime_warning`.
    let mut meta = if cli.is_verbose() {
        json!({ "via_daemon": via_daemon })
    } else {
        json!({})
    };
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

/// Send each user-specified field to the actor. Kept separate from
/// [`build_applied`] so the wire call and the echoed JSON are driven by the
/// same set of flags without duplicating the branching in one giant function.
fn apply_fields(
    config_front: &TargetConfigurationFront,
    transport: &mut ff_rdp_core::RdpTransport,
    args: &EmulateArgs,
) -> Result<(), AppError> {
    if let Some(ua) = &args.user_agent {
        config_front
            .set_custom_user_agent(transport, ua)
            .map_err(AppError::from)?;
    }
    if let Some(scheme) = args.color_scheme {
        config_front
            .set_color_scheme_simulation(transport, scheme.as_wire())
            .map_err(AppError::from)?;
    }
    if let Some(dppx) = args.dppx {
        config_front
            .set_override_dppx(transport, dppx)
            .map_err(AppError::from)?;
    }
    if let Some(print) = args.print {
        config_front
            .set_print_simulation_enabled(transport, print.is_on())
            .map_err(AppError::from)?;
    }
    if let Some(touch) = args.touch {
        config_front
            .set_touch_events_override(transport, touch.is_on())
            .map_err(AppError::from)?;
    }
    if let Some(js) = args.js {
        config_front
            .set_javascript_enabled(transport, js.is_on())
            .map_err(AppError::from)?;
    }
    if let Some(offline) = args.offline {
        config_front
            .set_tab_offline(transport, offline.is_on())
            .map_err(AppError::from)?;
    }
    if let Some(cache) = args.cache {
        // `--cache off` disables the cache (cacheDisabled = true).
        config_front
            .set_cache_disabled(transport, !cache.is_on())
            .map_err(AppError::from)?;
    }
    Ok(())
}

/// Map a `ColorScheme` clap enum to its wire value.
impl ColorScheme {
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            ColorScheme::Light => "light",
            ColorScheme::Dark => "dark",
            ColorScheme::None => "none",
        }
    }
}

/// `--flag on|off` helper.
impl OnOff {
    pub(crate) fn is_on(self) -> bool {
        matches!(self, OnOff::On)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> EmulateArgs {
        EmulateArgs {
            user_agent: None,
            color_scheme: None,
            dppx: None,
            print: None,
            touch: None,
            js: None,
            offline: None,
            cache: None,
            reset: false,
        }
    }

    #[test]
    fn no_fields_set_detects_empty() {
        assert!(no_fields_set(&base_args()));
    }

    #[test]
    fn no_fields_set_false_when_any_flag() {
        let mut a = base_args();
        a.color_scheme = Some(ColorScheme::Dark);
        assert!(!no_fields_set(&a));
    }

    #[test]
    fn build_applied_maps_color_scheme_wire_name() {
        let mut a = base_args();
        a.color_scheme = Some(ColorScheme::Dark);
        let applied = build_applied(&a);
        assert_eq!(applied["colorSchemeSimulation"], "dark");
        // Only the set field is present.
        assert!(applied.get("customUserAgent").is_none());
    }

    #[test]
    fn build_applied_user_agent() {
        let mut a = base_args();
        a.user_agent = Some("ff-rdp-test/1.0".to_owned());
        let applied = build_applied(&a);
        assert_eq!(applied["customUserAgent"], "ff-rdp-test/1.0");
    }

    #[test]
    fn build_applied_dppx() {
        let mut a = base_args();
        a.dppx = Some(2.0);
        let applied = build_applied(&a);
        assert_eq!(applied["overrideDPPX"], 2.0);
    }

    #[test]
    fn build_applied_touch_maps_to_enum() {
        let mut a = base_args();
        a.touch = Some(OnOff::On);
        assert_eq!(build_applied(&a)["touchEventsOverride"], "enabled");
        a.touch = Some(OnOff::Off);
        assert_eq!(build_applied(&a)["touchEventsOverride"], "none");
    }

    #[test]
    fn build_applied_cache_off_disables_cache() {
        let mut a = base_args();
        // `--cache off` -> cacheDisabled: true (cache is turned off).
        a.cache = Some(OnOff::Off);
        assert_eq!(build_applied(&a)["cacheDisabled"], true);
        a.cache = Some(OnOff::On);
        assert_eq!(build_applied(&a)["cacheDisabled"], false);
    }

    #[test]
    fn build_applied_js_and_offline() {
        let mut a = base_args();
        a.js = Some(OnOff::Off);
        a.offline = Some(OnOff::On);
        let applied = build_applied(&a);
        assert_eq!(applied["javascriptEnabled"], false);
        assert_eq!(applied["setTabOffline"], true);
    }

    #[test]
    fn print_on_maps_to_true() {
        let mut a = base_args();
        a.print = Some(OnOff::On);
        assert_eq!(build_applied(&a)["printSimulationEnabled"], true);
    }

    #[test]
    fn color_scheme_wire_values() {
        assert_eq!(ColorScheme::Light.as_wire(), "light");
        assert_eq!(ColorScheme::Dark.as_wire(), "dark");
        assert_eq!(ColorScheme::None.as_wire(), "none");
    }

    #[test]
    fn onoff_is_on() {
        assert!(OnOff::On.is_on());
        assert!(!OnOff::Off.is_on());
    }
}
