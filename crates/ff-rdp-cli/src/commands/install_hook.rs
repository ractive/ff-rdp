//! `ff-rdp install-hook` — opt-in session-start hook that prints the home view
//! (iter-212 Theme B).
//!
//! A skill loads on demand and still leaves an agent one `--help` turn short of
//! knowing whether a browser is up. A `SessionStart` hook removes that turn
//! entirely: the agent's very first context already contains
//! [`crate::commands::home`]'s live state and its `-> ff-rdp …` next steps.
//!
//! The file this writes is shared with the user's own configuration, so every
//! rule here is about not damaging it:
//!
//! * only the entry this command owns is ever added, rewritten, or removed —
//!   ownership is an explicit `ff_rdp_managed` key, never "it mentions ff-rdp";
//! * re-running is a no-op that does not touch the file at all, so
//!   `settings.json` stays byte-identical (and its mtime unchanged);
//! * a moved binary is repaired in place rather than duplicated;
//! * key order is preserved (`serde_json` is built with `preserve_order`), so a
//!   repair produces a minimal diff in the user's version control.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::cli::args::{Cli, InstallHookArgs};
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

/// Key stamped on the hook group this command owns.
///
/// Ownership must be explicit rather than inferred from the command string: a
/// user is entitled to write their own `ff-rdp` hook, and `--uninstall` must
/// not eat it.
const MANAGED_KEY: &str = "ff_rdp_managed";

/// The arguments appended to the resolved binary path to form the hook
/// command. `--hook` trims the page block to headings plus the first fifteen
/// interactive entries — see `home::HOOK_INTERACTIVE_LIMIT`.
const HOOK_ARGS: &str = "home --hook";

/// The agent runtime being targeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Claude,
    Codex,
    OpenCode,
}

impl Target {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
}

/// Resolve the user's home directory, honoring `HOME`/`USERPROFILE` before the
/// platform API.
///
/// Same rationale as `install_skill::resolve_home_dir`: `dirs::home_dir()`
/// ignores an overridden `HOME` on Windows, which made the iter-108 install
/// tests write into the real profile and leak state between cases. Every test
/// below redirects `HOME`, so this must honor it on all three platforms.
fn resolve_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

/// Walk up from `start` looking for a `.git` directory or file.
fn find_git_root(start: PathBuf) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The settings file a target/scope pair writes to.
fn settings_path(target: Target, project: bool) -> Result<PathBuf, AppError> {
    match target {
        Target::Claude => {
            if project {
                let cwd = std::env::current_dir()
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to get cwd: {e}")))?;
                let root = find_git_root(cwd).ok_or_else(|| {
                    AppError::User(
                        "not in a git repository — cannot use --project scope.\n\
                         hint: run from inside a git repo, or drop --project for a user-level hook."
                            .to_owned(),
                    )
                })?;
                Ok(root.join(".claude").join("settings.json"))
            } else {
                let home = resolve_home_dir().ok_or_else(|| {
                    AppError::User("could not determine home directory".to_owned())
                })?;
                Ok(home.join(".claude").join("settings.json"))
            }
        }
        // Unreachable in practice: `run` refuses these targets before asking
        // for a path. Kept total so adding a target cannot silently fall
        // through to the Claude location.
        Target::Codex | Target::OpenCode => Err(unsupported_target(target)),
    }
}

