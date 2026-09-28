---
title: "Iteration 147: console locale reproducibility"
type: iteration
date: 2026-08-12
status: done
branch: iter-147/provision-language-pack-main-20260928
depends_on:
  - kb/iterations/iteration-144-session-hygiene-followup.md
first_call_sites: []
dogfood_path: >-
  Use an exclusively owned Firefox runtime/profile with verified German
  application locale and available English and German language resources. Record baseline
  and product-launched requested/available/application/default locales plus build
  identity. Run live_147_console_locale_pinned with a real engine-produced
  console/error message; setting LANG alone is not qualification.
tags: [iteration]
---

# Iteration 147: console locale reproducibility

Carried over from [[iteration-144-session-hygiene-followup]] Theme F, itself carried over from
[[iteration-142-session-hygiene]] — this is the second consecutive iteration this item has been
re-deferred, for the same reason both times: **no non-English-locale Firefox is available in the
implementation environment.**

## Why this keeps getting deferred instead of guessed at

`crates/ff-rdp-cli/src/commands/launch.rs`'s `USER_JS` constant already sets
`intl.accept_languages`, `intl.locale.requested`, and `intl.locale.matchOS` to pin English —
added in iter-61j (dogfood-51; confirmed still present via `git log -S intl.accept_languages`).
Dogfooding session 63 observed German console output anyway, on a later iteration than iter-61j.
Two live-Firefox environments were checked during iter-144's implementation:

- macOS Firefox (`/Applications/Firefox.app`) — the only Firefox installed in this implementation
  environment. English-only; ships no non-English langpack; headless Firefox has no `--lang` CLI
  flag (checked `firefox --help` output — no locale-override flag exists on this build).
