//! `ff-rdp addon install|list|reload|uninstall`: temporary WebExtensions in a
//! Firefox that is already running — the about:debugging "Load Temporary
//! Add-on", Reload and Remove buttons over RDP.
//!
//! `install` and `launch --temporary-addon` share [`resolve_path`] and
//! [`install`], so both check paths and report installs the same way.
//! `reload` sends the WebExtension descriptor's `reload`, as about:debugging
//! does (`devtools/client/aboutdebugging/src/actions/debug-targets.js:161-175`);
//! for a temporary add-on Firefox re-installs it from its own source in place
//! (`XPIDatabase.sys.mjs:1505-1519`), so open tabs stay open.

use std::path::Path;

use ff_rdp_core::{
    ActorErrorKind, AddonDescriptor, AddonsActor, ProtocolError, RdpTransport, RootActor,
    WebExtensionDescriptorActor,
};
use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_controls::{OutputControls, SortDir};
use crate::output_pipeline::OutputPipeline;

/// An add-on path checked to exist, as given and as sent to Firefox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddonPath {
    /// The path as the user typed it (named in errors).
    pub(crate) given: String,
    /// Absolute, canonical native path handed to `installTemporaryAddon` and
    /// reported as `path`.
    pub(crate) absolute: String,
}

/// Check an add-on path before anything talks to Firefox: it must exist, and
/// a directory must hold a `manifest.json`. Anything else (a malformed
/// manifest, a file that is not an extension) is Firefox's call. `label`
/// names the flag or command in errors (`--temporary-addon`, `addon install`).
pub(crate) fn resolve_path(path: &Path, label: &str) -> Result<AddonPath, AppError> {
    let given = path.display().to_string();
    let canonical = path.canonicalize().map_err(|e| {
        AppError::User(format!(
            "{label} {given}: cannot read path ({e}); expected an extension \
             directory containing manifest.json or an .xpi file"
        ))
    })?;
    if canonical.is_dir() && !canonical.join("manifest.json").is_file() {
        return Err(AppError::User(format!(
            "{label} {given}: directory has no manifest.json; expected an \
             extension directory containing manifest.json or an .xpi file"
        )));
    }
    let absolute = strip_verbatim_prefix(&canonical.to_string_lossy());
    Ok(AddonPath { given, absolute })
}

/// `canonicalize` on Windows returns `\\?\C:\…` / `\\?\UNC\server\share\…`
/// verbatim paths. Firefox's `nsIFile` expects an ordinary native path, so
/// turn them back into `C:\…` and `\\server\share\…`. A no-op elsewhere.
fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        path.to_owned()
    }
}

/// Install `addon` as a temporary add-on and describe it as
/// `{path, id, actor}`. A rejected install keeps Firefox's message verbatim
/// (it names the problem, e.g. `Reading manifest: Property "version" is
/// required`); transport-level failures keep their own typed error.
pub(crate) fn install(
    transport: &mut RdpTransport,
    addons_actor: &ff_rdp_core::ActorId,
    addon: &AddonPath,
    label: &str,
) -> Result<Value, AppError> {
    let reply = AddonsActor::install_temporary_addon(transport, addons_actor, &addon.absolute)
        .map_err(|error| install_error(label, &addon.given, error))?;
    Ok(json!({
        "path": addon.absolute,
        "id": reply.id,
        // `false` on every Firefox to date (addons.js:75): the install reply
        // carries no actor. Passed through rather than invented.
        "actor": reply.actor,
    }))
}

fn install_error(label: &str, given: &str, error: ProtocolError) -> AppError {
    match error {
        ProtocolError::ActorError { message, .. } => AppError::User(format!(
            "{label} {given}: Firefox rejected the temporary install: {message}"
        )),
        other => AppError::from(other),
    }
}

/// `ff-rdp addon install <PATH>`.
pub(crate) fn run_install(cli: &Cli, path: &Path) -> Result<(), AppError> {
    let addon = resolve_path(path, "addon install")?;
    let mut connection = crate::commands::connect_tab::connect_browser(cli)?;
    let transport = connection.transport_mut();
    let addons_actor = AddonsActor::from_root(transport)?;
    let installed = install(transport, &addons_actor, &addon, "addon install")?;
    emit(cli, &installed, 1)
}