/// The refusal for a target whose file format this build cannot write
/// correctly.
///
/// The iteration plan's rule is explicit, and it is the right one: writing a
/// guessed hook shape is worse than writing nothing, because an inert entry in
/// a config file looks installed and is never noticed again. So these exit 1
/// with the reason and the working alternative rather than producing a file.
fn unsupported_target(target: Target) -> AppError {
    let (name, location, why) = match target {
        Target::Codex => (
            "codex",
            "~/.codex/hooks.json (with [features] hooks = true in ~/.codex/config.toml)",
            "the entry schema inside hooks.json is not pinned by anything this build can verify, \
             and an entry with the wrong shape parses but never fires",
        ),
        Target::OpenCode => (
            "opencode",
            "~/.config/opencode/plugins/",
            "OpenCode loads plugins as JavaScript modules, and ff-rdp ships no JavaScript \
             (CLAUDE.md: all code stays in Rust); a plugin file with a guessed export shape \
             would load inert",
        ),
        // Not reachable — `run` and `settings_path` both handle Claude before
        // asking for a refusal — but written out rather than `unreachable!`
        // so a future target added to the enum cannot turn a wiring mistake
        // into a panic in a command whose whole job is to not damage files.
        Target::Claude => (
            "claude",
            "~/.claude/settings.json",
            "this target is supported; reaching this message is a wiring bug",
        ),
    };
    AppError::User(format!(
        "install-hook --{name} is not supported yet: {why}.\n\
         Target location, for a hand-written hook: {location}\n\
         hint: `ff-rdp install-hook --claude` is supported today; run `ff-rdp` at the start of a \
         session to get the same view by hand."
    ))
}

/// Quote a command path for a POSIX/Windows shell if it contains whitespace
/// or a character that is special inside a double-quoted string.
///
/// The hook runner executes the string through a shell, so an unquoted
/// `C:\\Program Files\\…` would split into two words. Backslash is left alone
/// even when quoting: it is a literal path separator on Windows, not a
/// POSIX-shell escape, and an install location containing `"`, `$`, or `` ` ``
/// is exotic enough that surviving unbroken (this function's job) matters
/// more than surviving unescaped-by-the-shell (out of scope: a path with
/// those characters was already an unusual choice by whoever created it).
fn shell_quote(path: &str) -> String {
    const NEEDS_ESCAPE: [char; 3] = ['"', '$', '`'];
    if !path.contains(char::is_whitespace) && !path.contains(NEEDS_ESCAPE) {
        return path.to_owned();
    }
    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('"');
    for c in path.chars() {
        if NEEDS_ESCAPE.contains(&c) {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// Whether `candidate` and `current` are the same executable on disk.
///
/// Canonicalized so a symlinked `~/.cargo/bin/ff-rdp` still matches the target
/// it points at — the common install shape, and the case where preferring the
/// bare name is most valuable.
fn same_executable(candidate: &Path, current: &Path) -> bool {
    match (candidate.canonicalize(), current.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => candidate == current,
    }
}

/// The first `ff-rdp` on `PATH`, if any.
fn ff_rdp_on_path() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ff-rdp.exe"
    } else {
        "ff-rdp"
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The command string the hook entry runs.
///
/// Prefers the bare name `ff-rdp` when the first `ff-rdp` on `PATH` *is* this
/// executable: a settings file that says `ff-rdp` keeps working after a
/// `cargo install` upgrade moves the binary, and it does not leak the
/// operator's home directory into a file they may commit. Otherwise the
/// absolute path of the running executable, because a bare name that resolves
/// to a *different* build would be worse than verbose.
fn resolve_hook_command() -> String {
    let current = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ff-rdp"));
    let bare_is_us = ff_rdp_on_path().is_some_and(|p| same_executable(&p, &current));
    let bin = if bare_is_us {
        "ff-rdp".to_owned()
    } else {
        shell_quote(&current.to_string_lossy())
    };
    format!("{bin} {HOOK_ARGS}")
}

/// Build the hook group this command owns.
fn managed_group(command: &str) -> Value {
    json!({
        "hooks": [
            {
                "type": "command",
                "command": command,
            }
        ],
        MANAGED_KEY: true,
    })
}

/// Is this `SessionStart` group the one this command owns?
fn is_managed(group: &Value) -> bool {
    group.get(MANAGED_KEY).and_then(Value::as_bool) == Some(true)
}

/// The command string inside a managed group, if it has one.
fn group_command(group: &Value) -> Option<&str> {
    group
        .get("hooks")?
        .as_array()?
        .first()?
        .get("command")?
        .as_str()
}

/// What a plan would do to the settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// No managed entry existed; one was (or would be) added.
    Installed,
    /// A managed entry existed with a different command; it was rewritten.
    Repaired,
    /// A managed entry already names this command — the file is untouched.
    NoOp,
    /// A managed entry existed and was removed.
    Uninstalled,
    /// `--uninstall` with nothing of ours in the file.
    NotInstalled,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Repaired => "repaired",
            Self::NoOp => "no-op",
            Self::Uninstalled => "uninstalled",
            Self::NotInstalled => "not-installed",
        }
    }

    /// Whether applying this action changes the file.
    fn writes(self) -> bool {
        matches!(self, Self::Installed | Self::Repaired | Self::Uninstalled)
    }
}

