//! `ff-rdp completions` — generate a shell completion script.
//!
//! Writes a raw completion script for the requested shell to stdout — there is
//! no JSON envelope here, unlike most other commands, since the output is
//! meant to be `eval`'d or saved directly into a shell's completions
//! directory.

use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::args::Cli;
use crate::error::AppError;

/// Render the completion script for `shell` into `writer`.
///
/// Pure core logic — takes a `Write` so it is testable without spawning a
/// process or touching stdout.
pub(crate) fn generate_to(shell: Shell, writer: &mut impl std::io::Write) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_owned();
    clap_complete::generate(shell, &mut cmd, name, writer);
}

/// Entry point wired from `dispatch.rs`: generate the completion script for
/// `shell` and write it to stdout.
///
/// Always returns `Ok` today (`generate_to` cannot fail against an in-memory
/// `Cli::command()`), but the `Result<(), AppError>` signature matches every
/// other dispatch arm in `dispatch.rs` — keeping it uniform avoids a special
/// case at the call site if this command ever needs to report a write error.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn run(shell: Shell) -> Result<(), AppError> {
    let mut stdout = std::io::stdout();
    generate_to(shell, &mut stdout);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_to_bash_produces_non_empty_output() {
        let mut buf: Vec<u8> = Vec::new();
        generate_to(Shell::Bash, &mut buf);
        assert!(!buf.is_empty(), "bash completion output must not be empty");
        let text = String::from_utf8(buf).expect("completion output must be valid UTF-8");
        assert!(
            text.to_lowercase().contains("ff-rdp") || text.to_lowercase().contains("ff_rdp"),
            "bash completion script should reference the binary name: {text}"
        );
    }

    #[test]
    fn generate_to_zsh_produces_non_empty_output() {
        let mut buf: Vec<u8> = Vec::new();
        generate_to(Shell::Zsh, &mut buf);
        assert!(!buf.is_empty(), "zsh completion output must not be empty");
    }
}