/// `ff-rdp addon list`: what about:debugging's Extensions section shows —
/// debuggable, not hidden (`debug-targets.js:244-250`) — unless
/// `include_hidden`, which lists everything `listAddons` returns.
pub(crate) fn run_list(cli: &Cli, include_hidden: bool) -> Result<(), AppError> {
    let mut connection = crate::commands::connect_tab::connect_browser(cli)?;
    let addons = RootActor::list_addons(connection.transport_mut())?;
    let mut items: Vec<Value> = addons
        .iter()
        .filter(|a| include_hidden || shown_by_default(a))
        .map(describe)
        .collect();
    let controls = OutputControls::from_cli(cli, SortDir::Asc);
    controls.apply_sort(&mut items)?;
    controls.validate_fields(&items)?;
    let (limited, total, truncated) = controls.apply_limit(items, None);
    let shown = limited.len();
    let limited = controls.apply_fields(limited);
    let envelope =
        output::envelope_with_truncation(&json!(limited), shown, total, truncated, &meta(cli));
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

/// about:debugging's filter: `a.debuggable && !a.hidden`.
fn shown_by_default(addon: &AddonDescriptor) -> bool {
    addon.debuggable == Some(true) && addon.hidden != Some(true)
}

/// One `addon list` entry. Firefox's form has no version field.
fn describe(addon: &AddonDescriptor) -> Value {
    json!({
        "id": addon.id,
        "name": addon.name,
        "temporarily_installed": addon.temporarily_installed,
        "uuid": addon.uuid(),
        "url": addon.url,
        "manifest_url": addon.manifest_url,
        "debuggable": addon.debuggable,
        "hidden": addon.hidden,
        "system": addon.is_system,
        "warnings": addon.warnings,
        "actor": addon.actor.as_ref(),
    })
}

/// What `addon reload` was pointed at.
#[derive(Debug, PartialEq, Eq)]
enum ReloadTarget {
    Id(String),
    Path(AddonPath),
}

/// A path when it exists on disk or reads like one (a separator, `.xpi`);
/// otherwise an add-on id. Gecko ids (`name@domain`, `{GUID}`) contain
/// neither.
fn reload_target(arg: &str) -> Result<ReloadTarget, AppError> {
    let path = Path::new(arg);
    let path_like = arg.contains('/')
        || arg.contains('\\')
        || path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xpi"))
        || matches!(arg, "." | "..");
    if path_like || path.exists() {
        resolve_path(path, "addon reload").map(ReloadTarget::Path)
    } else {
        Ok(ReloadTarget::Id(arg.to_owned()))
    }
}

/// The add-on's source as a local path: its `file:` URL, canonicalised when
/// it exists here, so it compares equal to a resolved [`AddonPath`].
fn source_path(addon: &AddonDescriptor) -> Option<String> {
    let path = url::Url::parse(addon.url.as_deref()?)
        .ok()?
        .to_file_path()
        .ok()?;
    let path = path.canonicalize().unwrap_or(path);
    Some(strip_verbatim_prefix(&path.to_string_lossy()))
}

fn find_target<'a>(
    addons: &'a [AddonDescriptor],
    target: &ReloadTarget,
) -> Result<&'a AddonDescriptor, AppError> {
    match target {
        ReloadTarget::Id(id) => addons.iter().find(|a| &a.id == id).ok_or_else(|| {
            AppError::User(format!(
                "addon reload {id}: Firefox has no add-on with this id; \
                 `ff-rdp addon list` shows the installed ones"
            ))
        }),
        ReloadTarget::Path(path) => addons
            .iter()
            .find(|a| source_path(a).as_deref() == Some(path.absolute.as_str()))
            .ok_or_else(|| {
                AppError::User(format!(
                    "addon reload {}: no add-on is installed from {}; install it first \
                     with `ff-rdp addon install {}`",
                    path.given, path.absolute, path.given
                ))
            }),
    }
}

/// `ff-rdp addon reload <ID|PATH>`.
pub(crate) fn run_reload(cli: &Cli, arg: &str) -> Result<(), AppError> {
    let target = reload_target(arg)?;
    let mut connection = crate::commands::connect_tab::connect_browser(cli)?;
    let transport = connection.transport_mut();
    let addons = RootActor::list_addons(transport)?;
    let addon = find_target(&addons, &target)?;
    let previous_uuid = addon.uuid().map(str::to_owned);
    WebExtensionDescriptorActor::reload(transport, &addon.actor).map_err(|error| match error {
        ProtocolError::ActorError { message, .. } => {
            AppError::User(reload_error(arg, addon, &message))
        }
        other => AppError::from(other),
    })?;
    // Read the add-on back: its UUID (the moz-extension:// host) after the
    // reload, next to the one before it.
    let after = RootActor::list_addons(transport)?;
    let reloaded = after.iter().find(|a| a.id == addon.id);
    let result = json!({
        "id": addon.id,
        "name": addon.name,
        "url": addon.url,
        "temporarily_installed": addon.temporarily_installed,
        "uuid": reloaded.and_then(AddonDescriptor::uuid),
        "previous_uuid": previous_uuid,
    });
    emit(cli, &result, 1)
}

