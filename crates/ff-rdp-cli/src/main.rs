use clap::Parser;

mod cli;
mod commands;
mod connection_meta;
mod daemon;
mod dispatch;
mod error;
mod hints;
mod output;
mod output_controls;
mod output_pipeline;
mod port_owner;
mod script;
mod tab_target;

use cli::Cli;
use error::AppError;

/// Heuristic: is `type` the subcommand the user is invoking?
///
/// Walks past global flags (everything before the first non-flag token) and
/// checks whether the first non-flag token is `type`. Used purely to attach a
/// command-specific hint to clap's generic "unexpected argument" error.
fn is_type_invocation(args: &[String]) -> bool {
    // Allowlist of value-taking global flags defined on `Cli`. All other
    // globals are booleans (`--no-daemon`, `--all`, etc.) and do not consume
    // the next argv token. Keep in sync with `Cli` in `cli/args.rs`.
    const VALUE_GLOBALS: &[&str] = &[
        "--host",
        "--port",
        "--tab",
        "--tab-id",
        "--jq",
        "--timeout",
        "--daemon-timeout",
        "--limit",
        "--sort",
        "--fields",
        "--format",
    ];

    let mut iter = args.iter().skip(1); // skip program name
    while let Some(a) = iter.next() {
        if a == "--" {
            break;
        }
        if let Some(stripped) = a.strip_prefix("--") {
            // `--flag=value` is self-contained.
            if stripped.contains('=') {
                continue;
            }
            if VALUE_GLOBALS.contains(&a.as_str()) {
                let _ = iter.next();
            }
            continue;
        }
        return a == "type";
    }
    false
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let cli = match Cli::try_parse_from(&argv) {
        Ok(cli) => cli,
        Err(err) => {
            // Render clap's normal error (and exit on --help / --version).
            use clap::error::ErrorKind;
            let kind = err.kind();
            let is_help_or_version =
                matches!(kind, ErrorKind::DisplayHelp | ErrorKind::DisplayVersion);
            // For UnknownArgument on the `type` subcommand, attach a contextual hint
            // pointing at the supported invocation forms.
            let attach_type_hint = matches!(
                kind,
                ErrorKind::UnknownArgument | ErrorKind::InvalidSubcommand
            ) && is_type_invocation(&argv);

            err.print().ok();
            if attach_type_hint {
                eprintln!(
                    "\nhint: `type` takes selector and text positionally — try `ff-rdp type 'input[type=search]' 'Krankenkasse'`."
                );
                eprintln!(
                    "      The --selector/--text flag form also works: `ff-rdp type --selector 'input[type=search]' --text 'Krankenkasse'`."
                );
            }
            // Match clap's exit behavior.
            if is_help_or_version {
                std::process::exit(0);
            } else {
                std::process::exit(2);
            }
        }
    };
    let result = dispatch::dispatch(&cli);
    match result {
        Ok(()) => {}
        Err(AppError::User(msg)) => {
            eprintln!("error: {msg}");
            std::process::exit(1);
        }
        Err(AppError::Internal(err)) => {
            // Internal errors are unexpected runtime failures — exit 1.
            // Exit 2 is reserved for usage/argument errors (clap parse failures).
            eprintln!("internal error: {err:#}");
            std::process::exit(1);
        }
        Err(AppError::Exit(code)) => {
            std::process::exit(code);
        }
        Err(AppError::Connection(msg)) => {
            // Exit 3 — could not reach Firefox or daemon.
            eprintln!("error: {msg}");
            std::process::exit(3);
        }
        Err(AppError::Timeout(msg)) => {
            // Exit 124 — operation exceeded its timeout (matches GNU timeout convention).
            eprintln!("error: {msg}");
            std::process::exit(124);
        }
    }
}

#[cfg(test)]
mod main_tests {
    use super::is_type_invocation;

    #[test]
    fn detects_type_subcommand() {
        let args: Vec<String> = ["ff-rdp", "type", "input", "hi"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(is_type_invocation(&args));
    }

    #[test]
    fn detects_type_after_global_flags() {
        let args: Vec<String> = ["ff-rdp", "--port", "6000", "type", "--bogus"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(is_type_invocation(&args));
    }

    #[test]
    fn detects_type_with_eq_global_flag() {
        let args: Vec<String> = ["ff-rdp", "--port=6000", "type", "--bogus"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(is_type_invocation(&args));
    }

    #[test]
    fn rejects_other_subcommand() {
        let args: Vec<String> = ["ff-rdp", "click", "input"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(!is_type_invocation(&args));
    }

    #[test]
    fn rejects_no_subcommand() {
        let args: Vec<String> = ["ff-rdp", "--port", "6000"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(!is_type_invocation(&args));
    }

    // Boolean global flags (`--no-daemon`, `--all`, etc.) must NOT consume the
    // following token; otherwise the heuristic swallows `type` and the hint
    // never fires.
    #[test]
    fn detects_type_after_boolean_global_flag() {
        let args: Vec<String> = ["ff-rdp", "--no-daemon", "type", "--bogus"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(is_type_invocation(&args));
    }

    #[test]
    fn detects_type_after_mixed_globals() {
        let args: Vec<String> = [
            "ff-rdp",
            "--no-daemon",
            "--port",
            "6000",
            "--detail",
            "type",
            "--bogus",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert!(is_type_invocation(&args));
    }
}
