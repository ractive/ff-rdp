use clap::Parser;

mod cli;
mod commands;
mod connection_meta;
mod daemon;
mod daemon_record;
mod daemon_status;
mod dispatch;
mod error;
mod hints;
mod output;
mod output_controls;
mod output_pipeline;
mod page_map;
mod port_owner;
mod render_blocking;
mod script;
mod tab_target;
mod util;

use cli::Cli;
use error::AppError;

/// Find the first non-flag argv token — the top-level subcommand the user is
/// invoking — by walking past global flags.
///
/// Shared by [`is_type_invocation`] (the `type`-specific hint) and
/// [`flag_subcommand_trap_hint`] (iter-132 Theme D: `scroll --bottom` /
/// `dom --stats`-style traps). Used purely to attach contextual hints to
/// clap's generic "unexpected argument" error — never for real parsing.
fn find_subcommand_token(args: &[String]) -> Option<&str> {
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
        return Some(a.as_str());
    }
    None
}

/// Heuristic: is `type` the subcommand the user is invoking?
///
/// Used purely to attach a command-specific hint to clap's generic
/// "unexpected argument" error.
fn is_type_invocation(args: &[String]) -> bool {
    find_subcommand_token(args) == Some("type")
}

/// Known "wrote a flag, meant a subcommand" traps (iter-132 Theme D,
/// dogfood-62 friction): a boolean/value flag that does not exist on the
/// parent's `Args` struct but collides with the name of an actual
/// sub-subcommand, so the natural-but-wrong guess (`scroll --bottom`,
/// `dom --stats`) reads as plausible CLI syntax. Each entry is
/// `(parent subcommand, the exact flag clap rejected, correct invocation)`.
///
/// Checked directly against clap's own `ContextKind::InvalidArg` value, so
/// the hint can never drift out of sync with the flag clap actually
/// rejected — no guessing needed on our end. Add a new subcommand's
/// trap here when a dogfooding session hits it; keep the flag spelling
/// (`--foo`) exactly as `#[arg(long)]` would reject it (kebab-case).
const FLAG_SUBCOMMAND_TRAPS: &[(&str, &str, &str)] = &[
    ("scroll", "--top", "scroll top"),
    ("scroll", "--bottom", "scroll bottom"),
    ("dom", "--stats", "dom stats"),
    ("dom", "--tree", "dom tree"),
];

/// If `err` is an `UnknownArgument` on a known flag/subcommand trap (see
/// [`FLAG_SUBCOMMAND_TRAPS`]), return `(the rejected flag, the correct
/// invocation to suggest)`.
///
/// Returning `Some` signals the caller to skip clap's own default error
/// rendering: clap's edit-distance "did you mean" suggestion (e.g. `dom
/// --stats` → "tip: a similar argument exists: '--attrs'") is actively
/// misleading for these cases — `--attrs` is a real flag but not what the
/// user meant, and following it produces a different, silently-wrong
/// command rather than an error. This hint replaces that tip entirely
/// rather than appending alongside it, so the caller must build its own
/// error line rather than reuse `err`'s `Display`/`print()` (both still
/// carry the misleading tip internally).
fn flag_subcommand_trap_hint<'a>(
    args: &[String],
    err: &'a clap::Error,
) -> Option<(&'a str, &'static str)> {
    use clap::error::{ContextKind, ContextValue};

    if err.kind() != clap::error::ErrorKind::UnknownArgument {
        return None;
    }
    let subcommand = find_subcommand_token(args)?;
    let ContextValue::String(bad_flag) = err.get(ContextKind::InvalidArg)? else {
        return None;
    };
    FLAG_SUBCOMMAND_TRAPS
        .iter()
        .find(|(cmd, flag, _)| *cmd == subcommand && flag == bad_flag)
        .map(|(_, _, hint)| (bad_flag.as_str(), *hint))
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

            // iter-132 Theme D: a known flag-vs-subcommand trap (e.g.
            // `scroll --bottom`, `dom --stats`) replaces clap's default
            // rendering entirely — see `flag_subcommand_trap_hint`'s doc
            // comment for why the default "did you mean" tip is actively
            // wrong for these cases, not just incomplete. Built from scratch
            // (not `err.print()`/`{err}`) so the misleading tip never
            // reaches stderr at all.
            if let Some((bad_flag, hint)) = flag_subcommand_trap_hint(&argv, &err) {
                eprintln!("error: unexpected argument '{bad_flag}' found");
                eprintln!();
                eprintln!("hint: '{bad_flag}' is a subcommand, not a flag — try `ff-rdp {hint}`.");
                std::process::exit(2);
            }

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

    // Apply transport knobs from the CLI before opening any RDP connection.
    // --max-frame-mb caps the receive frame size; --redact-threshold tunes
    // how aggressively long non-sensitive strings are truncated in traces.
    //
    // Reject 0 for either flag — `set_*(0)` resets the underlying global
    // to its default, so accepting 0 here would silently invert operator
    // intent ("disable") into "use the default".
    if cli.max_frame_mb == 0 {
        eprintln!("error: --max-frame-mb must be ≥ 1 (0 would silently reset to the default cap)");
        std::process::exit(2);
    }
    if cli.redact_threshold == 0 {
        eprintln!(
            "error: --redact-threshold must be ≥ 1 (0 would silently reset to the default threshold)"
        );
        std::process::exit(2);
    }
    // checked_mul: an overflowing MB value would otherwise saturate to
    // usize::MAX and effectively disable the OOM guard in recv_from.
    let Some(max_frame_bytes) = cli.max_frame_mb.checked_mul(1024 * 1024) else {
        eprintln!(
            "error: --max-frame-mb {} overflows usize when converted to bytes; pick a smaller value",
            cli.max_frame_mb
        );
        std::process::exit(2);
    };
    ff_rdp_core::transport::set_max_frame_bytes(max_frame_bytes);
    ff_rdp_core::transport::set_redact_threshold(cli.redact_threshold);

    // iter-137 Theme B: publish the socket read deadline this invocation will
    // use so the error path can name a real duration.  `ProtocolError::Timeout`
    // is a unit variant — it carries no elapsed time — and the CLI used to
    // fabricate `after_ms: 0`, producing the useless
    // "operation timed out after 0ms (phase: recv)" that dogfooding kept
    // hitting when the daemon's RPC channel was contended.  The socket's read
    // timeout *is* the duration that elapsed, and it is known right here.
    error::remember_socket_timeout_ms(cli.timeout);

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
            // All other errors: emit the JSON error envelope to stdout as the
            // single error emission. Per the JSON-only output convention
            // (iter-98 Theme D), the envelope is authoritative — programmatic
            // callers parse `error_type`/`context` from it, and the previous
            // duplicate human `error: {err}` line on stderr is gone so the same
            // error is never reported twice.
            let exit_code = err.exit_code();
            let json = err.to_error_json();
            println!("{}", serde_json::to_string(&json).unwrap_or_default());
            std::process::exit(exit_code);
        }
    }
}