/// Firefox's reload error is only `Extension is invalid` (the descriptor does
/// not append the manifest details `installTemporaryAddon` does), so point at
/// the command that names the problem.
fn reload_error(arg: &str, addon: &AddonDescriptor, message: &str) -> String {
    let detail = match source_path(addon) {
        Some(path) => format!(
            "; the running version stays installed, and `ff-rdp addon install {path}` names \
             the manifest problem"
        ),
        None => "; the running version stays installed".to_owned(),
    };
    format!(
        "addon reload {arg}: Firefox could not reload {}: {message}{detail}",
        addon.id
    )
}

/// `ff-rdp addon uninstall <ID>`.
pub(crate) fn run_uninstall(cli: &Cli, id: &str) -> Result<(), AppError> {
    let mut connection = crate::commands::connect_tab::connect_browser(cli)?;
    let transport = connection.transport_mut();
    let addons_actor = AddonsActor::from_root(transport)?;
    match AddonsActor::uninstall_addon(transport, &addons_actor, id) {
        Ok(()) => emit(cli, &json!({"id": id, "uninstalled": true}), 1),
        Err(ProtocolError::ActorError {
            kind: ActorErrorKind::UnrecognizedPacketType,
            ..
        }) => Err(AppError::User(format!(
            "addon uninstall {id}: this Firefox has no uninstallAddon request (Firefox 113+)"
        ))),
        Err(ProtocolError::ActorError { message, .. }) => {
            // Firefox says the same thing for an unknown id and for an add-on
            // that is not temporary (addons.js:83-85); the list tells which.
            let known = RootActor::list_addons(transport)
                .ok()
                .and_then(|all| all.into_iter().find(|a| a.id == id));
            Err(AppError::User(uninstall_refusal(
                id,
                &message,
                known.as_ref(),
            )))
        }
        Err(other) => Err(AppError::from(other)),
    }
}

fn uninstall_refusal(id: &str, message: &str, known: Option<&AddonDescriptor>) -> String {
    let why = match known {
        None => "Firefox has no add-on with this id; `ff-rdp addon list` shows the installed ones",
        Some(a) if a.temporarily_installed != Some(true) => {
            "the add-on is not temporarily installed, and Firefox uninstalls only temporary \
             add-ons over RDP"
        }
        Some(_) => "Firefox refused to uninstall it",
    };
    format!("addon uninstall {id}: {why} ({message})")
}

fn meta(cli: &Cli) -> Value {
    let mut meta = json!({});
    crate::connection_meta::merge_into_if_verbose(
        &mut meta,
        &cli.host,
        cli.port,
        None,
        cli.is_verbose(),
    );
    meta
}

