//! One source of truth for the agent-facing description of this CLI
//! (iter-212 Theme C).
//!
//! Two surfaces used to describe ff-rdp to an agent, and neither knew about
//! the other:
//!
//! * the bundled Claude Code skill `skills/ff-rdp-debug/SKILL.md`, hand-written
//!   and checked by nothing, and
//! * whatever the agent could infer from `--help`.
//!
//! Both now read from the tables in this module. [`crate::commands::home`]
//! builds its one-line description and its state-dependent hints from
//! [`DESCRIPTION`] and [`IDIOMS`]; the generated section of `SKILL.md` is
//! produced by [`generate_block`] and pinned against the committed file by
//! `cargo run -p xtask -- check-skill-drift`.
//!
//! Editing the committed `SKILL.md`'s generated region by hand is therefore a
//! red CI check, not a silent drift: change the tables here and re-run
//! `cargo run -p xtask -- gen-skill`.

use serde_json::{Value, json};

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

/// The one-line description of what this binary is, shown by the home view
/// and as the opening line of the generated skill section.
///
/// Deliberately says *what it drives* and *how you talk to it* — an agent that
/// reads only this line should already know it is looking at a browser it can
/// command, not a log reader.
pub(crate) const DESCRIPTION: &str = "drive a live Firefox from the shell — inspect, act on, and measure the page over the \
     Remote Debugging Protocol; every command answers in JSON";

/// HTML comment that opens the generated region of `SKILL.md`.
pub(crate) const BLOCK_BEGIN: &str = "<!-- ff-rdp:generated:begin -->";
/// HTML comment that closes the generated region of `SKILL.md`.
pub(crate) const BLOCK_END: &str = "<!-- ff-rdp:generated:end -->";

/// The command surface, grouped the way an agent needs to reach for it:
/// `(group, what the group is for, the commands in it)`.
///
/// This is the table the generated skill section renders, and the reason a new
/// command cannot land without appearing in the agent-facing docs — leaving it
/// out here leaves it out of `SKILL.md`, which reviewers read.
pub(crate) const COMMAND_GROUPS: &[(&str, &str, &[&str])] = &[
    (
        "Get a browser",
        "start Firefox with the debug port open, then check the stack end to end",
        &["launch", "doctor", "tabs", "daemon"],
    ),
    (
        "Go somewhere",
        "navigate and wait for what you actually need, not a fixed sleep",
        &["navigate", "back", "forward", "reload", "wait"],
    ),
    (
        "See the page",
        "read structure and text; `a11y summary` is the orientation view and hands out refs",
        &[
            "a11y",
            "snapshot",
            "dom",
            "page-text",
            "screenshot",
            "inspect",
        ],
    ),
    (
        "Act on the page",
        "click and type against a ref or a selector, then scroll",
        &["click", "type", "scroll"],
    ),
    (
        "Ask the page",
        "evaluate JS and read what the page stored",
        &["eval", "cookies", "storage", "sources"],
    ),
    (
        "Watch the page",
        "console and network traffic, buffered by the daemon",
        &["console", "network"],
    ),
    (
        "Measure the page",
        "Web Vitals, contrast, layout, and emulated conditions",
        &[
            "perf",
            "geometry",
            "styles",
            "computed",
            "cascade",
            "responsive",
            "emulate",
            "throttle",
        ],
    ),
    (
        "Automate",
        "record a session, replay it, crawl a site, install the agent surface",
        &[
            "record",
            "run",
            "index",
            "install-skill",
            "install-hook",
            "completions",
        ],
    ),
];

/// The three idioms that separate a productive session from a guessing one,
/// as `(literal syntax, why it matters, the hint line the home view prints)`.
///
/// [`crate::commands::home`] turns the third field into the `-> ff-rdp …`
/// lines it prints once a page is loaded (substituting `{ref}` with a ref the
/// page view actually minted), so the hints an agent sees at session start and
/// the idioms the skill teaches cannot drift apart.
pub(crate) const IDIOMS: &[(&str, &str, &str)] = &[
    (
        "--ref <ref>",
        "act on the element you just saw: `a11y summary` and `--with-page` mint a `ref` for \
         every interactive entry, and `click --ref e3` needs no selector guess",
        "ff-rdp click --ref {ref}",
    ),
    (
        "--query \"<text>\"",
        "filter a page view down to the entries whose text, label, name, or href match, so a \
         control past the 50-entry cap is still reachable",
        "ff-rdp page-text --query \"<text>\"",
    ),
    (
        "--with-page",
        "have an action return the page it produced, so a click and the look at its result are \
         one round trip instead of two",
        "ff-rdp snapshot --query \"<text>\"",
    ),
];