// iter-105 Theme C: the former `error_exit_code(&AppError)` shadow mapping
// has been folded into the canonical `AppError::exit_code()` in `error.rs`,
// which is now the single exit-code authority.  `main.rs` calls `exit_code()`
// directly at the error emission site above.

#[cfg(test)]
mod main_tests {
    use super::is_type_invocation;
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
            AppError::RdpActorDestroyed {
                actor: "conn0/tab1".to_owned()
            }
            .exit_code(),
            3
        );
    }

    // ── iter-132 Theme D: flag-vs-subcommand traps ───────────────────────────

    use super::{Cli, flag_subcommand_trap_hint};
    use clap::Parser as _;

    fn args(argv: &[&str]) -> Vec<String> {
        argv.iter().map(ToString::to_string).collect()
    }

    /// `Cli` intentionally does not derive `Debug` (kept lean for the derive
    /// macro's compile-time cost), so `.unwrap_err()` — which requires `T:
    /// Debug` — is not available here. This does the same job by hand.
    fn expect_parse_err(argv: &[String]) -> clap::Error {
        match Cli::try_parse_from(argv) {
            Ok(_) => panic!("expected a parse error for argv={argv:?}"),
            Err(e) => e,
        }
    }

    #[test]
    fn scroll_bottom_flag_suggests_scroll_bottom_subcommand() {
        let argv = args(&["ff-rdp", "scroll", "--bottom"]);
        let err = expect_parse_err(&argv);
        let (bad_flag, hint) = flag_subcommand_trap_hint(&argv, &err)
            .expect("scroll --bottom must be a recognized trap");
        assert_eq!(bad_flag, "--bottom");
        assert_eq!(hint, "scroll bottom");
    }

    #[test]
    fn scroll_top_flag_suggests_scroll_top_subcommand() {
        let argv = args(&["ff-rdp", "scroll", "--top"]);
        let err = expect_parse_err(&argv);
        let (bad_flag, hint) =
            flag_subcommand_trap_hint(&argv, &err).expect("scroll --top must be a recognized trap");
        assert_eq!(bad_flag, "--top");
        assert_eq!(hint, "scroll top");
    }

    #[test]
    fn dom_stats_flag_suggests_dom_stats_subcommand() {
        let argv = args(&["ff-rdp", "dom", "sel", "--stats"]);
        let err = expect_parse_err(&argv);
        let (bad_flag, hint) =
            flag_subcommand_trap_hint(&argv, &err).expect("dom --stats must be a recognized trap");
        assert_eq!(bad_flag, "--stats");
        assert_eq!(hint, "dom stats");
    }

    #[test]
    fn dom_stats_flag_without_selector_also_matches() {
        // `ff-rdp dom --stats` (no selector at all) hits the same clap
        // UnknownArgument path as `dom sel --stats`.
        let argv = args(&["ff-rdp", "dom", "--stats"]);
        let err = expect_parse_err(&argv);
        assert!(flag_subcommand_trap_hint(&argv, &err).is_some());
    }

    #[test]
    fn dom_tree_flag_suggests_dom_tree_subcommand() {
        let argv = args(&["ff-rdp", "dom", "--tree"]);
        let err = expect_parse_err(&argv);
        let (bad_flag, hint) =
            flag_subcommand_trap_hint(&argv, &err).expect("dom --tree must be a recognized trap");
        assert_eq!(bad_flag, "--tree");
        assert_eq!(hint, "dom tree");
    }

    /// An unrelated unknown flag on a subcommand with no trap table entry
    /// must not match — the hint stays silent and clap's normal error
    /// (including its own "did you mean" tip) is used unchanged.
    #[test]
    fn unrelated_unknown_flag_does_not_match() {
        let argv = args(&["ff-rdp", "eval", "--bogus"]);
        let err = expect_parse_err(&argv);
        assert!(flag_subcommand_trap_hint(&argv, &err).is_none());
    }

    /// A flag that happens to share a name with a trap entry but on the
    /// WRONG parent subcommand must not match (table is keyed on both
    /// subcommand and flag name).
    #[test]
    fn same_flag_name_wrong_subcommand_does_not_match() {
        // `--stats` is only a trap under `dom`, not under `scroll`.
        let argv = args(&["ff-rdp", "scroll", "--stats"]);
        let err = expect_parse_err(&argv);
        assert!(flag_subcommand_trap_hint(&argv, &err).is_none());
    }
}