- No `MOZ_LOCALE`-style environment variable is documented for the installed Firefox version
  (searched the running binary's `--help` and the shipped `application.ini`; none found).

Per iteration-142 and iteration-144's own explicit run guidance, landing a "fix" without
reproducing the symptom first is exactly what these plans forbid — a pref change made without
seeing the German output firsthand cannot be verified to do anything.

## What would unblock this

Any of:
- A Firefox build or profile with a genuine non-English langpack installed (e.g. `de` langpack
  via `about:preferences#general` → Language, or a Linux distro package that ships a localized
  build by default rather than `en-US`).
- A documented `MOZ_LOCALE`/similar environment-variable override for headless Firefox, if one
  exists in a Firefox version newer/older than what's checked here — re-check `firefox --help`
  and `about:support` on whatever build is used to implement this plan.
- Direct access to report a diagnostic profile from the dogfooding-session-63 environment (Firefox
  version, OS, langpack state) that reproduced the original German output, to compare against.

## Tasks

### A. Reproduce
- [x] Obtain or build a Firefox whose UI locale is verifiably non-English (`about:support` →
  "Application Basics" → confirm locale is not `en-US`).
- [x] Run `ff-rdp launch --headless` against it and trigger a console error (e.g.
  `ff-rdp eval 'undefinedFn()'`) — confirm whether the error text is English or localized despite
  the `intl.*` prefs.

### B. Diagnose (only after A succeeds)
- [x] If still localized: identify which additional pref or mechanism the existing `intl.*` pin
  misses (e.g. `general.useragent.locale` on older builds, or a required restart/profile-creation
  ordering issue — prefs might apply too late if Firefox already cached the locale from the OS at
  first run). Cite the Firefox source (searchfox) for whatever pref is found missing.
- [ ] If not localized: the iter-142 dogfooding-session-63 report predates the iter-61j fix, or was
  itself measuring something other than console text (e.g. a localized system dialog, not RDP
  eval output) — close this out as "confirmed already fixed" with the repro steps documented,
  rather than landing a no-op change.

### C. Fix (only if B finds a real gap)
- [x] Add whatever pref/mechanism B identified to `USER_JS` (or wherever it belongs), with a
  comment citing the Firefox source confirming its effect.

## Acceptance Criteria [1/1]

- [x] live_147_console_locale_pinned: console output is locale-stable on a genuinely
      non-English-locale Firefox — name the reproduction method used in the test. If no
      non-English Firefox can be obtained, re-defer again (a third time) with a note naming what
      was tried, rather than landing an unverifiable "fix" — do not tick this box without a
      passing named test.

## Design notes

Nothing to design until Theme A (reproduction) succeeds — this plan is intentionally
investigation-first, per CLAUDE.md's "reproduce before diagnosing" rule for exactly this class of
carried-over, environment-blocked item.

## Out of scope

Building or CI-provisioning a non-English Firefox specifically for this repro is out of scope
unless Theme A repeatedly fails for lack of one — if so, that provisioning work should itself be
scoped as a follow-up rather than folded into this investigation.

## References

- [[iteration-144-session-hygiene-followup]]
- [[iteration-142-session-hygiene]]
- [[decision-log]] — DEC-028

## Scope clarification — 2026-09-25

Qualify actual locale capability before reproduction: the owned runtime must
demonstrate a German application-locale baseline and usable English resources
for the product's English pin. A matching language pack or official localized
runtime is sufficient; no Firefox source build or new CI provisioning system
is required unless ordinary provisioning proves inadequate.

If current product launch yields English console/error text, conclude only that
current behavior is locale-stable under the recorded reproduction. Do not infer
that the historical German report predated the pin or measured another channel.
Its chronology and cause remain unknown. The original named-test acceptance
criterion is unchanged.

The historical Task B “If not localized” inference is unsupported and is
explicitly superseded by this clarification. Its text and checkbox remain
unchanged as the original record; a passing current test cannot satisfy that
chronological assertion. No speculative preference change is authorized.


## Verified implementation and acceptance — 2026-09-28

The original named acceptance test and local closing requirements have passed.
Implementation is complete on `iter-147/provision-language-pack-main-20260928`,
based on merged main `d2a17038945169d21ae9a9005cde967e3bab0769`.
Unmerged iteration259 code is not part of this implementation.
The earlier English-only environment description is historical: the qualified
runtime is now Firefox156.0.1/build20260921121718, default locale `de`, with
verified German and English resources.

The feature is explicit opt-in provisioning: `--english-language-pack <XPI>`
with `--restart-after-language-pack-install` stages a validated profile-local
English pack, starts one owned initializer, verifies its signed active provider,
waits for normal shutdown and persisted state, then starts the operational browser
once using the same profile. It is not a retry. Default `USER_JS` is unchanged;
profile activation uses `extensions.autoDisableScopes=14` only in this mode.
The initializer currently requires Unix process ownership support and a local
loopback host. Unsupported combinations fail before external effects.

Task A is supported by the retained qualified German application/engine baseline
and actual managed English-locale failures. Task B's real gap is that English
application locale/preferences alone do not guarantee the legacy English chrome
resource provider. Staging a pack alone also failed the actual engine test.
The implementation establishes and persists that provider before the operational
start. Source comments cite Firefox revision
`6f2c158dfc7e9693f880fad2510ceb51a158c069`: `HTMLMediaElement.cpp`1715–1725/3204,
`nsContentUtils.cpp`5705–5730/5903–5917, `nsChromeRegistryChrome.cpp`351–374,
`preferences/config/languages.mjs`158–178 and `XPIProvider.sys.mjs`1839–1883.
The first three explain legacy resource routing/fallback; the latter two support
apply/restart and normal-shutdown persistence. They do not prove first-bundle
ordering or historical causation. Task C is implemented in
`commands/english_language_pack.rs` and `commands/launch/language_initialization.rs`.

The private, independently reviewed acceptance harness ran the exact named
`live_147_console_locale_pinned` once on September28,16:54–16:55CEST:
**1 passed/0 failed**, runner actual wait0, outer actual exit0, no watchdog signal.
It composed the unchanged retained German baseline with this new English arm;
German was not rerun and no newly captured pair is claimed. The operational
metadata was requested `en-US`, application `[en-US, de]`, available `[de, en-US]`,
default `de`. One engine trigger produced the English media error
`<source> element has no “src” attribute. Media resource load failed.`
This was an engine-produced console/error message, not a fabricated eval result.

The successful launch recorded exactly two Firefox starts, one profile, one public
launch and one operational relaunch. The initializer took1588ms, obtained active
signed-provider readiness, persisted extension/cache files, actually waited for
child exit0 and actually joined its stderr pump. Its quit response was a classified
receive EOF, not an acknowledged reply; normal exit and ownership absence were
separately established. Persisted cache bytes are hashed, not semantically decoded.
Operational Firefox was detached: its actual child wait is unavailable; cleanup
proved the owned native identity/listener/descendant absence instead. Fixture join
was actual. Root verified481 pre-existing profiles conserved, zero new profiles,
unchanged protected/real state and no unexpected survivors. External process death
was not substituted for worker return or normal initialization.

Historical dogfooding-session63 chronology and cause remain **unknown**. Original
Task B's alternative inference stays unticked and unchanged, under the September25
clarification. A passing current test cannot establish that inference. Different
historical source graphs are retained with reviewed relevance; no claim is made
that all historical failures share this cause or that the new English result
retroactively qualifies the earlier failed pair.

## Validation and recoverable evidence

Private evidence root `I` is
`.git/ralph-loop/20260924-all-open/iter147/` in the primary repository. These paths
are recovery references; raw profiles, private evidence and binaries are not
included in the PR. All prior handoffs, archives and failed results remain intact.

- Current ordered validation: `I/without259-listener-validation1/execution/`;
  fmt → strict workspace Clippy → one workspace command passed. Reported2719
  passes comprise2718 parent results plus1 nested helper;0 failures/429 ignored.
  Twelve product controls ran within that workspace, including the three new
  listener-parser controls. Three separately compiled mutants each failed its
  one exact intended assertion with actual exit101: old p-only parsing,
  discard-unknown-fields and first-PID ownership. Normal source was restored and
  all590 product inputs verified. Four unchanged private semantic controls were
  reused from their actual earlier passes, not rerun or counted as new bodies.
- Current producer: `I/without259-listener-native1/producer-execution/`;
  fresh enabled private runner,13 actual commands exit0, zero test/native bodies.
  The normal CLI is the one frozen before mutants; shared-target mutant artifacts
  are not acceptance producers.
- Named result: `I/without259-listener-native1/execution/occurrence/result.json`,
  SHA256 `a4d47168b80546a1e897c6204839a9bbc6459fff574ccd997d5ddd3a791eeede`.
  Root release is that execution directory's `root-release.json`, SHA256
  `67799f52c230ba342a7b2a81a9c6a71a9ee4592d66394c4624cf038a37239dda`.

| Bound input | SHA256 |
|---|---|
| Normal CLI | `e65f981e2b86fb59e57b021995e0d38d6f75c179a26b7205dcf9045ed00613c9` |
| Enabled named runner | `d8afd2b8ea071f33181d457fe61c165f1236f8212a83557ab1a158ee5a7ff6ad` |
| Current590 product+17 private source manifest | `5dbe293a8ab96164bc0a399e478a6177f51dffd051fa85e998912443e2b17c08` |
| Exact110-file runtime manifest | `e46a8cb29102bab7a34130bee69eb942c0ff0b5c6c0cd8d54a5ae0e797b89fe6` |
| English XPI156.0.20260921.121718 | `6a1f941b8748d312799d4c4fd432c963cac03a17573f8fd125686643ce5743a3` |
| Retained German reference,11 files | `07cd411cf81e51517e700eb5ade4fd55d63a9c69865ea368746ed6ab202e6ba3` |
| Retained staged-pack failure reference,12 files | `0fb7ded8edc184a3c1d0998adcaf3b0f0a8c9819ee95259f030cb43b0bdd3680` |

Relevant independent review receipts under `I`, each `report.md`:

| Review | SHA256 |
|---|---|
| `review-without259-main1` | `cee523b2a61a8c10e31d98a8d270644d2d5b42ce0ec4e2057cbc3a6b11e36c79` |
| `review-clippy-repair1` | `2fd6b0af628b90e81362a96f4e638fe3160197f19f0a8b7b19de228e1389c5ea` |
| `review-selector-repair1` | `93e4ba6ffb2b30595c78c0223e0b4292fa0629fd800e11547454382bff111d70` |
| `review-without259-native1` | `bf3142ca265b81244a4aedce2a71dbafc36052375c0d1a7fa13ee3f6a53dc05b` |
| `review-listener-repair1` | `438470154b3f4a3ef3b877ef0187cb2b5d8a1b4f4b383510b506e80eeb046afb` |

Fresh native-proof review `review-listener-native-proof1` accepted the original
criterion (SHA256 `85680cdf5dd6d00e1dbd4ac91e98ead90f9c7f88bc91dbd5ccfa09fd0bd5aa4a`).
Closing-driver review `review-closing-driver1` accepted the executable workflow
(`01455db303d4bbbef348e656133b3987a2b48c3f6c2d7b54de749d063d933bb8`).
Final substantive documentation/evidence review `review-closing-docs1` returned
zero actionable findings
(`3bc907092bfffaa4a0d411be01e3741ce0131c01ddf3694eb555eb42114cabab`).
Earlier implementation/design reviews remain in their original private ledgers;
these receipts do not replace them.

## Carry-over

| Result or remaining item | Disposition and evidence |
|---|---|
| Original Task B alternative history claim | **No plan, with stated reason:** unsupported historical chronology is explicitly superseded by the September25 clarification and remains unticked. Reopen only with attributable session63 evidence; current English success is insufficient. |
| Retained German/managed-English pair and later English-app/German-engine failures | **Closed in this PR for the measured current behavior:** retain `runner-draft/native-pair1`, `runner-draft/named-native2` and `integrated-native-after1` failures and their distinct inputs; current named proof qualifies only the new opt-in mode. Historical pair verdicts remain failed. |
| Earlier provisioning Clippy failure | **Closed in this PR:** reviewed initializer/executor repairs; `provisioning-nonlive-preparation1` and `provisioning-clippy-repair1` evidence retained; current ordered gates pass. |
| Relative custom-build dep-info rejected | **No plan, with stated reason:** private producer qualification repaired in `without259-nonlive-continuation1`; original `without259-main1` failure retained. Reopen if actual artifact/stem/source binding fails again; no product change was implied. |
| Wrong top-level/nested test namespaces | **Closed in this PR:** corrected `iter_175_tests` selector and private schedule; failed continuation retained, current workspace executes the intended controls and nested helpers. |
| Native admission used old nine-file failure reference | **No plan, with stated reason:** private admission binding corrected to exact compiled twelve-file staged-pack reference; `without259-native-preparation1` failure and older reference remain intact. Reopen on reference drift; no guard was removed. |
| Initializer rejected valid lsof p/f framing | **Closed in this PR:** strict parser accepts documented decimal descriptors while rejecting unknown/malformed/multiple-owner output; actual-output controls, three mutants and current native pass. `without259-native-reference-repair1` failed initializer/profile remains retained. Exact rejected query bytes were unavailable, so that particular f-line cause is not claimed as captured fact. |
| Three mutant exit101 results | **No plan, with stated reason:** expected sensitivity failures, each compiled successfully and failed one intended assertion; all receipts preserved and normal source restored. Reopen if a mutant survives or fails for a different reason. |
| Unix/loopback opt-in scope; detached operational wait unavailable | **No plan, with stated reason:** explicit supported boundary and ownership evidence limit, not a cross-platform or all-locale guarantee. Unsupported initialization is rejected before effects. Reopen if extending this feature to another ownership model. |
| Closing attribution checker initially required attempt-ledger output for every fixture | **No plan, with stated reason:** one custom direct-launch helper is attributed through exact product PID/start/profile records, per-test markers and passing verdicts. The failed checker is preserved; six completed inventory enumerations were reused. Reopen on an unmatched profile or live survivor. |
| Static dogfood invocation lacked its required live flag | **No plan, with stated reason:** initial exit1 retained; corrected only that invocation and remaining gates within the existing bound. With the flag set, no-script SKIP is explicit and is not native evidence. Reopen if a declared script fails. |
| Required closing sweep/static gates/final review | **Closed in this PR:** actual results and limits below; publication is separately gated by exact-head CI and GitHub merge. |

## Closing validation — 2026-09-28

The final implementation's own sweep ran once under both
`FF_RDP_LIVE_TESTS=1` and `FF_RDP_LIVE_NETWORK_TESTS=1`, with the default six-job,
300s stall and900s build bounds. Actual sweep/driver/outer exits were0. All351
named verdicts passed, reconciled against the actual ignored-test inventories
across all six tiers: CLI340, frame-target1, registry3,61u3, core2 and watcher2.

```text
LIVE_SWEEP_SUMMARY executed=351 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=351
LIVE_SWEEP_PROFILES leaked=0 unattributed=0
```

Evidence is `I/without259-closing2/`: `sweep.log`, `tier-reconciliation.json`,
`launch-ledger-reconciliation.json`, `cleanup.json` and `root-release.json`.
All345 recorded launch attempts pair start/output records across17 ledgers.
The481 pre-existing profile states are conserved; ten additional retained test
fixtures have exact launch-output or product PID/start/profile attribution and
per-test markers. None is a live-owned leak. The owned raw browser was actually
waited after scoped TERM (status-15); its group is absent. Desktop/helper and
real user state are unchanged. A fresh post-interruption census confirmed491
profiles and no unexpected survivor. This does not invent waits for detached
fixture browsers or unrecorded workers.

Nine actual xtask check commands completed: iteration plan, source invariants,
Firefox references, actor/KB sync, live-test layout, dogfood script, skill drift,
help idioms and vendored JS. All288 plans validated with0 failures and93
warning-only plans; HYALO005 and diff checks passed. Limitations are explicit:
this plan has no firefox_refs key, so that gate checked no references; it has
no dogfood_script, so that gate reported SKIP after its required live flag was
set. The actual named locale proof and own dual-gate sweep supply live evidence.
The initial missing-flag invocation is retained; already passing gates were not
repeated. The final committed-head actor comparison is recorded with publication
checks under `I/publication1/`.

Local implementation, originalAC1/1, independent reviews and required closing
validation are complete. Exact PR-head CI and GitHub merge remain publication
gates: the supervisor must verify them and preserve their receipts before
claiming publication. No green CI or merge is asserted by this local record.
Original acceptance wording and historical task text remain unchanged above.