fn emit(cli: &Cli, result: &Value, total: usize) -> Result<(), AppError> {
    let envelope = output::envelope(result, total, &meta(cli));
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(value: Value) -> AddonDescriptor {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn verbatim_prefixes_are_stripped() {
        assert_eq!(strip_verbatim_prefix(r"\\?\C:\ext"), r"C:\ext");
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\ext"),
            r"\\server\share\ext"
        );
        assert_eq!(strip_verbatim_prefix("/home/u/ext"), "/home/u/ext");
    }

    #[test]
    fn resolve_accepts_extension_dir_and_file_with_absolute_paths() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("ext");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), "{}").unwrap();
        let xpi = root.path().join("ext.xpi");
        std::fs::write(&xpi, b"PK").unwrap();
        for path in [&dir, &xpi] {
            let addon = resolve_path(path, "addon install").unwrap();
            assert_eq!(addon.given, path.display().to_string());
            assert!(Path::new(&addon.absolute).is_absolute(), "{addon:?}");
            assert!(!addon.absolute.starts_with(r"\\?\"), "{addon:?}");
        }
    }

    #[test]
    fn resolve_rejects_missing_path_and_dir_without_manifest_naming_the_label() {
        let root = tempfile::tempdir().unwrap();
        let err = resolve_path(&root.path().join("nope"), "addon install")
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("addon install ") && err.contains("cannot read path"),
            "{err}"
        );
        let err = resolve_path(root.path(), "--temporary-addon")
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("--temporary-addon ") && err.contains("no manifest.json"),
            "{err}"
        );
    }

    #[test]
    fn rejected_install_keeps_firefox_message() {
        let err = install_error(
            "addon install",
            "./bad",
            ProtocolError::ActorError {
                actor: "server1.conn0.addonsActor2".to_owned(),
                kind: ActorErrorKind::Other("Error".to_owned()),
                error: "Error".to_owned(),
                message: "Could not install add-on at '/abs/bad': Error: Extension is invalid"
                    .to_owned(),
            },
        );
        let text = err.to_string();
        assert!(
            text.starts_with("addon install ./bad: Firefox rejected"),
            "{text}"
        );
        assert!(text.contains("Extension is invalid"), "{text}");
    }

    #[test]
    fn reload_target_tells_ids_from_paths() {
        assert_eq!(
            reload_target("temporary-addon-fixture@ff-rdp.test").unwrap(),
            ReloadTarget::Id("temporary-addon-fixture@ff-rdp.test".to_owned())
        );
        assert_eq!(
            reload_target("{8c4d6f1e-0000-4000-8000-000000000000}").unwrap(),
            ReloadTarget::Id("{8c4d6f1e-0000-4000-8000-000000000000}".to_owned())
        );
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("manifest.json"), "{}").unwrap();
        let arg = root.path().to_str().unwrap();
        assert!(matches!(
            reload_target(arg).unwrap(),
            ReloadTarget::Path(AddonPath { .. })
        ));
        let missing = root.path().join("gone");
        let err = reload_target(missing.to_str().unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("addon reload ") && err.contains("cannot read path"),
            "{err}"
        );
    }

    #[test]
    fn default_list_matches_about_debugging_filter() {
        let temp = form(json!({"actor": "a1", "id": "t@x", "debuggable": true, "hidden": false}));
        let system = form(json!({"actor": "a2", "id": "s@x", "debuggable": true, "hidden": true}));
        let plugin = form(json!({"actor": "a3", "id": "gmp-widevinecdm"}));
        assert!(shown_by_default(&temp));
        assert!(!shown_by_default(&system));
        assert!(!shown_by_default(&plugin));
    }

    #[test]
    fn path_target_matches_the_add_on_installed_from_it() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("manifest.json"), "{}").unwrap();
        let path = resolve_path(root.path(), "addon reload").unwrap();
        let url = url::Url::from_directory_path(root.path()).unwrap();
        let addons = [
            form(json!({"actor": "a1", "id": "other@x", "url": "file:///elsewhere/"})),
            form(json!({"actor": "a2", "id": "mine@x", "url": url.as_str()})),
        ];
        let found = find_target(&addons, &ReloadTarget::Path(path)).unwrap();
        assert_eq!(found.id, "mine@x");
        let err = find_target(&addons, &ReloadTarget::Id("nope@x".to_owned()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no add-on with this id"), "{err}");
    }

    #[test]
    fn reload_error_points_at_install_for_the_details() {
        let root = tempfile::tempdir().unwrap();
        let url = url::Url::from_directory_path(root.path()).unwrap();
        let addon = form(json!({"actor": "a", "id": "x@y", "url": url.as_str()}));
        let text = reload_error("x@y", &addon, "Extension is invalid");
        assert!(
            text.starts_with(
                "addon reload x@y: Firefox could not reload x@y: Extension is invalid"
            ),
            "{text}"
        );
        assert!(text.contains("`ff-rdp addon install "), "{text}");
        let no_source = form(json!({"actor": "a", "id": "x@y"}));
        assert!(!reload_error("x@y", &no_source, "boom").contains("addon install"));
    }

    #[test]
    fn uninstall_refusal_says_why() {
        let msg = "Could not uninstall add-on \"x\"";
        assert!(uninstall_refusal("x", msg, None).contains("no add-on with this id"));
        let installed = form(json!({"actor": "a", "id": "x", "temporarilyInstalled": false}));
        assert!(
            uninstall_refusal("x", msg, Some(&installed)).contains("not temporarily installed")
        );
    }
}