/// Apply an install to a parsed settings document, returning the action taken.
///
/// Pure: takes and returns the whole document, so the merge rules — every other
/// hook survives, key order is preserved, ours is the only group touched — are
/// unit-testable without a filesystem.
///
/// Deliberately asymmetric with [`read_settings`]: a `hooks` or `SessionStart`
/// value of the wrong JSON *type* (e.g. `"hooks": "nonsense"`, covered by
/// `unit_212_install_survives_a_malformed_hooks_value`) is coerced into an
/// empty container rather than refused. `read_settings` refuses on invalid
/// *syntax* because that could be arbitrary content worth preserving; a
/// `hooks` value of the wrong type is already schema-invalid for Claude
/// Code's own settings format, so there is nothing valid there to lose by
/// replacing it, and refusing would turn "the file has one unrelated bad
/// field" into "this command cannot ever install."
fn apply_install(settings: &mut Value, command: &str) -> Action {
    if !settings.is_object() {
        *settings = Value::Object(Map::new());
    }
    let root = settings.as_object_mut().expect("just made it an object");
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let hooks = hooks.as_object_mut().expect("just made it an object");
    let session_start = hooks
        .entry("SessionStart")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !session_start.is_array() {
        *session_start = Value::Array(Vec::new());
    }
    let groups = session_start.as_array_mut().expect("just made it an array");

    if let Some(existing) = groups.iter_mut().find(|g| is_managed(g)) {
        if group_command(existing) == Some(command) {
            return Action::NoOp;
        }
        *existing = managed_group(command);
        return Action::Repaired;
    }
    groups.push(managed_group(command));
    Action::Installed
}

/// Remove the managed entry, pruning containers this command created and that
/// are now empty. Never removes an empty container it did not empty.
fn apply_uninstall(settings: &mut Value) -> Action {
    let Some(root) = settings.as_object_mut() else {
        return Action::NotInstalled;
    };
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Action::NotInstalled;
    };
    let Some(groups) = hooks.get_mut("SessionStart").and_then(Value::as_array_mut) else {
        return Action::NotInstalled;
    };
    let before = groups.len();
    groups.retain(|g| !is_managed(g));
    if groups.len() == before {
        return Action::NotInstalled;
    }
    if groups.is_empty() {
        hooks.remove("SessionStart");
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }
    Action::Uninstalled
}

/// Read a settings file, or an empty object when it does not exist.
///
/// A file that exists but is not JSON is a hard error: rewriting it would
/// destroy configuration the user cares about, and there is no safe merge.
fn read_settings(path: &Path) -> Result<Value, AppError> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Value::Object(Map::new())),
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            AppError::User(format!(
                "{} is not valid JSON ({e}) — refusing to rewrite it.\n\
                 hint: fix the file (or move it aside) and re-run.",
                path.display()
            ))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(AppError::Internal(anyhow::anyhow!(
            "failed to read {}: {e}",
            path.display()
        ))),
    }
}

/// Serialize a settings document the way this command always writes it:
/// two-space pretty JSON with a trailing newline.
///
/// Pinned in one place because the idempotence AC is about *bytes*: two
/// installs must produce an identical file, which they only do if the writer
/// is deterministic.
fn serialize_settings(settings: &Value) -> String {
    let mut text = serde_json::to_string_pretty(settings).unwrap_or_else(|_| "{}".to_owned());
    text.push('\n');
    text
}

