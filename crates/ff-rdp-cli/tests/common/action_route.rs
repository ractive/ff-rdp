//! Exact-action route evidence, including actions that return an error envelope.

pub fn command(port: u16, direct: bool, args: &[&str]) -> std::process::Command {
    let mut command = std::process::Command::new(super::ff_rdp_bin());
    command.env("RUST_LOG", "ff_rdp_cli::action_route=debug");
    // Match LiveFirefox::with_daemon_using exactly: the daemon registry
    // intentionally compares host strings, and the CLI defaults to localhost.
    command.args([
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--timeout",
        "4000",
    ]);
    if direct {
        command.arg("--no-daemon");
    }
    command.args(args);
    command
}

pub fn require_action_route(stderr: &[u8], action: &str, via_daemon: bool) -> Result<(), String> {
    let stderr = std::str::from_utf8(stderr).map_err(|error| error.to_string())?;
    let records: Vec<_> = stderr
        .lines()
        .filter_map(|line| {
            line.split_once("FF_RDP_ACTION_ROUTE ")
                .map(|(_, record)| record)
        })
        // tracing's terminal formatter may reset colour after the message.
        .map(|record| record.trim_end_matches("\u{1b}[0m"))
        .collect();
    let expected = format!("action={action} via_daemon={via_daemon}");
    if records == [expected.as_str()] {
        Ok(())
    } else {
        Err(format!(
            "expected exactly one connected-action record {expected:?}, got {records:?}"
        ))
    }
}
