---
title: 'Iteration 270: ambient guidance, action fidelity and remaining benchmark extraction gaps'
type: iteration
status: done
date: 2026-09-13
branch: iter-270/benchmark-ambient-action-and-extraction-gaps
depends_on: [256]
dogfood_path: |
  Read iteration 256's retained 84 trajectories and reproduce only the documented
  command/query paths on an exclusively owned Firefox with this checkout's ffrdp
  helper. Record the proposed guidance and validation design before changes.
  A new paid comparison requires a separately bounded plan and authorization;
  never replace the original baseline or SessionStart rows.
first_call_sites: []
tags: [iteration, carry-over, benchmark, agent-ergonomics]
---

# Iteration 270: preserve useful browser-first guidance without losing action fluency

Filed from iteration 256's single baseline 42 + real SessionStart 42 measurement
on 2026-09-13. Original source/harness was `8c400376aa1d8ee85bc0bcee2a7bb2a898a9e277`;
full tables, exact first-tool IDs, original/delivered argv, raw tool results and
costs live under `.git/ralph-loop/20260912-validation-efficiency/iter256-measurement/`
and [[axi-benchmark-comparison]]. No implementation is authorized in the current
252–257 queue.

## Measured problems

- Successful browser-first calls increased from 1/42 to 37/42, while average turns
  increased from 5.476 to 6.976. Both conditions scored 41/42 upstream passes.
  `--with-page` adoption fell from 40/42 to 5/42. The actual 533-byte hook names
  a11y, page-text and console, but provides no navigate-with-page or click/type
  syntax. This is a candidate explanation to test, not proof that default-on
  output would improve outcomes. The two matrices ran sequentially, not interleaved.
- The baseline click-through means were infobox 6.667/link 4.000/search 6.333;
  treatment 9.000/10.333/12.667. Only baseline link-follow met ≤ 5. Infobox-specific
  query/ref discovery remains with [[iteration-269-infobox-discovery-after-fact-refs]].
- Baseline tabular/deep extraction means6.667/7.667 missed ≤ 6; treatment 5.333/7.667
  met only tabular. Every one of these extraction runs used `--query`; compound
  literal misses and fallback extraction remain after adoption. GitHub issue
  investigation baseline run 2 omitted one full title and failed; treatment passed 3/3.
- Command errors were6 in 5 baseline runs and19 in 14 treatment runs. Examples:
  nonexistent content/js/nav/tab-new/find-ref(s), unsupported --url/--expression/
  --value, positional ref/type confusion, empty refs and guessed CSS selectors.
  Preserve each exact trace; do not add aliases or change syntax merely to legalize
  agent mistakes. Baseline search 1's hidden-button timeout and treatment infobox 3's
  empty-ref error are masked by shell recovery, so upstream error counts5/18 omit them.
- Task fidelity differs from answer grading. All six Makefile runs skip the initial
  repository-root navigation. All three treatment link-follow runs use direct URL
  navigation after failed clicks (raw grades PASS/FAIL/PASS). Treatment infobox 2/3
  also substitute URLs (PASS/PASS). Baseline search 1 types into the box but reaches
  the article through a direct Special:Search URL after its click times out (PASS).
  No historical score is to be rewritten; stricter observations belong beside it.

## Tasks [4/4]

- [x] Reproduce the retained command/query failures and explain the actual hook/help
      information each agent saw; separate invalid syntax, valid zero matches,
      truncated output and correct product errors before proposing changes.
- [x] Propose a bounded improvement to the opt-in home/SessionStart next steps,
      including correct navigate/act-and-see/ref/type syntax where useful; preserve
      compact output, literal/regex query compatibility and existing defaults.
      Coordinate infobox evidence with269 without absorbing its scope.
- [x] Define and validate an action-fidelity/error audit that retains upstream
      grades, first actual top-level tool IDs, first-command failures and all paid
      invocation failures; benchmark helpers must not hide omitted steps or errors.
- [x] Implement only the evidence-supported scoped outcome, or document why no
      product change is warranted; add appropriate regressions and retain measured
      target misses. Specify controlled validation before requesting any new paid run.

## Acceptance Criteria [4/4]

- [x] Every listed failure and numeric target miss has a trace-backed explanation
      and a bounded disposition; historical256 results remain unchanged.
- [x] Guidance and any implementation preserve current command/query/default
      contracts and correct browser ownership; broader changes require a separate
      explicit design decision.
