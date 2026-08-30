//! `xtask check-help-idioms` — the top-level `--help` must show the idioms
//! the skill teaches (iter-219 Theme E).
//!
//! The evidence for this gate is the axi benchmark (`kb/research/
//! axi-benchmark-comparison.md`): all 42 runs opened with `ff-rdp --help`,
//! several of them `| head -50`, and `--query` — the flag that turns a
//! three-turn hunt into one — appeared nowhere in it. An agent learns the
//! surface from `--help`; the bundled `SKILL.md` did not close the gap, because
//! several runs never read it.
//!
//! `check-skill-drift` already pins `SKILL.md` against `skill_doc.rs`. This is
//! its sibling for the other surface: every idiom flag the generated skill
//! block lists, and every one of its quick-start command lines, must appear in
//! `ff-rdp --help`. Both surfaces are rendered from the
//! same table today, so the gate is normally quiet — its job is to go red the
//! day someone re-hand-writes the help text and drops a line.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::Args as ClapArgs;

#[derive(ClapArgs)]
pub struct Args {
    /// ff-rdp binary to interrogate (defaults to $FF_RDP_BIN, else `cargo run`)
    #[arg(long, value_name = "PATH")]
    pub bin: Option<PathBuf>,
}

/// The workspace root, found by walking up from this crate's manifest dir.
fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while dir.pop() {
        if dir.join("Cargo.lock").exists() {
            return dir;
        }
    }
    PathBuf::from(".")
}

/// Run the CLI under test with `args` and return its stdout.
///
/// Uses `FF_RDP_BIN` / `--bin` when available, otherwise `cargo run`. Either
/// way the binary is the one built from the tree under test, never a stale
/// `ff-rdp` from PATH — the same rule `check-skill-drift` follows, for the
/// same reason.
fn cli_stdout(bin: Option<&PathBuf>, args: &[&str]) -> Result<String> {
    let explicit = bin
        .cloned()
        .or_else(|| std::env::var_os("FF_RDP_BIN").map(PathBuf::from));
    let output = if let Some(path) = explicit {
        Command::new(&path)
            .args(args)
            .output()
            .with_context(|| format!("failed to run {path:?} {args:?}"))?
    } else {
        Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
            .args(["run", "-q", "-p", "ff-rdp-cli", "--"])
            .args(args)
            .current_dir(workspace_root())
            .output()
            .with_context(|| format!("failed to run `cargo run -p ff-rdp-cli -- {args:?}`"))?
    };
    // `--help` exits 0; every other invocation here is expected to as well.
    if !output.status.success() {
        bail!(
            "ff-rdp {args:?} exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("ff-rdp emitted non-UTF-8 on stdout")
}

/// The idiom *flags* listed under the skill's "Idioms worth knowing" heading
/// — `--ref` out of `` `--ref <ref>` ``, `--query` out of
/// `` `--query "<text>"` ``.
///
/// The flag rather than the whole syntax, because `--help` renders each idiom
/// as a runnable example (`ff-rdp click --ref e3`) and a metavariable spelling
/// is not the contract worth pinning — "can an agent see that this flag
/// exists" is.
///
/// Reads the generated markdown rather than the Rust table so this gate needs
/// no link against the CLI crate, exactly as `check-skill-drift` does.
pub fn idiom_syntaxes(skill_block: &str) -> Vec<String> {
    let Some(section) = skill_block.split("## Idioms worth knowing").nth(1) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in section.lines() {
        let line = line.trim();
        if line.starts_with("##") {
            break;
        }
        let Some(rest) = line.strip_prefix("- `") else {
            continue;
        };
        if let Some(end) = rest.find('`') {
            let syntax = &rest[..end];
            let flag = syntax.split_whitespace().next().unwrap_or(syntax);
            out.push(flag.to_owned());
        }
    }
    out
}

/// The commands listed in the skill's Quick start fenced block.
pub fn quick_start_commands(skill_block: &str) -> Vec<String> {
    let Some(section) = skill_block.split("## Quick start").nth(1) else {
        return Vec::new();
    };
    let Some(fence) = section.split("```bash").nth(1) else {
        return Vec::new();
    };
    let Some(body) = fence.split("```").next() else {
        return Vec::new();
    };
    body.lines()
        .filter_map(|line| {
            let command = line.split('#').next().unwrap_or("").trim();
            (!command.is_empty()).then(|| command.to_owned())
        })
        .collect()
}

/// The slice of `--help` from its `Quick start` heading to the following
/// `Usage:` line — the region agents actually read (42/42 benchmark runs
/// opened with `--help`, several through `head -50`), and the one this gate
/// pins the idioms into. Falls back to the whole help when either marker is
/// missing, so a reworded help degrades to the old whole-output match rather
/// than passing vacuously on an empty region.
pub fn quick_start_region(help: &str) -> &str {
    let Some(start) = help.find("Quick start") else {
        return help;
    };
    let tail = &help[start..];
    match tail.find("\nUsage:") {
        Some(end) => &tail[..end],
        None => tail,
    }
}

/// Which of `needles` are missing from `help`.
pub fn missing<'a>(help: &str, needles: &'a [String]) -> Vec<&'a String> {
    needles
        .iter()
        .filter(|n| !help.contains(n.as_str()))
        .collect()
}

