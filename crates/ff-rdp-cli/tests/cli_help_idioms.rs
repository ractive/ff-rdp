//! iter-219 Theme E AC: the top-level `--help` must show the idioms.
//!
//! All 42 runs of the axi benchmark (`kb/research/axi-benchmark-comparison.md`)
//! opened with `ff-rdp --help`, several of them `| head -50`, and `--query` —
//! the flag that turns a three-turn hunt into one — appeared nowhere in it. The
//! bundled `SKILL.md` did not close the gap, because several runs never read
//! it.
//!
//! `ff-rdp --help`'s Quick start block is now rendered from the same
//! `skill_doc::IDIOMS` table `SKILL.md` and the home view use, so the three
//! surfaces cannot disagree by construction. This test is the e2e half of that
//! guarantee: it runs the real binary and looks for the idioms in the real
//! output, so a future refactor that hand-writes the help block again fails
//! here rather than silently costing turns in a benchmark nobody re-runs.
//!
//! `cargo run -p xtask -- check-help-idioms` is the CI-side sibling; it
//! compares `--help` against the *generated* skill block rather than against
//! this hard-coded list.

use std::process::Command;

fn help() -> String {
    let bin = env!("CARGO_BIN_EXE_ff-rdp");
    let output = Command::new(bin)
        .arg("--help")
        .output()
        .expect("failed to spawn ff-rdp --help");
    assert!(
        output.status.success(),
        "ff-rdp --help must exit 0; status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// AC: "Top-level `--help` shows `page-text --query` and
/// `click --ref … --with-page` in Quick start".
#[test]
fn cli_help_quick_start_carries_the_idioms() {
    let stdout = help();
    let quick_start = stdout
        .split("Quick start:")
        .nth(1)
        .unwrap_or_else(|| panic!("ff-rdp --help must have a Quick start block:\n{stdout}"));
    // The block ends at the first blank line after it.
    let block = quick_start.split("\n\n").next().unwrap_or(quick_start);

    for idiom in [
        "ff-rdp page-text --query",
        "ff-rdp click --ref",
        "--with-page",
    ] {
        assert!(
            block.contains(idiom),
            "Quick start must show {idiom:?} — agents learn the surface from \
             --help alone; got:\n{block}"
        );
    }
}

/// The idioms have to be reachable by an agent that pipes the help through
/// `head -50`, which several benchmark runs did. The Quick start block is
/// deliberately near the top for that reason.
#[test]
fn cli_help_idioms_survive_head_50() {
    let stdout = help();
    let head: String = stdout.lines().take(50).collect::<Vec<_>>().join("\n");
    for idiom in ["--query", "--with-page", "--ref"] {
        assert!(
            head.contains(idiom),
            "{idiom:?} must appear within the first 50 lines of --help; got:\n{head}"
        );
    }
}

/// `page-text`'s one-line description in the command list must mention its cap
/// and `--query` — an agent that reads only the command list otherwise learns
/// that `page-text` returns "the page text" and pipes it through `head`,
/// losing the answer further down (iter-211's original finding, iter-219
/// Theme E's fix).
#[test]
fn cli_help_page_text_one_liner_mentions_the_cap_and_query() {
    let stdout = help();
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("page-text"))
        .unwrap_or_else(|| panic!("ff-rdp --help must list page-text:\n{stdout}"));
    assert!(
        line.contains("--query"),
        "page-text's one-liner must point at --query: {line:?}"
    );
    assert!(
        line.contains("8000"),
        "page-text's one-liner must name the cap: {line:?}"
    );
}

/// iter-230 AC: `ff-rdp --help | head -50` contains a `navigate` line carrying
/// both `--with-page` and `--query`.
///
/// The e2e half of `xtask check-help-idioms`' new assertion. iter-228 read six
/// benchmark trajectories: the one run that spelled
/// `navigate … --with-page --query` answered in 4 turns, the five that ran bare
/// `navigate` and then hunted for a ref took 9-11 — and all five read only
/// `--help | head -50`, where the flag appeared on `click --ref e3` alone, a
/// line an agent holding no ref cannot use.
#[test]
fn cli_help_head_50_carries_the_act_and_see_navigate_line() {
    let stdout = help();
    let line = stdout
        .lines()
        .take(50)
        .find(|l| l.contains("ff-rdp navigate"))
        .unwrap_or_else(|| {
            panic!("the first 50 lines of --help must show an `ff-rdp navigate` line:\n{stdout}")
        });
    for flag in ["--with-page", "--query"] {
        assert!(
            line.contains(flag),
            "the navigate line an agent reads must carry {flag:?}, or it teaches the \
             bare landing that cost 5 of 6 benchmark runs 4-9 extra turns: {line:?}"
        );
    }
}
