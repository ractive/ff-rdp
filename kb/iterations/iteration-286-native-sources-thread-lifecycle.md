---
title: "Iteration 286: Native sources and current thread lifecycle"
type: iteration
status: planned
date: 2026-09-28
branch: iter-286/native-sources-thread-lifecycle
depends_on:
  - "259"
tags:
  - iteration
  - carry-over
  - protocol
  - sources
first_call_sites: []
dogfood_path: "Declare and independently review a finite owned-Firefox schedule for the real sources command: one baseline native-path occurrence and one matched repaired occurrence, recording native versus fallback selection, attach reply/event traffic and actual owned cleanup. Add one reviewed paused-debuggee safety control and existing fallback/error controls. Stop each answered hypothesis; no unchanged retries or full-sweep discovery. Run ordered workspace gates and this iteration's own reconciled dual-gate closing sweep after focused proof."
firefox_refs:
  - path: devtools/shared/specs/thread.js
    lines: "72-107"
    why: ordinary attach response and interrupt contract
  - path: devtools/server/actors/thread.js
    lines: "390-431"
    why: attach transitions without paused
  - path: devtools/server/actors/thread.js
    lines: "1583-1595"
    why: native sources implementation
kb_refs:
  - kb/rdp/actors/thread.md
  - kb/iterations/iteration-10-object-inspect-and-native-actors.md
  - kb/iterations/iteration-259-rpc-slot-handover-strands-an-in-flight-reply.md
---
# Iteration 286: Native sources and current thread lifecycle

## Problem and boundary

Installed Firefox156.0.1 declares an ordinary attach response and implements
attach as an already-attached no-op or transition to RUNNING, without a paused
event. Current `ThreadActor::attach` waits for paused; `list_sources` subsequently
assumes resume/detach cleanup, although detach is absent from the installed spec
and server. The real CLI `commands/sources.rs:73–86` uses this helper through
`connect_direct`. Its fallback at305–314 recognizes selected actor errors, not
timeouts. This is a current source-proven contract defect, not a new runtime
timeout finding. The September28 review executed no Firefox or tests.

These behaviors predate259.259 only gives attach its ordinary reply descriptor;
its ownership repair neither fixes nor causes the native lifecycle mismatch.
Serial placement after259 preserves its reviewed source and one-iteration PR
boundary; there is no claim that this work repairs historical268 failures.

## Tasks [0/4]

- [ ] Audit current supported attach, source enumeration and lifetime behavior,
      including already-attached/running and externally paused threads; qualify
      the installed package and concrete native caller before changing behavior.
- [ ] Run the declared bounded baseline, then repair the native sources flow to
      consume actual method completion, avoid unsupported detach, and preserve
      ownership of debugger state. Do not resume a pause this operation does not
      own. Use the current sources method rather than assuming it was removed.
- [ ] Add repair-sensitive coverage for an ordinary attach response without a
      paused event, already-attached state, paused-debuggee safety, interleaved
      events, errors and truthful fallback selection. Correct protocol notes
      and historical-summary claims with dated addenda, preserving history.
- [ ] Obtain independent review, matched focused native proof, ordered
      fmt/clippy/workspace gates and this iteration's required closing sweep.

## Acceptance Criteria [0/4]

- [ ] The actual sources command obtains a native source list from a controlled
      script-bearing page on the qualified Firefox package; evidence identifies
      the native path and actual expected script, so JS/walker fallback success
      cannot satisfy this criterion. Baseline outcome and packet attribution
      remain recorded even if they differ from the predicted timeout.
- [ ] Ordinary attach completion does not require an unrelated paused event;
      already-attached/running operation and independently paused-debuggee safety
      are demonstrated. No unsupported thread detach is sent, no unowned pause
      is resumed, and all owned runtime resources have actual cleanup outcomes.
- [ ] Focused regression and mutation controls detect restoration of the old
      wait/cleanup defect; native errors and fallback outcomes remain explicit,
      bounded and accurate. Interleaved events are preserved, and timeout
      increases or broad fallback-on-every-error cannot substitute for repair.
- [ ] Independent review, ordered workspace gates and a reconciled dual-gate
      closing sweep pass with every unrelated failure explicitly dispositioned;
      thread/source documentation matches the supported contract.

## Finite hypotheses and stop conditions

H1: on a controlled running target, attach completes without paused and the
current helper cannot reach native sources. One qualified baseline occurrence
answers this hypothesis; record an actor error or different result honestly.
H2: the reviewed correction returns native sources under the same setup. One
matched occurrence answers it. H3: an independently established paused target
retains its original pause after the command; one separate reviewed control
answers this safety question. Declare bounds and cleanup reserve before each
execution; a failed setup is not permission for another attempt. Do not fix only
the first wait and assume resume/detach semantics are then valid. Stop answered
experiments. Full sweeps are closing gates, not discovery tools. Any new causal
hypothesis needs a separate bounded reviewed schedule, preserving prior failures.

## Exact provenance

Source inspected:259 base `d2a17038945169d21ae9a9005cde967e3bab0769` plus reviewed
dirty source manifest `73fdcae5ebdd584ad64154cbeb39e81222b672355c1698aa3daa59ff1d4b5e58`.
Callsites: `crates/ff-rdp-core/src/actors/thread.rs:37–48,79–84,90–119`;
`crates/ff-rdp-cli/src/commands/sources.rs:73–86,305–314`.

Installed package: `/Applications/Firefox.app/Contents/Resources/browser/omni.ja`,
SHA256 `85f891cec3e54150027582ac74eb96fc3774cc0ab4bfd96f7192905149d05fff`;
Firefox156.0.1, BuildID20260921121718, SourceStamp
`6f2c158dfc7e9693f880fad2510ceb51a158c069`. Archive member prefix:
`chrome/devtools/modules/devtools/`. `shared/specs/thread.js:72–107,181–186`
SHA256 `2af56ba30d3c398735d5a9cffcd6ad1e0fbe3df807810e7934b51a10d9e4260f`;
`server/actors/thread.js:390–431,1308–1317,1583–1595`
SHA256 `509dab78d9319b4277eecac422093497d0fe9f565494c0d2eb4d548f7cd317ad`.
Spec interrupt at98–101 is not one-way; correct that existing note too.

No implementation or validation is claimed. All tasks/criteria remain pending.
No new public primitive is proposed; update first_call_sites if that changes.
