---
title: Hyalo interaction feedback from ff-rdp work
date: 2026-09-19
type: research
status: open
tags: [hyalo, feedback, tooling]
---

# Hyalo interaction feedback

Owner request: collect good and bad interactions for later work in `../hyalo`.
Continue appending observations during ff-rdp work. Do not implement Hyalo changes
as part of collection. Record command, outcome, impact, workaround, and whether a
finding is observed directly, reported by an agent, or merely a proposed improvement.
Preserve successes as well as friction. Do not describe operator mistakes or tool-output
truncation as established Hyalo bugs. No performance benchmarks are claimed here.

## 2026-09-19 — pending-iteration audit

Version: `hyalo 0.23.0 (adbafda6c7fc 2026-09-13)`. Invoked from
`/Users/james/devel/ff-rdp`; configured vault `kb`. The entries below summarize
observed interactions rather than claiming to be a verbatim full transcript.

| ID | Kind | Command / interaction | Observed outcome and impact | Follow-up or workaround |
|---|---|---|---|---|
| H01 | Good | `hyalo find --property status=planned --tag iteration --fields file,title --format text` | Compact inventory made the backlog easy to scan. Template was included because it has the same tag/status; this is correct filtering. | Use `--glob 'iterations/iteration-*.md'` to select actual numbered plans. Preserve projections and composable filters. |
| H02 | Good | `hyalo find --glob 'iterations/iteration-*.md' --fields file,properties --format json --jq '.results \| group_by(.properties.status) \| map({status: .[0].properties.status, count:length})'` | Returned 237 done, 22 obsolete, 16 planned without dumping plan bodies. The backslashes before pipes in this table are Markdown escapes, not shell syntax. | Preserve built-in jq and consistent envelopes. |
| H03 | Good | `hyalo find --glob 'iterations/iteration-*.md' --property status=planned --broken-links --fields file --format text` | Identified two unresolved references in plan259 with exact lines136/137. Links were usefully included despite the file-only projection. | Preserve actionable locations and filter-implied fields. |
| H04 | Good | `hyalo find --file iterations/iteration-246-sweep-load-misclassification.md --regexp '2026-09-14\|load-shaped\|inverted\|outside' --fields file --format text` | Located the corrected conclusion with line numbers and section names, avoiding a large full read. Pipes above are Markdown-escaped. | Preserve regex search and section context. |
| H05 | Good | `hyalo find --property status=in-review --format text` | No-results response suggested checking actual status values and showed counts. | Useful distinction between an empty selection and command failure. |
| H06 | Mixed | `hyalo find --property status=planned --format text` | Default metadata included very large historical evidence properties; tool output exceeded its requested budget (14,596 tokens reported). | Use `--fields file,title` first. Consider a compact metadata preview with explicit expansion; truncation itself came from the calling tool, not a demonstrated Hyalo defect. |
| H07 | Operator error / discoverability | `hyalo find --path iterations --fields file,properties --format json` | Exit2: unsupported `--path`; parser suggested passing it after `--`, which would not express intended directory filtering. | Corrected to `--glob 'iterations/iteration-*.md'`. Consider a targeted `--glob` hint for this mistaken flag. |
| H08 | Mixed | `hyalo read iterations/iteration-255-infobox-facts-refs-and-query-matching.md --section 'Results' --format text` (also attempted `'Final benchmark'`) | Exit1 and closest-section suggestions, with “and 11 more.” Actual desired heading was “Reviewed-checkpoint measurement — 2026-09-13.” Guessed headings were my mistake; diagnostics helped but did not surface the relevant one. | Discover headings first with `find --fields sections`. Consider pointing the error to that compact outline command instead of a full read. |
| H09 | Friction / semantics to verify | `hyalo read iterations/iteration-253-with-page-outgoing-doc-race-detection.md --lines 255:335 --format text` | Exit0 with effectively empty output although task search reported physical file lines around283. The file has substantial frontmatter. | Full body read recovered content. Investigate physical-file versus body-relative line coordinates and out-of-range diagnostics before calling this a bug. |
| H10 | Mixed, partly agent-reported | Body reads omit frontmatter; the daemon auditor reported plan266's body read empty because the plan lives entirely in YAML. Root also needed `find --fields properties --jq ...` to retrieve plan253's actual outcome. | Important current evidence can be missed by a normal read. Broad metadata selection then produced another oversized tool result. This is also a repository authoring problem inherited from older workflow decisions. | Use `read --frontmatter` or narrow property projections. Consider an explicit “empty body; frontmatter contains content” hint. |
| H11 | Mixed | `hyalo find --help` and `hyalo --help` | Detailed reference was useful but exceeded output budgets (8,613 and 8,733 tokens reported). | `hyalo new -h` was a compact169-token synopsis. Prefer `-h` first; preserve the short/long split and make it discoverable. |
| H12 | Good | `hyalo find 'hyalo feedback' --fields file,title --format text` | No match; suggested OR search and property discovery. | Helpful recovery hints without claiming a match. |
| H13 | Friction / capability boundary | `hyalo new -h`; `hyalo types list --format text` | Scaffold creation requires a configured schema type; this vault has no types. | Created this free-form research note directly under the owner's allowance for unsupported body operations. Consider a generic new-note path without requiring vault schema changes. No failed creation command was run. |

## Collection rules for subsequent work

- Append successes, failures, confusing output and effective recovery paths as they occur.
- Include version changes, exact exit codes where captured, and minimal reproduction inputs.
- Separate historical documentation claims (for example limits of Hyalo0.22) from behavior
  actually observed on the current executable.
- Treat improvement suggestions as proposals, not verified implementation requirements.
- Keep collection local to ff-rdp until the owner starts the later Hyalo work.

## 2026-09-19 — iteration263 execution

