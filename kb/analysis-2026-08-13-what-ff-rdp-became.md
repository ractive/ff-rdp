---
title: "What ff-rdp became — step-back analysis"
type: analysis
date: 2026-08-13
status: done
tags: [analysis, step-back, dogfooding, complexity]
sources: [dogfooding-lanes-A-E, gate-forensics, code-review-1-5, live-sweep-qualified-run]
---

# What ff-rdp became

Written 2026-08-13, from the step-back commissioned in [[step-back-2026-08-13]]. Every
claim below is measured on the wire or read in the code, with the command or `file.rs:line`
given. Where a claim was checked and turned out wrong, the retraction is recorded rather
than quietly dropped — see [[#Findings that did not survive verification]].

## 1. What ff-rdp is now, honestly

ff-rdp is a **well-engineered read-only inspector for a live Firefox page, with a genuinely
good JSON contract, wrapped around a write path that cannot honestly do what it claims and a
session layer that is currently broken in two places**. Its 42 commands divide cleanly: the
ones that *observe* a page — `dom`, `page-text`, `snapshot`, `geometry`, `computed`, `styles`,
`cascade`, `sources`, `screenshot`, `eval` — are accurate, fast, consistently shaped, and
pleasant to use. The ones that *act on* a page — `click`, `type` — dispatch untrusted synthetic
events with no hit testing, so they report success on elements no user could reach. And the
ones that *manage the session* — `launch`, `daemon`, `network` — are where the last four weeks
of regressions live. The README calls it "a fast Rust CLI for the Firefox Remote Debugging
Protocol," which is true and undersells the good part while hiding the bad: what it is really
good at is being an **LLM agent's eyes on a page**. What it is not, despite the command list,
is a browser automation driver.

## 2. What it is genuinely good at

Backed by lane evidence, not assertion.

**The JSON envelope is the best thing in this codebase.** 30 commands surveyed; every one
returns `{results, total, meta}` with disciplined extras (`truncated`+`hint` on lists,
`sampled` on contrast audits, `summary` on console). `--limit 3` still reports `total: 60` and
sets `truncated: true` with a hint naming `--all`. That is a coherent contract someone designed
and then held to across 150 iterations. It is the asset worth protecting.

**Error messages are unusually good.** Real examples from the lanes:

```
selector '#laneC_hidden' not ready — the 1 matching element is hidden (after 32ms, timeout 2000ms)
port 6113 is in use by firefox (PID 4204), which ff-rdp did not launch (no owner-PID marker).
  Refusing to stop a process ff-rdp does not own
URL scheme 'file:' is not allowed by default; pass --allow-file-urls to opt in
  (exfiltrates local files via subsequent page-text/eval/screenshot)
```

They name the discriminating detail, not the category. The `--allow-file-urls` message explains
the *threat model*. This is craft.

**Navigation error handling is precise.** Distinct typed errors and distinct exit codes:
404 → `{"status": 404}` exit 0; bad DNS → `error_type: nav_dns_fail` exit 7; malformed URL →
exit 1; `javascript:` refused unless opted in. Verified against all four cases.

**Page inspection is trustworthy.** `dom`, `page-text`, `snapshot`, `geometry`, `computed`,
`styles`, `cascade` were cross-checked against independent `eval` measurements and matched.
`--fields name`, `--sort name --asc/--desc`, `--jq`, `--jq-strict` all behave correctly.

**The scripting trio is better than its reputation.** I predicted `run`, `record` and `index`
were iteration filler. Wrong on all three. `run` does per-step JSONL with `--dry-run` and
`--continue-on-failure` and a real summary; `record` → `run` **round-trips** (5 steps recorded,
5 replayed, 5 succeeded); `index` respects robots.txt and bounds correctly with
`--depth`/`--max-pages` (21 HN pages in 20s). These stay.

**`eval` handles multi-statement scripts.** `const x = 5; x` → `5`. Multi-line via `--stdin` →
correct. The ~290 lines of statement-boundary detection built in iter-142 work.

## 3. What it does badly, or pretends to do

### 3.1 `click` and `type` cannot do what their names imply — and this is a ceiling, not a bug

`click` dispatches `new PointerEvent`/`MouseEvent` via `dispatchEvent` (`js_helpers.rs:613-654`)
with **no `clientX`/`clientY` in the options at all**. Measured: a button under a full-screen
overlay (`elementFromPoint` → the overlay) still returned `{"clicked": true}` and fired its
handler with `isTrusted: false, x: 0, y: 0`. `type` sets `.value` and dispatches only `input`
and `change` — **no `keydown`/`keypress`/`keyup`** — so key-driven UIs never respond while the
command reports `{"typed": true}`.

The repo's own research (`kb/rdp/client/remote-agent-cdp.md`) establishes that trusted input is
**architecturally unreachable** over devtools-RDP: the only trusted-input surfaces are Marionette
and WebDriver BiDi, both peer protocols to RDP. There is no `Input.dispatchMouseEvent` to wire up.
DEC-008 chose `evaluateJSAsync` for these commands and listed its trade-offs; untrusted input was
never among them.

So the honest fix is **not** trusted events. It is hit testing — and the technique is *already
written down in this repo*, in `kb/skills/ff-rdp-debug-playbooks.md:232-240` (playbook C3,
"invisible consent overlay captures pointer events"), as an instruction telling a **human** to run
`elementFromPoint` by hand. The knowledge exists and was never folded into the command.

`entered: true` compounds this. It is set the instant `querySelector` returns non-null, before any
dispatch. It means "selector matched" and its name implies "pointer could enter."

### 3.2 The daemon's network watcher has been dead since 2026-08-10, and a workaround hid it

Clean-room A/B on one Firefox instance:

| path | result |
|---|---|
| daemon, plain navigate | buffer empty; `network --source watcher` → **0 entries** |
| `--no-daemon`, same page | **10 entries** with `method: GET`, `status: 200`, `content_type`, `source: "watcher"` |

Firefox 153 supplies full HTTP metadata over RDP. The daemon never receives it. Consequence for
every daemon-mode user: `method`, `status`, `content_type` and `transfer_size` are **null on every
request, always**, because `network` silently falls back to the Performance API.

**Root cause** (`daemon/server.rs:447`): `establish_watcher` calls
`get_watcher_with_options(…, Some(true))` — `isServerTargetSwitchingEnabled: true`. The working
direct path calls plain `get_watcher()`. The flag's own doc comment at `actors/tab.rs:164-167`
reads:

> **CAUTION:** enabling this flag also changes *where* the top-level target is delivered …
> **do not flip this on the default target-acquisition path**; use it only for frame-aware callers.

The daemon's core resource watcher *is* the default target-acquisition path. Commit `1612e509`,
**iter-137**, 2026-08-10 — whose own plan records the pre-change state as *"daemon → 77 rows,
source: watcher."* It worked; iter-137 broke it while legitimately fixing frame targets.

> **CORRECTION (iter-159, 2026-08-14): the root cause above is wrong.** The A/B measurement
> stands; the attribution does not. `isServerTargetSwitchingEnabled` does not move
> `network-event` delivery — a recorded frame
> (`crates/ff-rdp-cli/tests/fixtures/resources_available_network_server_target_switching.json`)
> shows it arriving `from` the **watcher** actor under exactly that flag, because
> `TYPES.NETWORK_EVENT` lives in `ParentProcessResources` and the WatcherActor emits it itself.
> `is_watcher_event` accepted every one of those frames. What actually broke it: the daemon
> skipped buffering any resource type with an active stream subscriber, and since iter-138 a
> *plain* `navigate` opens a `network-event` stream — plus two wrong wire shapes in the buffer's
> own serialisers and a `tabNavigated` that arrives at load *stop*. See
> [[iterations/iteration-159-daemon-watcher-regression]] and DEC-032. This entry is left in
> place, uncorrected above the fold, as the ninth instance of the pattern §5 names: a confident
> root cause that the wire did not support.

**Why nobody noticed — this is the important part.** `navigate --with-network` pushes its own
capture back into the daemon's buffer via a `store-events` RPC. Demonstrated:

```
1. fresh daemon buffer:       (empty)
2. DIRECT --with-network:     5 requests captured
3. daemon network --watcher:  7 entries      ← the daemon never captured these
```

Any workflow that touches the direct path once leaves data behind that makes the broken daemon
path look healthy. I hit this myself mid-session and briefly concluded the review was wrong.
**A workaround built to paper over unreliable buffering now masks the outage it was built for.**

**And the gate was green because the same commit made it green.** iter-137's own daemon-parity AC:

> `live_137_network_source_parity`: **PASSED with `--source performance-api`, 3 rows in both modes.**

The commit that broke the watcher source added the `--source` pin that let its parity test pass
by not exercising the watcher source. The test that *would* have caught it —
`live_128_network_detail_uses_watcher`, which asserts `source: "watcher"` with non-null `method` —
is `#[ignore]`-gated behind `FF_RDP_LIVE_NETWORK_TESTS` and has not run since 2026-08-10.

### 3.3 `launch` fails under load, and the test suite converts that into a silent pass

`launch.rs:540` waits a hardcoded `Duration::from_secs(5)` for the debug port. Measured: Firefox
binds at **7s** under load. `ff-rdp launch` failed **5/5 attempts** at load 6.8. The global
`--timeout` is not threaded into this call. The error text — *"is the port already in use?"* —
blames a conflict that does not exist.

The suite cannot see it. `LiveFirefox::try_launch` (`tests/common/mod.rs:349`) spawns the real
product binary, and on failure:

```rust
if !output.status.success() { return None; }
```

`headless_on_random_port` retries 3× then returns `None`, and **all 167 call sites** do:

```rust
let Some(ff) = LiveFirefox::headless_on_random_port() else {
    eprintln!("…: Firefox not available — skipping");
    return;                                    // ← libtest reports `ok`
};
```

Worse, libtest captures test stderr and discards it for passing tests — confirmed: **zero**
`LiveFirefox: pid=` lines appear in a log containing 170 passing tests. The skip notices are
invisible. There is no way to tell how many `ok` results reached Firefox.

**This is the exact defect iter-155 was built to eliminate, surviving through a second door.**
iter-155 spent 845 LOC making unmet *env gates* report `ignored` instead of a fake `ok`. The
larger fake-`ok` source — Firefox failed to launch — was untouched, is invisible, and is counted
as `executed` by `live-sweep`, whose `executed=N` is computed from static classification before
any process spawns (DEC-031 states this as a feature).

### 3.4 `launch --replace` (iter-153, merged today) does not work, and its tests avoid the path

Three attempts by a lane, three errors: twice *"no owner-PID marker"* against instances ff-rdp
had launched itself, once *"port still in use after stopping the prior instance."* The claimed
single envelope with `meta.replaced` never occurred.

Traced cause (`daemon/client.rs:1149-1163`): the `DaemonRecord` is removed **before** checking
whether the port actually freed. A failed stop deletes the ownership trail; the retry falls
through to the raw port-owner branch and hits the fails-closed guard.

Compounding it, `kill_pid_and_wait_port` kills the PID **before** calling `run_escalation`, whose
first guard is "if the pid is already dead, bail." The entire SIGTERM → SIGKILL-pgid → tree-kill
ladder — the mechanism designed to reach orphaned children holding the port — **never executes on
the primary path**. That is the `"port still listening after 8s"` message.

All three `live_153_*` tests run Firefox under the real `$HOME` while pointing `FF_RDP_HOME` at a
tempdir, so the `DaemonRecord` lookup is a guaranteed miss *by construction* — the module doc says
so — forcing the registry branch. **The tests pass because they exercise a different code path than
users hit.** And tonight's sweep, the first full qualified run ever, failed on exactly
`live_153_replace_emits_single_envelope` — a test that passes in isolation and fails under
contention, which is the 5s launch bug again.

The AC ticked six hours earlier reads:

```
- [x] live_153_replace_emits_single_envelope: …
      [verified: 2026-08-13, `FF_RDP_LIVE_TESTS=1 cargo test … live_153` 3 passed / 0 failed]
```

The annotation is **truthful**. That run happened and did pass. It certified a broken feature
anyway. This is the clearest available evidence that DEC-030's `[verified: …]` requirement does
not do what it was built to do.

**The sweep's failure text closes the loop between §3.3 and §3.4 — they are one defect:**

```
---- live_153_replace_double_envelope::live_153_replace_emits_single_envelope stdout ----
panicked at crates/ff-rdp-cli/tests/live/live_153_replace_double_envelope.rs:121:5:
  FAIL — launch --replace returned non-zero
  stdout={"error":"Firefox started (pid 37649) but debug port 59593 is not reachable
           after 5s — is the port already in use?"}
```

`--replace` did not fail on its own logic here. It failed because `launch`'s hardcoded 5s port wait
lost the race under contention — the §3.3 bug, surfacing inside the §3.4 test. Run alone, the race
is won and the test passes; run in a full suite, it is lost. **That is precisely why isolated
verification of a `live_*` AC is worthless as evidence, and it is the strongest argument in this
document for running the whole sweep rather than the one test an AC names.**

### 3.5 Silent-failure flags

- `--fields bogusfield` → `results: [{},{}]`, exit 0. Silently destroys the data.
- `--sort nosuchfield` → document order, exit 0. Silent no-op.

Both operate on untyped `serde_json::Value` with no schema check. `--jq-strict` proves the project
already believes in strict modes; it was applied to exactly one flag.

- `--log-level debug` → zero output on ordinary commands. The filter is correct; there are only
  22 `debug!` sites and **none in the command paths**.

### 3.6 Smaller, verified

- **`--stringify` cannot take multi-statement scripts.** `eval 'const x=5; x'` works;
  `eval --stringify 'const x=5; x'` fails, because `user_script` is spliced into a call-argument
  slot: `format!("(function(){{return {STRINGIFY_HELPER}({user_script});}})()")`. The CLI's own hint
  text tells you to use `--stringify`. The file already contains the statement-splitting machinery
  that would fix it (built in iter-142 for `await`); `--stringify` just doesn't use it. Undocumented
  in `--help`.
- **`eval` is the one command that silently truncates long strings.** `js_helpers::resolve_result`
  calls `LongStringActor::full_string()` and serves ~18 commands. `eval::run` uses `to_json()` and
  returns the ~1000-char preview grip, with no `meta.truncated`. DEC-015 added `full_string()` for
  "any consumer that evaluates JS producing large output"; iter-102's longString sweep covered
  `dom_walker`/`storage`/`page_style` and missed `eval`.
- **A fake-green test that constructs the defect.** `eval.rs:884`
  `build_script_never_emits_eval_for_any_combination` iterates `"const x = 1; x"` × `stringify=true`
  — generating syntactically invalid JavaScript every run — and asserts only `!s.contains("eval(")`.
- **`network`'s `--jq` divergence.** `network.rs:326` folds `cli.jq.is_some()` into a `use_detail`
  flag that switches `results` from summary-object to entries-array. Verified network-only;
  `console`, `a11y`, `perf`, `sources`, `cookies` are single-shape. The help text's
  *"`--jq` operates on the full envelope"* is false for this command.
- **`a11y contrast --fail-only`** on a commercial site: 32 checked, **0 failures**, `"capped": true`,
  `"source": "js-fallback"`. A clean bill of health from a capped sample via a fallback path.
- **`consent accept`** returns `{"cmp": null, "action": null}` exit 0 with the banner still present.
- **`meta.eval_path`** is hard-set to `"page-await"` (`eval.rs:614`). A discriminator that
  discriminates nothing since iter-93.
- **`type body hello`** → *"layout did not stabilise."* The real exception ("not an input, textarea,
  select, or contenteditable") is thrown correctly and then **discarded**; `diagnose_selector_failure`
  re-probes blind, checks only match-count and hidden-ness, and falls to a hardcoded else-branch.
- **`launch --profile <dir>`** fails if the directory doesn't exist — `ensure_devtools_prefs` creates
  the file but never `create_dir_all`s the parent.
- **`navigate --with-network` burns its full timeout** even on success (`drain_network_events_timed`
  is wall-clock, not idle-based), and **conflicts with `--auto-consent`** at the clap level, so you
  cannot dismiss a banner and capture network in one call.

## 4. What comes out

### Delete outright

| Item | Reasoning |
|---|---|
| `meta.eval_path` field (`eval.rs:614`) | Constant since iter-93. Update the assertion in `live_61r_eval.rs:90-94`. |
| `build_script_never_emits_eval_for_any_combination` (`eval.rs:884`) | Asserts the wrong property on inputs that are broken. Strengthen or remove; do not leave as-is. |
| `serialize_network_resources_for_buffer` + the `store-events` RPC (`network_events.rs:220-286`, `server.rs:2724-2761`) | **Only after the watcher is fixed.** A workaround that masks the defect it patches — actively harmful to diagnosis. |
| `network.rs` auto-fallback bookkeeping (~150-200 lines, `network.rs:214-295`, `380-460`) | Exists only because the watcher can silently return nothing. Keep `--source` as an explicit opt-out; delete the parity-massaging. |
| `EscalationHooks` DI indirection (`daemon/client.rs:63-167`) | Tests a ladder that never runs in production. Fix the neutering bug first, then reassess. |

### Fix, do not delete

- `establish_watcher`'s `Some(true)` — **the single highest-value fix in the repo.**
- `launch.rs:540`'s 5s constant → configurable, defaulting to something ≥ 30s.
- `daemon_record::remove` before the `port_free` check (`client.rs:1151`, `client.rs:963`).
- The four duplicated SIGTERM→SIGKILL→poll sequences in `daemon/client.rs` → one function that
  captures pgid *before* any kill.
- `--fields` / `--sort` → validate against the union of keys actually present in the result set
  (needs no schema; mirrors `--jq-strict`).
- `--stringify` → route through the existing statement-splitting machinery.
- `eval::run` → call `LongStringActor::full_string()` like every other command.
- `diagnose_selector_failure` → thread the original exception through instead of re-probing.
- `click` → hit-test via `elementFromPoint`; split the envelope into "selector resolved" and
  "element reachable." Rename or redefine `entered`.

### Keep, against my own prediction

`run`, `record`, `index`, `doctor`, `profiles`, `cascade`, `manifest`, `throttle`, `emulate`.
All were exercised and all work. I was wrong to suspect them.

### Test-suite changes that matter more than any gate

1. **A failed `LiveFirefox::headless_on_random_port()` must fail the test, not return.** 167 call
   sites currently convert a product bug into a silent pass. This one change would have surfaced
   §3.3 the day it appeared.
2. **Un-`#[ignore]` the network-fidelity tests** or run them in the sweep by default. §3.2 sat
   undetected for three days behind an ignore gate.
3. **Assert effects, not self-reports.** `live_140_ref_click_resolves` asserts `clicked == true` —
   the command's own claim. The overlay case shows why that is not evidence.

## 5. What the discipline machinery is worth

**Net: it cost more than it returned, and it is not close.**

11,700 LOC (xtask 5,907 + xtask tests 2,165 + tools 3,634) against ~52,000 lines of non-test
product source — **22%**. From git history, across ~20 gates:

**Real catches: two.**
- `lint-dogfood-script.sh` — iter-86's `grep -qi 'headless'`, which false-passes against the note
  text "…regardless of headless mode…". A genuine false-green.
- `check-firefox-refs` — two false Firefox spec citations (`7ed0852`: a plan cited
  `devtools/server/actors/performance.js`, which does not exist at that path).

**Against:**
- **Two "required checks" are self-declared no-ops.** `.github/workflows/ci.yml` step names read
  `Check firefox_refs (no-op in CI — no Firefox checkout)` and
  `Check oneway conformance (no-op in CI — no Firefox checkout)`. `check-oneway-conformance` is
  291 LOC that has never executed a real check.
- **`check-dead-primitives` induced dead code and then failed to see it.** A `DemuxReader::new()`
  decoy was constructed in `daemon/server.rs` *specifically to pass the gate*; 425 lines of dead
  public API shipped and survived every CI run. A human review found it.
- **28 commits exist whose entire content is rewording a plan so a gate stops firing.** `c51656d`:
  1 file changed, 12 insertions, 13 deletions, all in the kb plan. The AC changed from
  `perf audit`/non-headless to `perf vitals`/headless — different command, different mode — to match
  whichever test existed. Gate: 11/11 PASS.
- **`branch-protection.sh` was falsified in the field.** `d6f31c4`: *"main was in fact unprotected
  the whole time and nothing revealed it."*
- **`check-iteration-plan` was wrong on 142 of 142 merged plans** for ~2.5 months — the repo's own
  commit says this was *"training people to ignore its output."*
- **`check-todo-annotations` guards an empty set.** 0 TODO/FIXME/XXX in product source.
- **`check-pre-fix-repro` is 1,192 LOC — 20% of xtask — with zero catches**, and consumed iterations
  91 and 96 plus a dedicated bug file to stop flapping.
- **`ac-fidelity-check.sh` exists in four byte-identical copies** (1,856 LOC of the same script), and
  a 208-LOC gate (`check-discipline-regression`) exists solely to keep the copies in sync — a
  maintenance obligation created entirely by the deployment shape.
- **`branch-protection.sh` is worse than falsified — it is self-defeating.** Verified:
  `tools/branch-protection.sh:24` sets `REQUIRED_CHECK="live-tests"`, but `.github/workflows/live.yml`
  states in its own header comment that it *"no longer runs per-PR"* since iter-117. If that
  protection config were ever applied, **no PR could ever merge**, because the required context can
  never report. It also shells to `python3` (`:53`, `:66`) in a repo whose CLAUDE.md forbids Python
  scripts.
- **Two tests exist whose only job is asserting that CLAUDE.md contains certain strings.**
  `crates/xtask/tests/claude_md_lists_new_gates.rs` asserts the literals `check-firefox-refs` and
  `check-actor-kb-sync` appear in CLAUDE.md; `discipline_docs_mention_aggregator.rs` asserts
  CLAUDE.md *and* CONTRIBUTING.md *and* `~/.claude/skills/create-pr/SKILL.md` all mention
  `check-iteration-ready`. Editing the documentation turns the build red. This is the purest
  instance of machinery inspecting machinery in the repository.

The strongest case *for* is a replay experiment showing the strengthened `ac-fidelity-check` would
have rejected three of four false **security** ACs in iter-61w — but the version live at the time
**passed all four**. It proves the concept, not the deployed gate.

**The decisive evidence is from tonight.** Every gate was green while: the daemon's network capture
was dead for three days, `launch` failed 5/5 under load, `--replace` shipped broken with a truthful
`[verified: …]` annotation, and `eval --file --stringify` couldn't run a two-line script. The
machinery audits *plan prose*; the product broke underneath it. Worse, it is **structurally unable**
to catch these: `ac-fidelity-check` reads a diff and a plan and cannot know a test ran (DEC-030 says
so), and live tests never run in CI, so nothing downstream executes them.

**Recommendation: shrink hard.** The minimum viable set is three, and **none of them reads
acceptance-criteria text** — that is the finding, not a coincidence:

1. **`live-sweep`, actually run.** Every defect found today came from running the product; no gate
   saw any of them.
2. **`check-live-test-layout`** — cheap, guards a demonstrated expensive failure (`abe759b`: ungated
   live tests hung the Firefox-less Windows runner to a 10-minute job timeout), and it is
   *structurally required* by (1): 45 separate live targets instead of one consolidated target is the
   difference between a sweep that runs and one that doesn't.
3. **The dogfood apparatus** — the skill, `lint-dogfood-script.sh`, and `check-dogfood-script`. The
   only machinery in the repo with a track record of finding *product* bugs: `iteration-85:68-73`
   (iter-84's dogfood_path cited a `--debug-events` flag that does not exist on `navigate` — direct
   proof the path was never executed before ACs were ticked), iter-87 Theme E, and tonight's four.

Two gates deserve better than the forensics gave them, and the reason matters for what replaces the
rest. **`check-firefox-refs` is the only gate that checks a claim against ground truth *outside the
repository*** — the actual Firefox checkout — rather than against the repo's own prose. Both its
catches were false claims stopped before merge. That is exactly the shape recommended for the AC
replacement, already working, in 216 LOC: keep it as the template, and delete only its CI step.
The `stderr_scan` pair (`check-error-envelope-paths` + `check-stderr-annotations`) scans product
source for a specific real defect shape — a command writing to stderr and exiting, bypassing the
JSON envelope — which tonight's dogfooding hit twice. Keep both, merged into one subcommand and one
CI step. `check-iteration-plan` also stays: its 142/142 failure was a one-word vocabulary mismatch,
now fixed, and `parse_plan` is a library dependency of three other tools.

Everything else is a candidate for deletion, in this order of confidence: `check-pre-fix-repro`,
`check-oneway-conformance`, `check-todo-annotations`, `claims-vs-code.sh`, `branch-protection.sh`,
`check-dead-primitives`, `check-iteration-ready`, both doc-mention tests, then the
`ac-fidelity-check` family once (3) exists. Net: ~4,800 in-repo LOC, ~41% of the machinery; the CI
`discipline` job goes from 10 steps to 4.

**Removal order is not free — the aggregator must go first.** `check-iteration-ready`'s tests
hard-code the sub-check count (`12/12 PASS`, `[N/12]` loops at `tests/check_iteration_ready.rs:106,
153,161,222-230`). Removing any sub-check while it lives makes every deletion a two-file edit plus a
count bump — churn already paid for twice (`17574a6`, `97a9ed9`, as the count went 10→11→12).

**One non-invertible constraint:** `check-discipline-regression`'s replay shells
`run-iteration.sh --replay 61v/61t`, and `run-iteration.sh:83` calls `claims-vs-code.sh`. Delete
`claims-vs-code.sh` first and the regression check fails on its own removal path. So the
`ac-fidelity` shrink and the `claims-vs-code` deletion must land in all four copies **while
`check-discipline-regression` still runs** — it is the only thing that would catch a 3-of-4 edit,
and a 3-of-4 edit is exactly what happened in iter-140/146 (`3dc5330`). Only then delete it. Per
CLAUDE.md:170-172 this phase must be hand-driven; it cannot run through ralph-loop.

**On the run-log store, I have changed my position.** My first draft said don't build it. The
counter-argument is better. DEC-031's stated objection is that *"an agent can paste a fabricated
`executed=17` exactly as easily as a fabricated `109 passed`"* — true, but **forgery is not this
repo's failure mode**. The failure mode is 28 commits of *rewording*, which happened because editing
prose was the cheapest path to green. Nobody faked a test result; they nudged wording until a grep
stopped firing. Move the cheapest path to green from "edit the AC" to "run the sweep" and the reflex
inverts — forging a 189-entry JSON keyed on real test names and a real SHA is a categorically
different act, and it leaves an artifact.

It is also much smaller than DEC-031 assumes. `live_sweep.rs` already computes the qualified set,
per-test names and libtest statuses — verified, it writes **nothing** to disk (zero `File::create`
in the file). An `--emit target/live-run.json` carrying `{git_sha, timestamp, tests: {name → status}}`
is a serialization change against an existing structure: roughly 100 LOC replacing ~1,600
(`ac-fidelity-check.sh` × 4 at 464 each, plus 413 lines of its tests, plus the 208-line mirror-sync
gate that exists only because the script is quadruplicated).

**Two conditions, both binding.** It must *replace* the text heuristics, never sit on top of them —
otherwise it is one more layer. And it must not be built yet: `live-sweep` is one day old, 845 LOC,
and already has a bug filed against it. Betting the discipline story on yesterday's 845 lines is
precisely how `check-pre-fix-repro` happened. Run the sweep by hand for two or three iterations
first, pasting `LIVE_SWEEP_SUMMARY` into the PR body, and see whether the habit sticks before
automating it.

## 6. What would make the next 20 iterations product work

**Run the tests instead of gating the prose about them.**

Tonight's qualified `live-sweep` was the first full run in the project's history. Final result:

```
LIVE_SWEEP_SUMMARY executed=197 skipped=25 total=222
EXIT=1                                          # 49.5 minutes wall
190 passed · 7 failed across 5 targets
```

**One of the seven was a real product defect** — `live_153_replace_emits_single_envelope`, a feature
merged six hours earlier. The other six were all one environmental cause: the `ff-rdp-core` live
tests do not launch Firefox, they connect to a pre-existing instance on port 6000 (their own
`#[ignore]` text says *"start Firefox with `--start-debugger-server 6000`"*), and `live-sweep`
neither provides that nor checks for it.

That is a **third** way `executed=N` fails to mean what it says, distinct from the two in §3.3:

| how a test can be counted `executed` without testing anything | status |
|---|---|
| env gate unmet | fixed by iter-155 |
| Firefox failed to launch (§3.3) | silent `ok`, invisible, counted as executed |
| Firefox not pre-started on port 6000 | hard failure, counted as executed |

One run found what six weeks of gates did not. Running
`FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- live-sweep` once per iteration would have caught §3.2 on
2026-08-11, §3.4 today, and §3.3 whenever load first spiked — and it costs 50 minutes of machine
time, not 11,700 lines of Rust and shell.

This gate is **local and stays local**: `live.yml` has not run per-PR since iter-117 (permanently red
from environmental runner failures, ~27 min/PR, advisory, no signal) and is now
`workflow_dispatch` / release / weekly with `continue-on-error: true`. GitHub runners have no
Firefox. "Run it in CI" is not available; "run it before you open the PR" is.

Three changes, in order:

1. **Make the live suite honest, then run it every iteration.** Fail on unavailable Firefox instead
   of returning; un-`#[ignore]` or sweep-run the network tests; assert effects rather than
   self-reports. Without step one the run is decorative — a green sweep currently cannot distinguish
   "passed" from "never reached Firefox."
2. **Fix the four session-layer defects** — watcher (§3.2), launch timeout (§3.3), `--replace`
   record removal and escalation neutering (§3.4). These are the whole gap between "an inspector
   that works" and "a tool you can trust in a loop."
3. **Decide what ff-rdp is for, and delete the rest.** The evidence says it is *an LLM agent's eyes
   on a page*: the envelope, `--jq`, the hint system, `snapshot`, `--format text` token economy, and
   the honest error messages all point one way. The write path points the other way and cannot be
   made honest over this protocol. Either scope `click`/`type` down to what they can truthfully
   promise (hit-tested, untrusted, documented as such) or drop the pretence of being an automation
   driver. Ambivalence between the two is what produced a `click` that returns `true` for an
   unreachable element.

**The process rule that matters more than any gate:** an iteration that changes zero product source
is not an iteration. Four of the last six were tooling fixing tooling. The chain 154 → 155 → 156 →
157 was each step locally defensible; the cure is not a better gate, it is refusing to start the
chain.

**Close 156 and 157 as obsolete.** Both are tooling fixes for tooling merged in the last 48 hours.
156 addresses friction in a gate this analysis recommends removing; 157 files a bug against
`live-sweep`'s classifier, and tonight's run shows classification is not what is wrong with the
sweep — the silent-skip path is, and that lives in the test harness, not the classifier.

## Findings that did not survive verification

Recorded because the point of this exercise is that stated causes have diverged from reality
repeatedly, and this document should not add to that.

| Claim I made | What checking showed |
|---|---|
| "The test harness waits 30s where the product waits 5s, so the suite is engineered around the bug" | The 30s applies *after* the product already succeeded (dead weight), and to a different launcher used only for kill-scoping tests. The real mechanism is retry-3×-then-silently-skip. Conclusion unchanged, mechanism wrong. |
| "`eval` cannot evaluate any statement that declares a variable" | Only `--stringify` breaks. Bare `eval`, `--file`, `--stdin` all handle multi-statement scripts. Every repro I ran had used `--stringify`. |
| "`--redact-threshold` is a no-op" | **Retracted.** Its help says "un-keyed string values *in trace output*." I tested `results`, which it was never meant to touch. |
| "`--max-frame-mb` is a no-op" | **Retracted.** It caps RDP frame payloads and has passing unit tests. My 2 MB test used a longString, which is chunked and never crosses as one oversized frame. Not verified either way. |
| "`run`, `record`, `index` are never-needed iteration filler" | Wrong on all three. All work; `record`→`run` round-trips. |
| "eval's complexity is 2,242 lines across eval.rs + js_helpers.rs" | ~130 lines of `js_helpers.rs` are eval-adjacent; the rest serves click/type/wait/scroll. Overstated ~8×. |
| "The live sweep is on a 6-hour pace" | Startup artefact. Steady state ~3.5 tests/min; 49.5 min total. |
| "The sweep finished 188 passed, 1 failed" | Quoted the first target's line mid-run. Final across all five targets: **190 passed, 7 failed**. Six of the seven were `ConnectionRefused` from core tests needing a manually-started Firefox on port 6000. |

## Links

[[step-back-2026-08-13]] · [[decision-log]] (DEC-008, DEC-015, DEC-020, DEC-030, DEC-031) ·
[[iteration-137-daemon-mode-parity]] · [[iteration-153-launch-replace-double-envelope]] ·
[[iteration-155-live-skip-reports-green]] · [[iteration-156-ac-fidelity-names-its-test]] ·
[[iteration-157-live-sweep-classifier-drift]]