/// Render the generated section of `SKILL.md` — everything between
/// [`BLOCK_BEGIN`] and [`BLOCK_END`], markers included.
///
/// Deterministic and version-free: the output must not change unless one of
/// the tables above changes, or `cargo run -p xtask -- check-skill-drift`
/// would go red on every release bump.
pub(crate) fn generate_block() -> String {
    let mut out = String::new();
    out.push_str(BLOCK_BEGIN);
    out.push_str("\n<!-- Generated by `cargo run -p xtask -- gen-skill`. Do not edit by hand: \n");
    out.push_str("     `cargo run -p xtask -- check-skill-drift` fails CI when this region and\n");
    out.push_str("     crates/ff-rdp-cli/src/commands/skill_doc.rs disagree. -->\n\n");

    out.push_str("## The CLI in one line\n\n");
    out.push_str(DESCRIPTION);
    out.push_str(".\n\n");

    out.push_str("## Quick start\n\n");
    out.push_str("```bash\n");
    out.push_str(
        "ff-rdp                       # live state: daemon, browser, tabs, page, next steps\n",
    );
    out.push_str(
        "ff-rdp launch --headless     # no browser yet? start one with the debug port open\n",
    );
    out.push_str("ff-rdp navigate <URL>        # blocks until the document commits\n");
    out.push_str(
        "ff-rdp a11y summary          # landmarks, headings, and interactive entries with refs\n",
    );
    out.push_str("ff-rdp click --ref e3        # act on a ref from the view above\n");
    out.push_str("```\n\n");
    out.push_str(
        "Run bare `ff-rdp` before reaching for `--help`: it costs one turn and answers \
         \"is there a browser, what is on it, and what can I do next\" — `--help` answers none \
         of those.\n\n",
    );

    out.push_str("## Command groups\n\n");
    out.push_str("| Group | Use it for | Commands |\n");
    out.push_str("|---|---|---|\n");
    for (group, purpose, commands) in COMMAND_GROUPS {
        let list = commands
            .iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("| {group} | {purpose} | {list} |\n"));
    }
    out.push('\n');

    out.push_str("## Idioms worth knowing\n\n");
    for (syntax, why, _) in IDIOMS {
        out.push_str(&format!("- `{syntax}` — {why}\n"));
    }
    out.push('\n');

    out.push_str(BLOCK_END);
    out.push('\n');
    out
}

/// `ff-rdp skill-doc` — print the generated skill section to stdout.
///
/// Hidden from `--help`: its only callers are `cargo run -p xtask -- gen-skill`
/// and `… check-skill-drift`, which need the generator's output without
/// linking this binary crate as a library.
pub fn run(cli: &Cli) -> Result<(), AppError> {
    let block = generate_block();
    // `--format text` (and the bare hidden invocation xtask uses) wants the
    // markdown itself, not a JSON envelope wrapping it — xtask splices stdout
    // straight into SKILL.md.
    if cli.jq.is_none() && cli.format != "json" {
        print!("{block}");
        return Ok(());
    }
    if cli.jq.is_none() && cli.format == "json" {
        // Default (`--format json`) still emits markdown for xtask; JSON is
        // only produced when a caller explicitly filters it.
        print!("{block}");
        return Ok(());
    }
    let results: Value = json!({ "markdown": block });
    let envelope = output::envelope(&results, 1, &json!({}));
    OutputPipeline::from_cli(cli)?.finalize(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The generator must be a pure function of the tables — two calls in the
    /// same process, and any two calls across releases, produce the same
    /// bytes. A version string or a timestamp in here would make
    /// `check-skill-drift` fail on every release bump.
    #[test]
    fn generated_block_is_deterministic() {
        assert_eq!(generate_block(), generate_block());
        let block = generate_block();
        assert!(
            !block.contains(env!("CARGO_PKG_VERSION")),
            "the generated block must not embed the crate version: {block}"
        );
    }

    #[test]
    fn generated_block_is_delimited_by_its_markers() {
        let block = generate_block();
        assert!(block.starts_with(BLOCK_BEGIN), "{block}");
        assert!(block.trim_end().ends_with(BLOCK_END), "{block}");
    }

    /// Every command group and every idiom reaches the rendered markdown —
    /// the point of the table is that a command cannot be added here and
    /// missed in the skill.
    #[test]
    fn every_group_and_idiom_reaches_the_markdown() {
        let block = generate_block();
        for (group, _, commands) in COMMAND_GROUPS {
            assert!(block.contains(group), "group {group} missing from {block}");
            for command in *commands {
                assert!(
                    block.contains(&format!("`{command}`")),
                    "command {command} missing from the generated block"
                );
            }
        }
        for (syntax, _, _) in IDIOMS {
            assert!(
                block.contains(&format!("`{syntax}`")),
                "idiom {syntax} missing from the generated block"
            );
        }
    }
}