Version remains0.23.0 (adbafda6c7fc,2026-09-13).

- Verified good: projected pending inventory using `find --jq` gave all16 numbered
  pending plans; body plus `read --frontmatter --lines 1:500` exposed YAML-only
  requirements. `read -h` explicitly documents body-relative line coordinates,
  clarifying H09 without proving a bug.
- Verified operator/calling-tool friction: broad `find --fields properties` and
  `hyalo --help` again exceeded the calling-tool output budget. Narrow jq
  projections and short `-h` recovered; no Hyalo truncation defect is claimed.
- Verified operator error: guessed section `Outcome` was absent; `read --section`
  exited1 with nearby headings. Body-relative range recovered the closing text.
- Agent-reported: searching property `iteration=263` returned no match because
  this vault encodes numbers in titles/filenames; filename selection worked.
- Verified good: `set status=done --dry-run`, then apply, and HYALO005 support
  routine completion without rewriting body evidence. General body prose remains
  a direct Markdown edit under the owner's established allowance.

## 2026-09-19 — iteration261 supervisor and review

Hyalo remains0.23.0 (`/Users/james/.cargo/bin/hyalo`). Good: `find --glob
'**/*260*' --fields file,title` located the exact plan; `read` and `set` updated
body metadata/status/tasks as intended. Worker reports seven task updates and
HYALO005 zero violations; reviewer independently used body/frontmatter reads.
Operator errors: guessed plan basenames, `create`, `property set`, and reviewer
`properties <path>` are unsupported; corrected using `find`, `new -h`, `set -h`
and `read --frontmatter`. `--glob '*260*'` does not match nested iteration paths;
`**/*260*` does. These are calling mistakes, not verified defects. Long `--help`
output was truncated by the calling tool; short `-h` avoided that friction.

Verified limitation: `new --type iteration ... --dry-run` refused because this
vault has no schema types. The plan body was created directly, then supported
scalar/list metadata added through `set`. No schema or ../hyalo changes made.
`set -p 'first_call_sites=[{"primitive":"...","site":"..."}]'` parsed the
comma-separated value into two strings, not a nested mapping. The output exposed
the shape immediately; the unsupported list-of-maps edit was repaired directly
and checked with the repository plan validator. This is an unsupported structured
input/operator assumption, not established data corruption. Proposed improvement:
provide an explicit structured YAML/JSON property input or a clear shape warning.

## 2026-09-19 — iteration260 investigation

Version0.23.0 (adbafda6c7fc2026-09-13). Working directory was dedicated worktree; paths vault-relative. read (body/frontmatter), read section, backlinks, task read --all, task set --line --status x with preview, set status and lint HYALO005 all worked. Precise frontmatter-only read prevented losing the dogfood instructions; targeted task mutations preserved original text. One operator error: task read without --all/--line/--section correctly refused with helpful syntax guidance; corrected --all succeeded. Tool output truncation occurred on overly broad combined reads and a full historical reconciliation JSON, not a Hyalo defect; narrowed reads recovered relevant text. No verified Hyalo defects; no ../hyalo edits.

Supervisor: `read --section Outcome` correctly rejected the guessed nonexistent heading and offered closest sections; this was an operator lookup error. `find --fields tasks --jq` precisely selected the landed-answer checkbox; preview and apply preserved its wording.

## 2026-09-19 iteration265 closure

Hyalo0.23.0: selected plan `read --frontmatter --format text` and body read kept metadata and preflight explicit. Supervisor initially used positional `set FILE KEY VALUE`; tool rejected with a useful `--property K=V --file` hint. Operator syntax mistake, not a Hyalo defect. Corrected `set --file iterations/iteration-265-typed-watcher-protocol-fidelity.md --property scope_note=... --property status=done` changed both scalars; HYALO005 passed. Direct body appends for exact sweep recurrences and this feedback remain the supported-workflow exception because Hyalo has no general prose editor. No Hyalo version change or ../hyalo edit.

## 2026-09-19 iteration267 interactions

Installed executable: /Users/james/.cargo/bin/hyalo, version 0.23.0 (adbafda6c7fc 2026-09-13).
- Supplied executable /Users/james/.cargo/bin/hyalo0.23.0 did not exist (exit127); corrected to verified installed executable. Operator/launch-input error, not Hyalo defect.
- read discipline-rationale.md --format text: success.
- find --property 'title~=267|268' --format text: success, exposed full metadata including267 tasks/ACs.
- read267 and268 --format text: success. read267 --frontmatter --format text: success. Metadata/body distinction is documented in help.
- read203 --section '8.': exit1, accurate section-not-found and suggested headings; corrected to --section 'Carried-forward conditions': success. Operator selector error, not defect.
- The combined read-help tool output was truncated by the tool-output budget, not Hyalo. No verified Hyalo defect found; no ../hyalo changes.

Supervisor: new cascade carry-over initially reused275, already allocated on blocked262 branch. Hyalo mv dry-run correctly previewed one backlink rewrite; apply moved it to276 and updated that backlink. Scalar title/branch updated with set; unsupported body heading prose updated directly. This was queue-numbering coordination, not a Hyalo defect. Long mv --help output exceeded the calling-tool budget; no provider output loss established.

## 2026-09-19 iteration264 and next-plan lookup

Hyalo remains0.23.0. Worker operator error: `hyalo version` is unsupported;
`/Users/james/.cargo/bin/hyalo --version` succeeded. Supervisor guessed a269
basename that did not exist; the exact filename recorded in the prior audit
worked with body/frontmatter reads. These are operator mistakes, not verified
Hyalo defects. Final264 scalar `set --file ... --property status=done` and
HYALO005 validation support completion without altering original AC wording.
Long combined reads were truncated by the calling tool, not Hyalo. No version
change or ../hyalo edit.