/// Write `settings` to `path` atomically: build the new content in a sibling
/// temp file, then `rename` it over `path`.
///
/// `path` is `~/.claude/settings.json` — Claude Code's own configuration,
/// read at the start of every session. A plain truncate-then-write leaves a
/// window where a crash, a full disk, or Claude Code itself reading the file
/// mid-write would see a truncated or empty document; a same-directory
/// `rename` is atomic on both POSIX and Windows (NTFS), so any reader always
/// sees either the old complete file or the new one, never a partial write.
/// This does not close the separate race of two `install-hook` invocations
/// running concurrently and each reading-modifying-writing — last writer
/// wins, same as it would with a plain write — but that window is far
/// narrower and does not corrupt the file.
fn write_settings(path: &Path, settings: &Value) -> Result<(), AppError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "failed to create {}: {e}",
            parent.display()
        ))
    })?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "failed to create a temp file next to {}: {e}",
            path.display()
        ))
    })?;
    std::io::Write::write_all(&mut tmp, serialize_settings(settings).as_bytes()).map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "failed to write temp file for {}: {e}",
            path.display()
        ))
    })?;
    tmp.persist(path).map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "failed to replace {} with the updated settings: {e}",
            path.display()
        ))
    })?;
    Ok(())
}

/// Which target the flags selected. Exactly one is required — defaulting to
/// Claude would write a file the caller never named.
fn selected_target(args: &InstallHookArgs) -> Result<Target, AppError> {
    let selected: Vec<Target> = [
        (args.claude, Target::Claude),
        (args.codex, Target::Codex),
        (args.opencode, Target::OpenCode),
    ]
    .into_iter()
    .filter_map(|(on, t)| on.then_some(t))
    .collect();
    match selected.as_slice() {
        [one] => Ok(*one),
        [] => Err(AppError::User(
            "install-hook needs a target: --claude (supported), --codex or --opencode".to_owned(),
        )),
        _ => Err(AppError::User(
            "install-hook takes exactly one target flag".to_owned(),
        )),
    }
}