- [x] Validation reports action fidelity, observed errors, first-command success,
      extraction adoption, task means, original model/prompt/CLI identities and
      separate agent/judge list costs; no missing or failed row becomes a cheap success.
- [x] Required ordered gates and relevant live evidence pass for any product change;
      remaining requirements stay unticked with their actual dispositions.

## Implementation outcome — 2026-09-19

The retained 84 rows support one narrow change. `home --hook`, the command used
by the opt-in SessionStart installer, now gives a loaded page five bounded next
steps: navigate with `--with-page --query`, click a ref with `--with-page`, type
through a ref using the required `--text` flag, use positional CSS without
passing it to `--ref`, and refresh refs with `a11y summary`. A concrete `eN` is
printed only when that hook invocation minted it. Without daemon refs the click
and type examples use explicit `<minted-ref>` / `<input-ref>` placeholders and
say to replace them; no usable handle is fabricated. The ordinary home hints,
CLI syntax, query matching, defaults and ownership rules are unchanged. The
five-line guidance block is capped at 600 bytes in unit coverage, while the live
hook fixture keeps the complete trimmed payload below 2,000 bytes.

The repaired live regression exercises the real trimmed text hook path. Its page
contains one action link and forty text inputs, exceeding the 15-entry hook cap;
both action kinds remain visible after the collector groups links before inputs.
The test asserts truncation and the byte budget, derives both action refs from
the actual text hook, uses the advertised `type --ref ... --text ... --with-page`
form, then uses `click --ref ... --with-page` and verifies the destination heading.
Ordinary home output is used only to compare list lengths, never to feed actions. The
first attempted regression exposed the exact retained syntax family: positional
text beside `type --ref` is parsed as the mutually exclusive selector and exits
2. The final guidance therefore names `--text` explicitly.

### Retained audit and dispositions

`.git/ralph-loop/20260919-queue/iter270/audit-validation.json` validates all 84
retained rows without rerunning or changing them. Every row keeps its raw grade,
strict action-fidelity note, first actual top-level tool ID, first-command result,
observed/embedded errors, query and with-page adoption, turns, and separate agent
and judge list costs. It also keeps the original product/harness/upstream SHAs,
model and judge model, prompt hash, CLI/binary and Firefox identities. Both
conditions retain 42 rows and 41/42 raw passes. Baseline retains 6 observed errors
in 5 runs versus upstream 5; SessionStart retains 19 in 14 runs versus upstream
18. Each difference is one shell-masked/embedded error. Paid invocation failures
are separately reported as zero rather than being inferred from task grades.

The failure families have these bounded dispositions:

- Guessed `content`, `js`, `tab-new`, `find-ref(s)` and `nav` commands, plus
  guessed `--url`, `--expression` and `--value` flags, remain invalid. No aliases
  were added. Existing command help remains authoritative; the hook surfaces only
  the supported high-frequency action forms its measured 533-byte predecessor
  omitted.
- Positional text with `type --ref` remains a clap error. The hook now demonstrates
  the correct `--ref <minted input ref> --text <text>` form.
- Valid literal zero matches remain successful filtered reads with `matches: 0`;
  literal queries remain one case-insensitive substring and regex remains
  explicit. Iteration 269 independently reproduced the existing Developer fact
  ref and real click path, so this iteration does not absorb its infobox scope.
- Hidden, nonexistent and guessed CSS selectors, visible names used as refs,
  refs used as positional CSS, and empty/unregistered refs remain correct product
  errors. The guidance distinguishes minted refs from positional CSS instead of
  weakening those errors.
- Truncated successful output followed by an invalid command, and errors masked
  by `head` or shell recovery, remain separate occurrences. The strict audit reads
  every top-level tool result and embedded error rather than trusting only the
  final pipeline status or upstream `error_count`.
- Direct-URL substitutions and omitted requested steps remain fidelity defects
  beside the unchanged grades: all six Makefile rows omit repository-root
  navigation; SessionStart link-follow runs miss the required click with grades
  PASS/FAIL/PASS; SessionStart infobox runs 2/3 substitute URLs with PASS/PASS;
  baseline search 1 substitutes Special:Search after a hidden-button timeout
  with PASS. No historical score is rewritten.

All numeric misses remain recorded: click-through baseline means are
6.667/4.000/6.333 and SessionStart means 9.000/10.333/12.667, with only baseline
link-follow meeting <=5. Baseline tabular/deep extraction means 6.667/7.667 both
miss <=6; SessionStart 5.333/7.667 meets only tabular. Baseline issue investigation
remains 2/3. Browser-first success remains 1/42 to 37/42, with-page adoption
40/42 to 5/42, query adoption 38/42 in both, and mean turns 5.476 to 6.976. These
sequential observations justify surfacing omitted syntax; they do not show that
the new wording improves agent outcomes.

