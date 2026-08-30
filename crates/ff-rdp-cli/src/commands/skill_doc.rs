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
pub(crate) const DESCRIPTION: &str =
    "drive a live Firefox from the shell — inspect, act on, and measure the page over the \
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
        &["a11y", "snapshot", "dom", "page-text", "screenshot", "inspect"],
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
        &["perf", "geometry", "styles", "computed", "cascade", "responsive", "emulate", "throttle"],
    ),
    (
        "Automate",
        "record a session, replay it, crawl a site, install the agent surface",
        &["record", "run", "index", "install-skill", "install-hook", "completions"],
    ),
];

/// The three idioms that separate a productive session from a guessing one,
/// as `(literal syntax, why it matters)`.
///
/// [`crate::commands::home`] turns these into the `-> ff-rdp …` hint lines it
/// prints once a page is loaded, so the hints an agent sees at session start
/// and the idioms the skill teaches are the same three strings.
pub(crate) const IDIOMS: &[(&str, &str)] = &[
    (
        "--ref <ref>",
        "act on the element you just saw: `a11y summary` and `--with-page` mint a `ref` for \
         every interactive entry, and `click --ref e3` needs no selector guess",
    ),
    (
        "--query \"<text>\"",
        "filter a page view down to the entries whose text, label, name, or href match, so a \
         control past the 50-entry cap is still reachable",
    ),
    (
        "--with-page",
        "have an action return the page it produced, so a click and the look at its result are \
         one round trip instead of two",
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
    out.push_str("ff-rdp                       # live state: daemon, browser, tabs, page, next steps\n");
    out.push_str("ff-rdp launch --headless     # no browser yet? start one with the debug port open\n");
    out.push_str("ff-rdp navigate <URL>        # blocks until the document commits\n");
    out.push_str("ff-rdp a11y summary          # landmarks, headings, and interactive entries with refs\n");
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
    for (syntax, why) in IDIOMS {
        out.push_str(&format!("- `{syntax}` — {why}\n"));
    }
    out.push('\n');

    out.push_str(BLOCK_END);
    out.push('\n');
    out
}

/// Replace the generated region of `existing` with `generated`.
///
/// Returns `None` when the markers are missing or out of order — the caller
/// decides whether that is a hard error (`gen-skill`) or a drift report
/// (`check-skill-drift`). Never appends the block to a file that does not
/// declare where it goes: silently bolting a generated section onto the end of
/// a hand-written skill is worse than refusing.
pub(crate) fn splice_block(existing: &str, generated: &str) -> Option<String> {
    let start = existing.find(BLOCK_BEGIN)?;
    let end = existing.find(BLOCK_END)?;
    if end < start {
        return None;
    }
    let after = end + BLOCK_END.len();
    let mut out = String::with_capacity(existing.len() + generated.len());
    out.push_str(&existing[..start]);
    // `generated` ends with a newline after the end marker; the tail of the
    // file already starts with one, so trim ours to avoid growing a blank line
    // on every regeneration (which would make the check flap).
    out.push_str(generated.trim_end_matches('\n'));
    out.push_str(&existing[after..]);
    Some(out)
}

/// Extract the generated region of `existing`, markers included, or `None`
/// when it is absent or malformed.
pub(crate) fn extract_block(existing: &str) -> Option<&str> {
    let start = existing.find(BLOCK_BEGIN)?;
    let end = existing.find(BLOCK_END)?;
    if end < start {
        return None;
    }
    Some(&existing[start..end + BLOCK_END.len()])
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
        for (syntax, _) in IDIOMS {
            assert!(
                block.contains(&format!("`{syntax}`")),
                "idiom {syntax} missing from the generated block"
            );
        }
    }

    #[test]
    fn splice_replaces_only_the_marked_region() {
        let existing = format!("head\n\n{BLOCK_BEGIN}\nold\n{BLOCK_END}\n\ntail\n");
        let generated = format!("{BLOCK_BEGIN}\nnew\n{BLOCK_END}\n");
        let spliced = splice_block(&existing, &generated).expect("markers present");
        assert_eq!(spliced, format!("head\n\n{BLOCK_BEGIN}\nnew\n{BLOCK_END}\n\ntail\n"));
    }

    /// Splicing must be idempotent, or `gen-skill` would report a diff every
    /// time it ran on an already-current file.
    #[test]
    fn splice_is_idempotent() {
        let generated = generate_block();
        let existing = format!("head\n\n{BLOCK_BEGIN}\nold\n{BLOCK_END}\n\ntail\n");
        let once = splice_block(&existing, &generated).expect("markers present");
        let twice = splice_block(&once, &generated).expect("markers present");
        assert_eq!(once, twice);
    }

    #[test]
    fn splice_refuses_a_file_without_markers() {
        assert!(splice_block("no markers here\n", &generate_block()).is_none());
        let reversed = format!("{BLOCK_END}\n{BLOCK_BEGIN}\n");
        assert!(splice_block(&reversed, &generate_block()).is_none());
    }

    #[test]
    fn extract_returns_the_marked_region_only() {
        let existing = format!("head\n{BLOCK_BEGIN}\nbody\n{BLOCK_END}\ntail\n");
        let extracted = extract_block(&existing).expect("markers present");
        assert_eq!(extracted, format!("{BLOCK_BEGIN}\nbody\n{BLOCK_END}"));
        assert!(extract_block("nothing").is_none());
    }
}
