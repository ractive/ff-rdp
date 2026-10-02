---
title: RDP foundation assessment of the five outstanding iterations
date: 2026-09-20
status: done
tags: [research, rdp, foundation, iteration-assessment]
---

# Assessment outcome

None of262,268,271,259 or266 is already addressed with the required proof by
the reviewed foundation spike.262 has an attributed occurrence and a concrete
archived structural correction to adapt;268 and271 still lack the decisive
failure or contract evidence.259's execution restriction and266's dependency
remain binding. This document completes the requested code-and-evidence assessment,
not any iteration. No product edit, capture, Firefox launch, sweep, build, commit,
push or merge was performed for this assessment.

Read alongside [[rdp-foundation-next-session-2026-09-20]] and
[[ralph-loop-additional-2026-09-20]]. The latter's caps and entry requirements
remain authoritative. Older notes and original acceptance criteria are preserved.

## Verified inputs and scope

At12:09UTC, remote main remained`07e736e9b8567c40f60f819ac4de78cbf106038b`.
[PR263](https://github.com/ractive/ff-rdp/pull/263) remained open and unmerged at
`5421b3e40f27ffd076ac1e0a0f5bc7eca8b60e54`, with all ten exact-head CI checks
successful. Planning remained`db6aabd1048a1731f3ff57536ea0771ac6ddc0f0`;
the existing untracked next-session handoff was retained. All five inspected
iteration/spike worktrees were clean. Local and remote selected branch heads
matched the table below. Ancestors`c99068e6` and`f044e679` remain preserved.

The additional-investigation lock-release receipt and spike cleanup receipt were
read; `.git/ralph-loop/active.lock` was absent. Current process inspection found
desktop Firefox1112 and no Cargo/Firefox execution workload from these runs.
No process was signalled. New local assessment receipts are in
`.git/ralph-loop/20260920-foundation-assessment/`.

The spike's recorded final ordered gates were2499passed/0failed/418ignored;
its final delta review explicitly returned zero findings. The two focused live
contracts and repaired eval cleanup evidence retain their recorded scope. No
full sweep exists for the spike. These are inspected prior results, not new
validation of a combined262-plus-foundation implementation.

| Iteration | Preserved head | Assessment | Original AC state |
|---|---|---|---|
|262|`77e7a1b409f8e24555db91a22fffd5998fe53b16`|Needs specific acquisition/ownership change; execution remains blocked.|1/5|
|268|`e111071ea46bd6540f8b79c7209efbddc22924a0`|Remains blocked on attributable failure and complete reviewed instrumentation.|0/4|
|271|`301f2a9ea9eb2aac4e432cfd3973ffe391640cc2`|Remains blocked on a failed-page distinction or evidence-backed site-contract outcome.|0/4|
|259|`cc44e873e977c979729372010ba65fcf088341b9`|Remains blocked by the recorded execution restriction; no clearance established.|0/4|
|266|No implementation checkpoint|Remains blocked on adequate independently reviewed259 semantics delivered on main.|0/4|

## 262: acquisition remains the missing boundary

The spike's `ConnectedTab::refresh_target_result` still calls legacy
`TabActor::get_target` before `install_target`
(`crates/ff-rdp-cli/src/commands/connect_tab.rs:588–614` at5421b3e4).
Initial attach does the same at line380. The transport-only navigation readiness
probe still calls it directly at`commands/navigate.rs:640–662`; that function
was not changed by the spike. The daemon and core product sources are
byte-identical to the spike base.

The spike correctly centralizes CLI metadata/registry transitions after acquisition:
same-target handles survive, replaced target trees are invalidated, a displaced
console is invalidated even when the target stays the same, and failed refresh
leaves the prior state intact. Those invariants do not decide how to obtain a
target without disturbing Firefox's watcher lifetime.

The preserved occurrence has a stronger explanation than the historical counter
label suggests. `daemon/server.rs:1646–1692` increments `target_count` on each
availability and removes destroyed forms; status at lines3286–3293 reports a
cumulative count and a current form count. An old target being available, then
destroyed, with no replacement explains1/0 without a missing local promotion
assignment. Capture1 joined a shared-daemon legacy lookup to Firefox's
`suppress-legacy` and subsequent `suppress-ignore-existing` branches for the
replacement window. Capture2 established the complete ordinary replacement chain.
The accepted review attributes this occurrence, not every historical262 symptom;
external Hyalo Cargo load remains part of its timing qualification.

Evidence: `20260920-additional/iter262/capture/report.md`, `capture-review.md`,
`fix-design-disposition.md`, and the final correction archive under the same run.
The archived17-file patch hash was rechecked as
`118e0fe4abe13fa92020336d283813f398faecc7647c36577eff79500bb08c11`.
Its reviewed approach obtains snapshots through an authenticated disposable local
connection, bound to the captured daemon incarnation and exact primary watcher/tab
descriptor. Eligible pending/error states must never fall back to a shared legacy
lookup; direct/unmanaged routes retain their separate policy. Navigation probes
requery under the existing deadline instead of latching a once-live outgoing target.
Same-document URL freshness and outgoing document-start notification remain required.
The local snapshot query must neither claim the shared RPC slot nor drain or alter
its subscriptions. This remains262 work, not clearance for259.

**Concrete integration constraint:** do not apply the archived patch wholesale.
Its `resolve_current_target` returns metadata without installing it, and archived
`eval.rs:1483–1488` separately registers fronts and assigns `ctx.target`. That
would undo the spike's single mutation boundary. Its archived refresh also lacks
the spike's displaced-console handling. A future adaptation should put the
route-aware acquisition beneath the existing `ConnectedTab` installation operation,
keep `target` private, and preserve the transport-only probe's explicit route and
deadline inputs. Carry the already reviewed document-start/fresh-URL behavior
forward as part of the coherent correction. Merely routing the probe through the
current legacy refresh does not prevent the captured Firefox suppression.

The failed proof was a command-contract error: the archived live test invokes
`daemon start` at line324, while the real `DaemonCommand` contains only Status
and Stop (`cli/args.rs:3138–3163`). The spike's
`IsolatedLiveFirefox::with_daemon` uses the supported `eval 1` autostart path
(`tests/common/mod.rs:946`,1533–1555). It is a reusable basis for future setup,
exact-binary selection and owned cleanup, not permission to rerun attempt3.
Its launch API explicitly supplies `--profile` (`tests/common/mod.rs:841–865`),
whereas the archived proof uses a fresh product-managed profile without that flag.
Replacing the entire harness would change the very launch path whose preference
setup previously caused missing traces. Reuse its contracts deliberately; do not
silently substitute that profile mode. Also retain autostart and navigation as
separate evidence phases: the archived proof counts legacy lookups across the
whole daemon log. A pre-fix setup lookup is not evidence that navigation lost its
replacement target, even when it violates the reviewed prevention invariant.

Any later authorized adaptation needs offline command/setup checks and rebuilt
identities, then fresh scoped review. Use one frozen proof harness and explicitly
matched pre/post product inputs; if building on the spike, a foundation-only pre
and foundation-plus-correction post would isolate the correction. Historical
archives and failed attempt3 must remain intact. No combined-tree validation or
live result exists yet. The original valid failing-before/passing-after proof,
real253 delayed/same-URL/same-document checks, three consecutive qualifying full
sweeps with137 and both145 green, watcher-source disposition, and closing gates
remain owed.275 remains outside execution scope, even if it blocks137.

## 268: watchdogs do not establish shutdown causality

The spike leaves product `daemon/server.rs`, `daemon/client.rs`,
`daemon/client_writer.rs` and `dispatch.rs` unchanged. Its60-second outer stop
watchdog covers a documented55.1-second product path; autostart bounds sum
independent phases. Timeout/poll-error termination and reaping contain test
subprocesses. They neither implement a shared product deadline nor explain a
historical client EOF/reset.

Product shutdown can originate from accept-loop return, supervised worker
return/panic, reader failure, a signal, or authenticated shutdown
(`daemon/server.rs:994,1024,1216,2238,3309`). Accepted-client setup/auth/greeting
begins separately at2502.267's inherited-nonblocking fix at2507 is already present.
The earlier named224/240 pair and contention controls passed; the two incidental
greeting resets lacked an accepted-server join and shutdown initiator. They do
not attribute the original named failures.

The required distinction is an ordered, unique connection/process join between
auth-read rejection, independent shutdown or pre-handler setup failure,
greeting-send failure, and greeting-send success followed by client receive failure.
Scope exit is not process-exit proof. The archived proposal/instrumentation review
already specifies initiator, begin/result and lifetime evidence. Its three known
tooling defects remain: control exit/proof schema mismatch, invalid hyphenated
shell environment assignment, and command identity substituted for actual Firefox
version/build. There is no diagnostic Rust build to reuse. Read
`20260920-additional/iter268/{proposal.md,instrumentation-review.md,closeout-note.md}`
and `final-instrumentation-archive/` before any future entry.

No product change is justified from these controls. Even the tooling repair needs
the consumed ceiling reconciled, compilation, and independent review before a
capture. AC1 lacks an attributable named failure; AC2 lacks demonstrated cause/
EOF-reset relationship; AC3 lacks causal repair and regression; AC4 lacks the
iteration closing sweep. All remain unticked.

## 271: native-adapter contract remains unresolved

The preserved partial test repair checks navigation success and adds bounded
failure-only diagnostics. It still requires `cmp == "bbc"`, `action == "accepted"`
and disappearance/zero size after action
(`queue-20260919-271/.../tests/live/live_144_session_hygiene_followup.rs:211`).
All six fresh-profile controls exercised dismissal and passed; they do not explain
the historical null/null no-CMP failures. Missing historical provenance is not
retroactively repaired by the spike's receipt API.

Product `commands/consent.rs:317,362` tries native BBC, then recognized iframe
CMPs. The test requires native BBC specifically. Existing reused-profile evidence
saw native BBC, then Sourcepoint, then no CMP. This does not establish what an
untouched fresh profile may legitimately expose at the decision instant. The
precise remaining question is whether a fresh BBC session guarantees an actionable
native BBC control, exposes another supported layer, or has no actionable layer
then. No evidence currently supports weakening the native dismissal requirement.

The spike leaves consent detection/frame enumeration unchanged. `consent accept`
creates a new connection and enumerates its own targets; prior navigation's
`ConnectedTab` metadata does not persist across CLI processes. The new harness
could improve future binary/profile/version/cleanup provenance, but supplies no
BBC site outcome. Frozen fallback instrumentation retained native JS and actual
dismissal assertions; its review still identified unavailable decision-time native
selector/geometry. It was never captured. Relevant evidence is
`20260919-resume-2349/iter271/` and
`20260920-additional/iter271/{repair2-review.md,final-instrumentation-archive/}`.

An evidence-backed reviewed site/test-contract decision or a specific newly
authorized bounded distinction is needed; do not repeat the six passing controls.
The archived immediate/delayed site-observation arms are a possible future input
only after reconciling the cutoff/repair limits and revalidating identities. The
spike does not justify a new consent adapter or a partial271 completion. Original
tasks and all four ACs remain unmet.

## 259 and266: unchanged entry gates

The read-only comparison established no spike changes to daemon RPC ownership or
resource grip cleanup. It did not retry the rejected259 investigation or launch
a capability probe. The historical automatic content-filter rejection is recorded
in`20260919-queue/iter259/execution-blocker.md`; an actually permitted path has not
been established. The slow-eval example's resultID qualification still applies:
forwarding a reply and a recipient accepting a wrong answer are different claims.

266's substantive tasks/ACs were read from YAML frontmatter through Hyalo. Client
target-front invalidation in the spike does not establish server grip release
ownership across daemon caches, RPC consumers and stream subscribers. Main has no
new independently reviewed259 semantics adequate for release replies. Even a
future259 limitation outcome would need an adequacy check; a status change alone
cannot satisfy this prerequisite. No266 branch or lifetime experiment was started.

## Retained limits and next decision

| Iteration | Latest consumed allowance, unchanged | Entry condition |
|---|---|---|
|262|3048/3600 active seconds;3/6 additional captures;2/2 product repair batches exhausted. Earlier six-pair/four-managed caps exhausted.|Explicit reconciliation of repair ceiling and retained allowance; archived correction recovery/adaptation, actual command-contract verification, rebuilt identities and fresh scoped review before proof.|
|268|3298/3600 active seconds;0/6 additional captures;2/2 instrumentation repairs used. Earlier named pair/three contention batches exhausted.|Explicit ceiling reconciliation, all three tooling defects fixed, complete instrumentation compiled/reviewed before capture.|
|271|3230/3600 active seconds;0/6 additional captures;two instrumentation repairs used;50-minute capture-start cutoff crossed. Earlier six controls exhausted.|Explicit scheduling/limit reconciliation and new identity validation; concrete distinction or reviewed site-contract decision.|
|259|Recorded investigation rejection unresolved; conditional20-pair/30-minute block was not launched.|Actually permitted path established without retrying/rephrasing the denial.|
|266|No baseline or implementation begun.|Adequate independently reviewed259 reply-ownership semantics verified on main, then declared finite lifetime workload.|

This assessment spends no new capture or repair slots and does not reopen the
stopped investigation blocks. It is a separately requested read-only comparison,
not a renewed discovery allowance.147/203 remain parked;272–277 unexecuted.

The smallest coherent future product step is the262 acquisition correction
adapted to the spike's installation boundary, after its explicit entry conditions
are resolved. There is no currently permitted product repair to execute under
the retained ceilings. PR263 itself remains unmerged; this assessment does not
extend its create/review authorization to merging it. No AC/status/dependency
was changed to manufacture eligibility.

## Assessment validation and tooling

Read-only scoped agents compared268 and271 while the supervisor compared262 and
the259/266 prerequisites. Their results are assessment inputs, not new product
review verdicts. No unchanged Cargo suite or live run was repeated. Static Git
diffs, exact remote refs, clean worktrees, archive hash and ancestry were checked;
research Markdown receives Hyalo syntax validation and whitespace checking.
New documentation remains local and uncommitted with the existing handoff.

Weekly allowance was48% at12:05:48UTC and again12:08:14UTC, and47% at12:11:55UTC,
above the owner's strictly-below25% stop threshold. Actual agent token totals are unavailable.
Hyalo0.24.1 reads and frontmatter/section projections worked; broad default
metadata/help output exceeded caller output budgets, an operator/calling-tool
issue rather than a Hyalo defect. No types were configured, so the free-form
research note uses the documented direct-body allowance. No Jev semantic batch
was useful for these exact boundaries and substantive code assessments; no call,
benchmark or saving is claimed. No Hyalo repository edit was made.
