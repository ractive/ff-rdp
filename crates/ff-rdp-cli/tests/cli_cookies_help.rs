//! iter-83 AC: `cookies_help_no_fields_paragraph_leak`.
//!
//! Runs `ff-rdp cookies --help` and asserts that the `--include-document-cookie`
//! help paragraph does NOT bleed into the `--fields` paragraph.  In iter-82 the
//! `long_help` text for `--include-document-cookie` was missing a terminator,
//! causing the `Comma-separated list of fields` snippet from `--fields` to appear
//! inside the `--include-document-cookie` description.
//!
//! # Running
//!
//!   cargo test -p ff-rdp-cli --test cli_cookies_help

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::ff_rdp_bin;

/// `cookies_help_no_fields_paragraph_leak` (iter-83 AC):
///
/// Runs `ff-rdp cookies --help` and asserts:
///   - The command exits 0 (or exit code 2, which clap uses for `--help`).
///   - The combined help output does NOT contain the string
///     `"Comma-separated list of fields"` adjacent to `"include-document-cookie"`.
///     This ensures the long_help text for `--include-document-cookie` does not
///     bleed into the `--fields` description paragraph.
#[test]
fn cookies_help_no_fields_paragraph_leak() {
    let output = Command::new(ff_rdp_bin())
        .args(["cookies", "--help"])
        .output()
        .expect("ff-rdp cookies --help");

    // clap exits 0 for --help on most versions but 2 on some; accept both.
    let exit_ok = output.status.success() || output.status.code() == Some(2);
    assert!(
        exit_ok,
        "cookies_help_no_fields_paragraph_leak: ff-rdp cookies --help exited with unexpected \
         status {:?}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    // Combine stdout and stderr (clap may write to either).
    let help_text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // The `--include-document-cookie` flag is now hidden; it must not appear
    // in help at all.  Asserting absence makes this test catch regressions
    // where the flag accidentally reappears.
    assert!(
        !help_text.contains("include-document-cookie"),
        "cookies_help_no_fields_paragraph_leak: \
         '--include-document-cookie' should be hidden but appears in help\n\
         full help text:\n{help_text}"
    );
    // The `--fields` description text must still be present.
    assert!(
        help_text.contains("Comma-separated list of fields"),
        "cookies_help_no_fields_paragraph_leak: \
         '--fields' help paragraph missing 'Comma-separated list of fields'\n\
         full help text:\n{help_text}"
    );

    eprintln!("cookies_help_no_fields_paragraph_leak: PASS");
}