pub fn run(cli: &Cli, args: &InstallHookArgs) -> Result<(), AppError> {
    let target = selected_target(args)?;
    if target != Target::Claude {
        return Err(unsupported_target(target));
    }

    let path = settings_path(target, args.project)?;
    let command = resolve_hook_command();
    let mut settings = read_settings(&path)?;

    let action = if args.uninstall {
        apply_uninstall(&mut settings)
    } else {
        apply_install(&mut settings, &command)
    };

    if action.writes() && !args.dry_run {
        write_settings(&path, &settings)?;
    }

    let results = json!({
        "target": target.as_str(),
        "scope": if args.project { "project" } else { "user" },
        "path": path.to_string_lossy(),
        "command": command,
        "action": action.as_str(),
        "dry_run": args.dry_run,
        "entry": if args.uninstall { Value::Null } else { managed_group(&command) },
    });

    if cli.format == "text" && cli.jq.is_none() {
        // `--dry-run` must never read as a completed write: the whole point of
        // the flag is that the file on disk is unchanged.
        let prefix = if args.dry_run && action.writes() {
            "would be "
        } else {
            ""
        };
        println!("{prefix}{} {}", action.as_str(), path.display());
        if !args.uninstall {
            println!("SessionStart hook command: {command}");
            println!(
                "{}",
                serialize_settings(&json!({
                    "hooks": { "SessionStart": [managed_group(&command)] }
                }))
                .trim_end()
            );
        }
        return Ok(());
    }

    let envelope = output::envelope(&results, 1, &json!({}));
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "ff-rdp home --hook";

    fn args(claude: bool, codex: bool, opencode: bool) -> InstallHookArgs {
        InstallHookArgs {
            claude,
            codex,
            opencode,
            project: false,
            dry_run: false,
            uninstall: false,
        }
    }

    /// AC `install_hook_is_idempotent` (the pure half; the e2e sibling
    /// redirects `HOME` and compares the file on disk): the second apply
    /// reports a no-op and leaves the document untouched, so the writer is
    /// never even reached and the bytes cannot change.
    #[test]
    fn unit_212_install_is_idempotent() {
        let mut settings = json!({});
        assert_eq!(apply_install(&mut settings, CMD), Action::Installed);
        let after_first = serialize_settings(&settings);

        assert_eq!(apply_install(&mut settings, CMD), Action::NoOp);
        assert_eq!(serialize_settings(&settings), after_first);
        assert!(!Action::NoOp.writes(), "a no-op must not touch the file");

        let groups = settings["hooks"]["SessionStart"].as_array().expect("array");
        assert_eq!(groups.len(), 1, "never a second copy: {settings}");
    }

    /// AC `install_hook_repairs_a_moved_binary_path`: a stale absolute path is
    /// rewritten in place, and every unrelated hook survives untouched.
    #[test]
    fn unit_212_install_repairs_a_moved_binary_and_keeps_other_hooks() {
        let mut settings = json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [
                    {"hooks": [{"type": "command", "command": "echo hello"}]},
                    {"hooks": [{"type": "command", "command": "/old/path/ff-rdp home --hook"}],
                     "ff_rdp_managed": true},
                ],
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "guard.sh"}]}
                ],
            },
        });

        assert_eq!(apply_install(&mut settings, CMD), Action::Repaired);

        let groups = settings["hooks"]["SessionStart"].as_array().expect("array");
        assert_eq!(
            groups.len(),
            2,
            "repair must not append a duplicate: {settings}"
        );
        assert_eq!(
            groups[0]["hooks"][0]["command"], "echo hello",
            "the user's own SessionStart hook must survive"
        );
        assert_eq!(groups[1]["hooks"][0]["command"], CMD);
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "guard.sh",
            "unrelated hook events must be untouched"
        );
        assert_eq!(
            settings["model"], "opus",
            "unrelated top-level keys survive"
        );
    }

    /// AC `install_hook_uninstall_removes_only_its_entry`.
    #[test]
    fn unit_212_uninstall_removes_only_the_managed_entry() {
        let mut settings = json!({
            "hooks": {
                "SessionStart": [
                    {"hooks": [{"type": "command", "command": "echo hello"}]},
                    {"hooks": [{"type": "command", "command": CMD}], "ff_rdp_managed": true},
                ],
                "PreToolUse": [{"hooks": [{"type": "command", "command": "guard.sh"}]}],
            },
        });

        assert_eq!(apply_uninstall(&mut settings), Action::Uninstalled);

        let groups = settings["hooks"]["SessionStart"].as_array().expect("array");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["hooks"][0]["command"], "echo hello");
        assert!(settings["hooks"].get("PreToolUse").is_some());

        // A second uninstall is a clean "not installed", not an error.
        assert_eq!(apply_uninstall(&mut settings), Action::NotInstalled);
    }

    /// A hand-written hook that happens to run ff-rdp is not ours: ownership is
    /// the explicit key, so `--uninstall` must leave it alone.
    #[test]
    fn unit_212_uninstall_spares_an_unmarked_ff_rdp_hook() {
        let mut settings = json!({
            "hooks": {
                "SessionStart": [
                    {"hooks": [{"type": "command", "command": "ff-rdp home --hook"}]}
                ]
            }
        });
        assert_eq!(apply_uninstall(&mut settings), Action::NotInstalled);
        assert_eq!(
            settings["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "ff-rdp home --hook"
        );
    }

    /// Uninstalling the only entry leaves no empty scaffolding behind — but
    /// only when this command is what created it.
    #[test]
    fn unit_212_uninstall_prunes_containers_it_emptied() {
        let mut settings = json!({});
        apply_install(&mut settings, CMD);
        assert_eq!(apply_uninstall(&mut settings), Action::Uninstalled);
        assert_eq!(
            settings,
            json!({}),
            "no empty hooks/SessionStart left: {settings}"
        );

        let mut with_siblings = json!({"hooks": {"PreToolUse": [{"hooks": []}]}});
        apply_install(&mut with_siblings, CMD);
        apply_uninstall(&mut with_siblings);
        assert!(
            with_siblings["hooks"].get("PreToolUse").is_some(),
            "a sibling event must keep `hooks` alive: {with_siblings}"
        );
    }

    /// A settings file whose `hooks` is the wrong JSON type must not make the
    /// command panic; it is coerced, because there is nothing there to lose.
    #[test]
    fn unit_212_install_survives_a_malformed_hooks_value() {
        let mut settings = json!({"hooks": "nonsense"});
        assert_eq!(apply_install(&mut settings, CMD), Action::Installed);
        assert_eq!(
            settings["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            CMD
        );
    }

    #[test]
    fn unit_212_exactly_one_target_flag_is_required() {
        assert!(selected_target(&args(false, false, false)).is_err());
        assert!(selected_target(&args(true, true, false)).is_err());
        assert_eq!(
            selected_target(&args(true, false, false)).expect("claude"),
            Target::Claude
        );
    }

    /// The unsupported targets refuse with a reason and a working alternative
    /// rather than writing a hook entry that would never fire.
    #[test]
    fn unit_212_unsupported_targets_refuse_with_a_reason() {
        for target in [Target::Codex, Target::OpenCode] {
            let AppError::User(msg) = unsupported_target(target) else {
                panic!("expected a user error for {target:?}");
            };
            assert!(msg.contains("not supported yet"), "{msg}");
            assert!(msg.contains("install-hook --claude"), "{msg}");
            assert!(settings_path(target, false).is_err(), "{target:?}");
        }
    }

    #[test]
    fn unit_212_hook_command_carries_the_hook_flag() {
        let command = resolve_hook_command();
        assert!(
            command.ends_with(&format!(" {HOOK_ARGS}")),
            "the hook must request the trimmed view: {command}"
        );
    }

    #[test]
    fn unit_212_shell_quote_only_quotes_when_needed() {
        assert_eq!(
            shell_quote("/usr/local/bin/ff-rdp"),
            "/usr/local/bin/ff-rdp"
        );
        assert_eq!(
            shell_quote("C:\\Program Files\\ff-rdp.exe"),
            "\"C:\\Program Files\\ff-rdp.exe\""
        );
    }

    /// A code-review catch (iter-212): the original `shell_quote` wrapped in
    /// `"..."` on whitespace but never escaped an embedded `"`, so a path
    /// containing one would break out of the quoting and corrupt the written
    /// `command` string. `$` and `` ` `` are the same problem one level down
    /// (still special inside POSIX double quotes) and are escaped for the
    /// same reason, without touching backslash — a literal separator on
    /// Windows paths, not a shell escape.
    #[test]
    fn unit_212_shell_quote_escapes_dangerous_characters() {
        assert_eq!(
            shell_quote("/tmp/weird\"name/ff-rdp"),
            "\"/tmp/weird\\\"name/ff-rdp\""
        );
        assert_eq!(shell_quote("/tmp/$HOME/ff-rdp"), "\"/tmp/\\$HOME/ff-rdp\"");
        assert_eq!(shell_quote("/tmp/`id`/ff-rdp"), "\"/tmp/\\`id\\`/ff-rdp\"");
        // No whitespace and no dangerous characters: unchanged, as before.
        assert_eq!(shell_quote("/usr/bin/ff-rdp"), "/usr/bin/ff-rdp");
    }

    /// The written form is deterministic — the idempotence AC is about bytes,
    /// and a non-deterministic serializer would break it invisibly.
    #[test]
    fn unit_212_serialization_is_stable_and_newline_terminated() {
        let mut settings = json!({});
        apply_install(&mut settings, CMD);
        let a = serialize_settings(&settings);
        let b = serialize_settings(&settings);
        assert_eq!(a, b);
        assert!(a.ends_with("}\n"), "{a}");
    }
}
