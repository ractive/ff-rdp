//! `ff-rdp --help` carries the idioms and quick-start lines the skill teaches.
//!
//! Formerly `cargo run -p xtask -- check-help-idioms` (iter-219); a plain
//! `cargo test` since DEC-059.
//!
//! All 42 runs of the axi benchmark opened with `ff-rdp --help` and `--query`
//! appeared nowhere in it. `skill_drift.rs` pins SKILL.md against
//! `skill_doc.rs`; this is its sibling for `--help`: every idiom flag the
//! generated skill block lists must appear in `--help`'s Quick start block,
//! and every one of its quick-start command lines must appear in `--help`.
//! Both are rendered from the same table today, so this goes red the day
//! someone hand-writes the help text again and drops a line. The iter-230
//! "navigate line within the first 50 lines" rule lives in
//! `tests/e2e/cli_help_idioms.rs`.

use std::process::Command;

fn cli_stdout(arg: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ff-rdp"))
        .arg(arg)
        .output()
        .unwrap_or_else(|e| panic!("spawn ff-rdp {arg}: {e}"));
    assert!(
        out.status.success(),
        "ff-rdp {arg} failed: status={:?} stdout={} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("ff-rdp emits UTF-8")
}

/// The idiom *flags* listed under "Idioms worth knowing" — `--ref` out of
/// `` - `--ref <ref>` — … ``. The flag, not the metavariable spelling: `--help`
/// renders each idiom as a runnable example.
fn idiom_flags(skill_block: &str) -> Vec<String> {
    let Some(section) = skill_block.split("## Idioms worth knowing").nth(1) else {
        return Vec::new();
    };
    section
        .lines()
        .map(str::trim)
        .take_while(|l| !l.starts_with("##"))
        .filter_map(|l| l.strip_prefix("- `"))
        .filter_map(|rest| rest.split('`').next())
        .filter_map(|syntax| syntax.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

/// The command lines of the skill's Quick start fenced block, comments dropped.
fn quick_start_commands(skill_block: &str) -> Vec<String> {
    let body = skill_block
        .split("## Quick start")
        .nth(1)
        .and_then(|s| s.split("```bash").nth(1))
        .and_then(|s| s.split("```").next())
        .unwrap_or("");
    body.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `--help` from its `Quick start` heading to the following `Usage:` line.
/// Falls back to the whole help when a marker is missing, so a reworded help
/// degrades to a whole-output match rather than passing on an empty region.
fn quick_start_region(help: &str) -> &str {
    let Some(start) = help.find("Quick start") else {
        return help;
    };
    let tail = &help[start..];
    tail.find("\nUsage:").map_or(tail, |end| &tail[..end])
}

#[test]
fn help_carries_every_skill_idiom_and_quick_start_line() {
    let help = cli_stdout("--help");
    let skill_block = cli_stdout("skill-doc");

    let flags = idiom_flags(&skill_block);
    assert!(
        !flags.is_empty(),
        "`ff-rdp skill-doc` listed no idioms under `## Idioms worth knowing`:\n{skill_block}"
    );
    let commands = quick_start_commands(&skill_block);
    assert!(
        !commands.is_empty(),
        "`ff-rdp skill-doc` produced no Quick start commands:\n{skill_block}"
    );

    // Idiom flags are matched against the Quick start region only: `--query`
    // is a real flag on several subcommands, so a whole-help match would pass
    // even when the Quick start block dropped the idiom.
    let region = quick_start_region(&help);
    let mut failures: Vec<String> = flags
        .iter()
        .filter(|f| !region.contains(f.as_str()))
        .map(|f| format!("idiom `{f}` is not mentioned in `ff-rdp --help`'s Quick start block"))
        .collect();
    failures.extend(
        commands
            .iter()
            .filter(|c| !help.contains(c.as_str()))
            .map(|c| format!("Quick start line `{c}` is in SKILL.md but not in `ff-rdp --help`")),
    );
    assert!(
        failures.is_empty(),
        "`ff-rdp --help` and the skill's idiom table disagree:\n  {}\nfix: render the \
         Quick start block from crates/ff-rdp-cli/src/commands/skill_doc.rs (see \
         `quick_start_help`) rather than writing it out by hand.",
        failures.join("\n  ")
    );
}

const BLOCK: &str = "\
## Quick start

```bash
ff-rdp                             # live state
ff-rdp page-text --query \"<text>\"  # find the part you need
```

## Idioms worth knowing

- `--ref <ref>` — act on the element you just saw
- `--query \"<text>\"` — filter a page view
- `--with-page` — have an action return the page

## Output contract
";

#[test]
fn parsers_read_flags_and_commands_out_of_the_block() {
    assert_eq!(idiom_flags(BLOCK), ["--ref", "--query", "--with-page"]);
    assert_eq!(
        quick_start_commands(BLOCK),
        ["ff-rdp", "ff-rdp page-text --query \"<text>\""]
    );
    assert!(idiom_flags("# nothing").is_empty());
    assert!(quick_start_commands("# nothing").is_empty());
}

#[test]
fn idioms_below_usage_do_not_count() {
    let drifted = "Quick start:\n  ff-rdp launch\n\nUsage: ff-rdp <COMMAND>\n  --ref --query";
    assert_eq!(
        quick_start_region(drifted),
        "Quick start:\n  ff-rdp launch\n"
    );
    assert_eq!(quick_start_region("no markers"), "no markers");
}
