# ff-rdp project memory

## Takeover resumed — 2026-09-09

The owner explicitly lifted the pause and authorized Codex to finish iterations
252–256, discharge iteration 242's dual-gate sweep, correct plan 246's elapsed-time
conclusion without a PR, and complete iteration 257. Use the installed Codex
`ralph-loop` and `iteration-close` skills; do not launch the old Claude workflow.
The paused handoff below remains historical context, including its verbatim
review findings in `HANDOFF-iter-233-256.md` section 3.

The owner's current standing authorization is:

> I authorize Codex and its delegated agents to perform these iteration PR merges without per-PR approval when all CI checks on the exact PR head are green, an independent local review has run and its findings are addressed, and merging uses GitHub through gh pr merge --merge.

The supervisor owns queue advancement and verified merges. Use fresh implementers
and independent read-only reviewers, one writer in the checkout, and no messages
to running workflow agents. Resolve plan 255's dependency on plan 256 Theme A
before implementation while preserving one iteration per PR and the original ACs.
Reconcile all upcoming pending plans, including those outside this execution range,
after each iteration without executing the broader backlog.

Checkpoint recoverable progress frequently, but never commit failing code:
`cargo fmt`, strict workspace clippy, and workspace tests must pass in that order.
This overrides handoff section 8's broken-WIP instruction. Keep long quiet work
in the background with logs and bounded polling. Verify real sweep verdicts across
all tiers and preserve all profile-leak summaries and unmet acceptance criteria.

At resume, fetched `main` was `9e6ec3d8b3fa98a24b8583dda740e74d8caf1505`;
PR #247 was open at `f3f1ba19fc0d9467bce3c7ecef0bd5be04bdedcf`, ten checks green.
Eight actionable iterations in 233–256 were merged, not the handoff's claimed ten.
These are dated observations, not substitutes for checking current refs.

## Saved takeover — paused by the owner

On 2026-09-07 the owner supplied the autonomous iteration takeover below, then
explicitly said: **"But don't start it."** This is saved context, not an active
instruction to execute the run. Do not start the loop, fix or merge the PRs, or
perform the outstanding sweeps until the owner explicitly resumes this work.

Project: `/Users/james/devel/ff-rdp`, a Rust CLI driving Firefox through the Remote
Debugging Protocol; GitHub `ractive/ff-rdp`, base branch `main`. Verify the actual
checked-out branch and remote state when resuming; the handoff is a historical
snapshot, not proof of current state.

## Read first when resuming

Before touching the iteration work, read these in this order:

1. `HANDOFF-iter-233-256.md` at the repository root, in full. This is the
   authoritative handoff. Preserve section 3 verbatim: it contains the three
   review findings for PR #247, originally recorded nowhere else.
2. `CLAUDE.md`.
3. `.claude/CLAUDE.md`.
4. `CONTRIBUTING.md`.
5. `kb/discipline-rationale.md`, using `hyalo read discipline-rationale.md --format text`.

Follow the project rules in those files. Use `hyalo` for markdown operations in
`kb/`, without changing directory into it.

## Historical context to verify

The previous `/new-ralph-loop 233 256` run reported ten of thirteen pending
iterations merged; verify counts and merge SHAs rather than repeating that claim
as fact. The handoff contains the merge records. Iteration 252 was CI-green on
open PR #247 and blocked on review findings. Iterations 253–256 had not started.

Both launches died while a review agent waited on a headless review subprocess
in the foreground. The recovery is for the launching session to run the review
itself, address findings, and merge. The handoff also records a Claude account
entitlement error; verify current capability rather than assuming it persists or
has been resolved.

## Work order once explicitly resumed

1. Finish PR #247 (iteration 252). Address all three findings in handoff section 3.
   Finding 1 requires a measurement against real Firefox: prime the daemon with
   `ff-rdp console`, then count timer-generated logs from `console --follow` to
   detect double delivery. An argument is insufficient; this finding governs
   whether the PR can merge. Finding 2 requires non-live assertions for the
   `getWatcher` flag and request ordering, demonstrated with mutation checks.
   Finding 3 concerns dropped subscription catch-up events and requires a fix or
   an explicit, visible, evidence-backed semantics decision. Run the required
   gates, closing dual-gate live sweep, and review before merging.
2. Run `/new-ralph-loop 253 256`. Pass the verbatim standing merge authorization
   from `CLAUDE.md` as `mergeAuthorization` and handoff section 8's operational
   rules as `extraContext` launch arguments. Delegated prompts are the reliable
   control channel. Consult handoff sections 4, 5, and 8 for launch details.
3. Run the live sweep owed by iteration 242 (handoff section 6):
   `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep`.
   Treat discoveries as fix-forward work. Preserve unticked acceptance criteria
   until their requirements are actually satisfied.
4. Correct plan 246's inverted conclusion about
   `live_navigate_default_fast::live_navigate_elapsed_matches_wall` (handoff
   section 7). Wall-clock time includes work outside the internal measurement;
   contention can widen the gap. Record the existing isolation/sweep evidence
   accurately and move the row into the load-shaped set. This is a KB-only change;
   follow the owner's recorded preference for handling it without a PR.
5. Then execute plan 257: Firefox 155 broke every `--full-page` screenshot through
   the changed fourth argument to `drawSnapshot`. This is a shipped regression,
   not a test defect.

## Binding operational rules

- Green CI alone is not correctness evidence. Run a real local review and address
  its findings. "No findings were returned" is a stop, not a pass; an explicit
  zero-findings verdict from a review that demonstrably ran is different.
- Verify agent claims about counts and capabilities. Reconcile sweep counts over
  all tiers, not only the CLI tier.
- Never reword an acceptance criterion to fit the result. Leave an honest unticked
  box and explain unmet requirements.
- Never SendMessage a running workflow subagent: the prior harness forked a
  duplicate into the same working tree. Put run-wide instructions in launch prompts.
- The owner has standing authorization for iteration PR merges under the three
  conditions in `CLAUDE.md`: green CI on the PR head, completed local review with
  findings addressed, and merge through GitHub using `gh pr merge --merge`.
  Quote that entire authorization section verbatim into delegated prompts. Do not
  ask for per-PR approval again once the owner resumes this authorized run.
- Never squash iteration PRs or bypass GitHub with a local merge and push to main.
- Run potentially quiet long operations in the background and poll with short
  commands. Never wait on a review subprocess in the foreground. Apply section 8's
  bounded review retry and early handoff procedure if a reviewer is unavailable.
- Checkpoint and push recoverable progress frequently as directed in section 8;
  preserve the mandated commit gates in the project instructions.
- Do not work concurrently in this tree while the workflow is live: agents share
  the Cargo target directory, Firefox processes, and port 6000.
- Finish with verified merge SHAs and an explicit list of what remains undone and
  why. Never substitute a claimed merge or test result for verification.
