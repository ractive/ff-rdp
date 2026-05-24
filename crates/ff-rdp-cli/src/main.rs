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
mod page_map;
mod port_owner;
mod script;
mod tab_target;
mod util;

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
        "--log-level",
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

fn init_tracing(cli: &Cli) {
    use cli::args::LogLevel;
    use tracing_subscriber::EnvFilter;

    // Determine the filter directive: --log-level wins over RUST_LOG.
    let filter = if let Some(level) = cli.log_level {
        // Map Trace to include the transport target at trace level so that
        // a simple `--log-level trace` gets wire-level packet dumps.
        let directive = match level {
            LogLevel::Trace => {
                "ff_rdp_core::transport=trace,ff_rdp_core=trace,ff_rdp_cli=trace".to_owned()
            }
            LogLevel::Debug => "ff_rdp_core=debug,ff_rdp_cli=debug".to_owned(),
            LogLevel::Info => "info".to_owned(),
            LogLevel::Warn => "warn".to_owned(),
            LogLevel::Error => "error".to_owned(),
        };
        EnvFilter::new(directive)
    } else {
        // Fall back to RUST_LOG if set; otherwise suppress everything.
        EnvFilter::from_default_env()
    };

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_target(true)
        .init();
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

    init_tracing(&cli);

    // Warn operators when raw (unredacted) trace mode is active so that
    // credentials and payloads visible in the trace output are not overlooked.
    if matches!(std::env::var("FF_RDP_TRACE_RAW").as_deref(), Ok(s) if !s.is_empty()) {
        eprintln!("warning: FF_RDP_TRACE_RAW is set — raw unredacted trace output enabled");
    }

    let result = dispatch::dispatch(&cli);
    match result {
        Ok(()) => {}
        Err(AppError::Exit(code)) => {
            std::process::exit(code);
        }
        Err(AppError::Diagnostics { message, .. }) => {
            // Assertion failure with structured diagnostics — exit 1.
            // The diagnostics payload is already surfaced in the NDJSON step output
            // by the script runner; the CLI-level error just shows the message.
            eprintln!("error: {message}");
            std::process::exit(1);
        }
        Err(err) => {
            // All other errors: emit human-readable message to stderr and
            // JSON error envelope to stdout so programmatic callers can
            // parse error_type and context.
            let exit_code = error_exit_code(&err);
            eprintln!("error: {err}");
            let json = err.to_error_json();
            println!("{}", serde_json::to_string(&json).unwrap_or_default());
            std::process::exit(exit_code);
        }
    }
}

/// Map an `AppError` to a deterministic exit code.
///
/// | Variant                    | Exit code |
/// |----------------------------|-----------|
/// | Protocol                   | 3         |
/// | Connection                 | 3         |
/// | RdpActorDestroyed          | 3         |
/// | Shape                      | 4         |
/// | RdpTimeout                 | 5         |
/// | Transport / RemoteClosed   | 6         |
/// | Timeout (op-level)         | 124       |
/// | User / Internal / *        | 1         |
fn error_exit_code(err: &AppError) -> i32 {
    match err {
        AppError::RdpProtocol { .. }
        | AppError::Connection(_)
        | AppError::RdpActorDestroyed { .. } => 3,
        AppError::RdpShape { .. } => 4,
        AppError::RdpTimeout { .. } => 5,
        AppError::RdpTransport(_) | AppError::RdpRemoteClosed(_) => 6,
        AppError::Timeout(_) => 124,
        AppError::User(_)
        | AppError::Internal(_)
        | AppError::Diagnostics { .. }
        | AppError::DaemonVersionMismatch { .. } => 1,
        AppError::Navigation { .. } => err.exit_code(),
        AppError::Exit(code) => *code,
    }
}

#[cfg(test)]
mod main_tests {
    use super::{error_exit_code, is_type_invocation};
    use crate::error::AppError;

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

    #[test]
    fn rdp_actor_destroyed_exit_code() {
        assert_eq!(
            error_exit_code(&AppError::RdpActorDestroyed {
                actor: "conn0/tab1".to_owned()
            }),
            3
        );
    }
}