### Ordinary reproduction and any future paid validation

An exclusively owned Firefox/local CLI reproduction retained under the iteration
run store recorded: valid zero-match exit 0 with `matches: 0`; positional text
with `type --ref` exit 2; and a nonexistent CSS selector as the correct JSON
timeout envelope (exit 124). The focused live regression then passed after the
syntax repair. The shared iteration-269 evidence remains the source for the
Developer fact-ref/click reproduction.

No paid comparison, smoke, model probe or judge ran. If outcome measurement is
later authorized, use the same 14 tasks x 3 repetitions per condition, exact
model/judge model, prompt, CLI and task inputs; interleave conditions; retain raw
grades and every strict-audit field above; count failed/missing rows and paid
invocation failures explicitly; and report agent and judge list costs separately.
That proposal needs a separately bounded plan and authorization. Until it runs,
there is no claim that the guidance improved adoption, turns, fidelity or grades.

## Carry-over

| Observation | Disposition |
|---|---|
| Initial dual-gate sweep: `live_137_consent_accept_via_daemon` stayed at target_count1/live_target_count0 for15,135ms/47polls; exact serial isolation reached readiness in110ms and passed | Fold into [[iteration-262-daemon-live-target-never-promoted]], which already owns this exact target-lifecycle signature. The recurrence and clean isolation are appended there; this first sweep remains red and no guidance change is implicated. |
| No paid comparison of the new wording ran; adoption, turns, fidelity and grades after the change are unknown | No plan in this iteration: a future paid run requires the separately bounded authorization and controlled design above. The observable trigger is explicit owner authorization for that comparison. |
| Initial closing sweep: the unrelated262-owned target-lifecycle recurrence made the run red | Preserved under262 with its exact isolation. A second complete post-style-repair sweep passed all346 names and all profile checks; this supplies final closure without erasing the first failure. |

Final dual-gate evidence is
`LIVE_SWEEP_SUMMARY executed=346 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=346`
and `LIVE_SWEEP_PROFILES leaked=0 unattributed=0`; all346 tests passed across all
six tiers (one CLI target and five core targets). The preceding run remains 345passed/1failed with the same complete
name and profile accounting. Stable Rust remained1.98.1; the nine discovered
xtask checks, formatting, strict workspace clippy and workspace tests then passed
in their required order before the final no-source-change sweep.

Independent review R270-1 found that the original regression obtained its input
ref from untrimmed home. Repair batch 1 corrected the fixture and action-ref
provenance; the first repair attempt still hid the input behind grouped links
and failed; the next attempt exposed an incorrect test role assertion (`textbox`
instead of this page map's `input`). Both failed logs are retained. The corrected test has separate focused
live and ordered-gate evidence under
`.git/ralph-loop/20260919-queue/iter270-repair1/astra/`.
The 346/346 product-source sweep above is retained for unchanged runtime inputs;
it predates this test-only repair and is not a fresh sweep of the repaired test.

## Out of scope

Executing this follow-up during252–257; rerunning or replacing the84 measurements;
turning on --with-page or hooks by default; rewriting upstream tasks/grades to
remove required actions; repairing screenshot or daemon lifecycle defects here.

## Implementation preflight — 2026-09-19

Dependency256 is delivered;254 already supplied installation-target support. Audit the actual trimmed533-byte hook payload and retained trajectories, not merely full home output or shared-idiom unit tests. Preserve the measured distinctions: browser-first1/42 to37/42, with-page40/42 to5/42, mean turns5.476 to6.976, and both grades41/42 despite action-fidelity problems. These samples support investigation, not a causal default-on decision. Keep guidance opt-in and coordinate269's overlapping action/error audit. No new paid comparison, smoke, model probe or judge is authorized; use retained traces and ordinary local reproduction, leaving any paid measurement explicitly pending.

This source/evidence audit adds implementation guidance, not a new execution result.
Original task and acceptance-criterion wording and checkbox states remain unchanged.

Supervisor closure: the fresh independent scoped repair review returned explicit zero findings (`iter270-repair-review/phase-result.json`). Both initial findings are resolved. Two review rounds and one completed repair batch are recorded; actual token usage is unavailable. The existing 346/346 sweep is reused only for unchanged runtime and other test inputs, with the changed hook test separately passing; no new combined sweep result is claimed.