pub fn run(args: Args) -> Result<()> {
    let help = cli_stdout(args.bin.as_ref(), &["--help"])?;
    let skill_block = cli_stdout(args.bin.as_ref(), &["skill-doc"])?;

    let syntaxes = idiom_syntaxes(&skill_block);
    if syntaxes.is_empty() {
        bail!(
            "`ff-rdp skill-doc` listed no idioms — expected a `## Idioms worth knowing` \
             section with `- \\`<syntax>\\` — …` bullets"
        );
    }
    let commands = quick_start_commands(&skill_block);
    if commands.is_empty() {
        bail!("`ff-rdp skill-doc` produced no Quick start commands");
    }

    // iter-219 review: match idiom flags against the Quick start region only.
    // `--query` is a real flag on several subcommands, so a whole-help
    // substring match passes even when the Quick start block dropped the
    // idiom — exactly the drift this gate exists to catch.
    let region = quick_start_region(&help);
    let mut failures: Vec<String> = Vec::new();
    for m in missing(region, &syntaxes) {
        failures.push(format!(
            "idiom `{m}` is not mentioned in `ff-rdp --help`'s Quick start block"
        ));
    }
    for m in missing(&help, &commands) {
        failures.push(format!(
            "Quick start line `{m}` is in SKILL.md but not in `ff-rdp --help`"
        ));
    }

    if failures.is_empty() {
        println!(
            "check-help-idioms: `ff-rdp --help` carries all {} idioms and all {} quick-start lines",
            syntaxes.len(),
            commands.len()
        );
        return Ok(());
    }
    bail!(
        "`ff-rdp --help` and the skill's idiom table disagree:\n  {}\n\
         fix: render the Quick start block from \
         crates/ff-rdp-cli/src/commands/skill_doc.rs (see `quick_start_help`) \
         rather than writing it out by hand.",
        failures.join("\n  ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "\
## Quick start

```bash
ff-rdp                             # live state
ff-rdp page-text --query \"<text>\"  # find the part you need
```

Some prose.

## Idioms worth knowing

- `--ref <ref>` — act on the element you just saw
- `--query \"<text>\"` — filter a page view
- `--with-page` — have an action return the page

## Output contract
";

    #[test]
    fn unit_219_idioms_are_read_out_of_the_generated_block() {
        assert_eq!(
            idiom_syntaxes(BLOCK),
            vec!["--ref", "--query", "--with-page"]
        );
    }

    #[test]
    fn unit_219_quick_start_commands_drop_their_comments() {
        assert_eq!(
            quick_start_commands(BLOCK),
            vec!["ff-rdp", "ff-rdp page-text --query \"<text>\""]
        );
    }

    /// AC: the gate fails when `--help` and the table diverge, and names the
    /// idiom that went missing.
    #[test]
    fn unit_219_a_dropped_idiom_is_reported() {
        let syntaxes = idiom_syntaxes(BLOCK);
        let complete = "usage … --ref <ref> … --query \"<text>\" … --with-page …";
        assert!(missing(complete, &syntaxes).is_empty());

        let stale = "usage … --ref <ref> … --with-page …";
        let gaps = missing(stale, &syntaxes);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0], "--query");
    }

    /// iter-219 review: the idiom match is scoped to `--help`'s Quick start
    /// block — a flag mentioned only in some subcommand's summary must not
    /// satisfy the gate.
    #[test]
    fn unit_219_idioms_outside_quick_start_do_not_count() {
        let syntaxes = idiom_syntaxes(BLOCK);
        let help = "Quick start:\n  ff-rdp click --ref e3\n  ff-rdp page-text --query \"x\"\n  ff-rdp click --ref e3 --with-page\n\nUsage: ff-rdp <COMMAND>\n  page-text --query elsewhere";
        assert!(missing(quick_start_region(help), &syntaxes).is_empty());

        // Same flags present, but only *below* Usage: — the region match
        // must report all three as missing.
        let drifted =
            "Quick start:\n  ff-rdp launch\n\nUsage: ff-rdp <COMMAND>\n  --ref --query --with-page";
        assert_eq!(missing(quick_start_region(drifted), &syntaxes).len(), 3);

        // No markers at all: degrade to the whole-help match, not an empty
        // region that passes vacuously.
        assert_eq!(quick_start_region("no markers"), "no markers");
    }

    #[test]
    fn unit_219_a_block_without_the_sections_yields_nothing() {
        assert!(idiom_syntaxes("# nothing here").is_empty());
        assert!(quick_start_commands("# nothing here").is_empty());
    }
}
