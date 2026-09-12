//! Codex 0.153.4's recorded hook contract; see agent-hook-formats in the KB.
//! Keep this target's stricter merge and shell rules separate from Claude.

use std::path::Path;

use serde_json::{Value, json};

use super::{Action, HOOK_ARGS, ff_rdp_on_path, is_managed, managed_group, same_executable};
use crate::error::AppError;

pub(super) const TRUST_NOTE: &str = "Review and trust this hook in Codex /hooks before it can run. Changed commands need renewed trust; other active hook sources accumulate.";

pub(super) fn check_gate(path: &Path) -> Result<(), AppError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(AppError::User(format!(
                "cannot read {} ({error}) — no hook written",
                path.display()
            )));
        }
    };
    let config = toml::from_str::<toml::Value>(&text).map_err(|error| {
        AppError::User(format!(
            "{} is not valid TOML ({error}) — no hook written",
            path.display()
        ))
    })?;
    if config
        .get("features")
        .and_then(|features| features.get("hooks"))
        .and_then(toml::Value::as_bool)
        != Some(true)
    {
        return Err(AppError::User(format!(
            "ff-rdp requires explicit opt-in in {} before installing: add `hooks = true` under `[features]`.\nCodex enables hooks by default; this is ff-rdp installer policy, not an effective-policy check. No config or hook was written.",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn install(settings: &mut Value, command: &str) -> Result<Action, AppError> {
    // Do not coerce containers: unrelated configuration must survive even when
    // it is malformed. Claude's historical coercion remains in its own writer.
    if !settings.is_object()
        || settings
            .get("hooks")
            .is_some_and(|hooks| !hooks.is_object())
        || settings
            .pointer("/hooks/SessionStart")
            .is_some_and(|groups| !groups.is_array())
    {
        return Err(AppError::User(
            "Codex hooks.json must contain an object, with an object `hooks` and array `SessionStart` when present — refusing to rewrite malformed containers".to_owned(),
        ));
    }
    let desired = managed_group(command);
    let mut groups = settings
        .pointer("/hooks/SessionStart")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let owned_count = groups.iter().filter(|group| is_managed(group)).count();
    if owned_count == 1 && groups.iter().any(|group| group == &desired) {
        return Ok(Action::NoOp);
    }
    let action = if owned_count == 0 {
        Action::Installed
    } else {
        Action::Repaired
    };
    let first_owned = groups.iter().position(is_managed).unwrap_or(groups.len());
    groups.retain(|group| !is_managed(group));
    groups.insert(first_owned, desired);
    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    }
    settings["hooks"]["SessionStart"] = Value::Array(groups);
    Ok(action)
}

pub(super) fn resolve_command() -> Result<String, AppError> {
    let current = std::env::current_exe().map_err(|error| {
        AppError::Internal(anyhow::anyhow!("cannot resolve hook executable: {error}"))
    })?;
    let path = if ff_rdp_on_path().is_some_and(|path| same_executable(&path, &current)) {
        "ff-rdp"
    } else {
        current.to_str().ok_or_else(|| {
            AppError::User(
                "Codex hook executable path is not valid Unicode; no hook written".to_owned(),
            )
        })?
    };
    Ok(command_for_path(path))
}

fn command_for_path(path: &str) -> String {
    #[cfg(not(windows))]
    {
        // Codex invokes SHELL -lc (fallback /bin/sh). Single quotes preserve
        // whitespace, substitutions, backslashes, separators and wildcards.
        format!("'{}' {HOOK_ARGS}", path.replace('\'', "'\"'\"'"))
    }
    #[cfg(windows)]
    {
        use base64::Engine;
        // Codex's Windows default is COMSPEC /C, where percent expansion still
        // happens inside quotes. Transport a literal PowerShell invocation as
        // UTF-16LE base64 so cmd never sees any path metacharacters.
        let script = format!(
            "& '{}' {HOOK_ARGS}; exit $LASTEXITCODE",
            path.replace('\'', "''")
        );
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        format!("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_repairs_owned_shape_and_duplicates() {
        let other = json!({"ff_rdp_managed": "true", "hooks": [{"command": "mine"}]});
        let mut settings = json!({"hooks":{"SessionStart":[other, {
            "ff_rdp_managed":true,"matcher":"never", "hooks":[{"command":"same"}]
        }, managed_group("same")]}});
        assert_eq!(install(&mut settings, "same").unwrap(), Action::Repaired);
        assert_eq!(
            settings["hooks"]["SessionStart"],
            json!([other, managed_group("same")])
        );
    }

    #[test]
    fn codex_command_executes_literal_path_and_argv() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(not(windows))]
        let name = " space'\";$()`touch INJECTED`&|<>*?\\\n";
        #[cfg(windows)]
        let name = " space';&()%FF_RDP_QUOTE_PROBE% ! ";
        let dir = temp.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        let binary = dir.join(if cfg!(windows) { "probe.exe" } else { "probe" });
        // A compiled argv probe avoids confusing shell-script parsing with the
        // hook command's parsing. A safe-path control proves the harness runs.
        let source = temp.path().join("probe.rs");
        std::fs::write(&source, "fn main() { let a: Vec<_> = std::env::args().skip(1).collect(); assert_eq!(a, [\"home\", \"--hook\"]); println!(\"ARGV_OK\"); }").unwrap();
        assert!(
            std::process::Command::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap()
                .success()
        );
        let control = temp.path().join(if cfg!(windows) {
            "control.exe"
        } else {
            "control"
        });
        std::fs::copy(&binary, &control).unwrap();
        #[cfg(not(windows))]
        let shells: Vec<_> = ["/bin/sh", "/bin/bash", "/bin/zsh"]
            .into_iter()
            .filter(|shell| Path::new(shell).exists())
            .collect();
        #[cfg(windows)]
        let shells = ["cmd.exe"];
        for (shell, path) in shells
            .iter()
            .flat_map(|shell| [&control, &binary].map(|path| (shell, path)))
        {
            let command = command_for_path(path.to_str().unwrap());
            #[cfg(not(windows))]
            let mut process = {
                let mut p = std::process::Command::new(shell);
                p.args(["-lc", &command]);
                p
            };
            #[cfg(windows)]
            let mut process = {
                use std::os::windows::process::CommandExt;
                let mut p = std::process::Command::new(shell);
                p.arg("/C").raw_arg(format!("\"{command}\""));
                p
            };
            let output = process
                .current_dir(temp.path())
                .env("FF_RDP_QUOTE_PROBE", "EXPANDED")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "ARGV_OK");
            assert!(!temp.path().join("INJECTED").exists());
        }
    }
}
