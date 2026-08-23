---
title: "Iteration 194: two watch conditions carried out of iter-185's toolchain canary"
type: iteration
date: 2026-08-23
status: planned
branch: iter-194/toolchain-watch-carryover-conditions
depends_on:
  - iteration-185-main-red-under-clippy-1-98
first_call_sites: []
dogfood_path: |
  # No code exists yet for either condition below — this plan holds two
  # "if this happens, act on it" triggers from iter-185's carry-over sweep.
  # Exercising it means checking whether either trigger has fired since
  # .github/workflows/toolchain-watch.yml started running weekly.

  # Condition 1 — a merge-introduced lint-red `main` that survives a full
  # canary cycle. Compare the canary's weekly runs against merge history:
  gh run list --workflow=toolchain-watch.yml --limit=10
  git log --since="8 days ago" --oneline main
  # If two green PRs merged back-to-back produced a red `main` and the next
  # scheduled canary run did NOT catch it (still green, or caught it late),
  # that is condition 1 firing.

  # Condition 2 — a red canary going unnoticed for a week. Compare consecutive
  # canary run conclusions against whether anyone acted on a failure:
  gh run list --workflow=toolchain-watch.yml --limit=10 --json conclusion,createdAt
  # If a `failure` run is followed a week later by another `failure` run on
  # the same underlying cause (i.e. nobody fixed it in between), that is
  # condition 2 firing.
tags: [iteration, tooling, ci, watch-condition, carry-over]
---

# Iteration 194: still watching, no sweep yet

[[iteration-185-main-red-under-clippy-1-98]] added `.github/workflows/toolchain-watch.yml` (a
weekly lint canary against `main`) and, in its PR #220 carry-over sweep, identified two conditional
risks it deliberately did not build a mitigation for — accepting each as a bounded, reasoned cost
rather than inventing work for a scenario that has not happened. This plan is the fold-forward that
keeps those two conditions from being silently dropped, per the standing rule that carry-over items
need a named place to live, not just a paragraph in a PR body that nobody reads again.

Nothing here is new evidence. Both conditions and their reasoning are unchanged from PR #220's
carry-over table; what this plan adds is a place to record the first observation, positive or
negative.

## Carried-forward conditions

| # | Condition | Trigger | Why it was accepted rather than fixed in iter-185 |
|---|---|---|---|
| 1 | Nothing lints `main` immediately after a merge. Two individually-green PRs can merge into a lint-red `main` (a semantic merge conflict), and `ci.yml`'s `pull_request`-only trigger will not see it. | A merge-introduced red `main` survives a full weekly canary cycle without being caught, or the 7-day exposure window proves too wide in practice. | The weekly canary bounds exposure to at most 7 days — the same detection mechanism iter-185 already added, and a strict improvement over the 4-day *undetected* window that motivated it. Adding `push: [main]` would double CI cost per merge and would not even have caught the original incident (zero commits involved). Worth revisiting only if the bound is shown to be too wide, not preemptively. |
| 2 | The canary does not open an issue or otherwise alert on failure — it relies on GitHub's default scheduled-workflow-failure notification reaching the one maintainer. | A red canary run goes unnoticed (unfixed) for a week — i.e. two consecutive scheduled runs fail on the same underlying cause with no intervening fix. | An alerting mechanism is a second thing to build and to be wrong about, for a notification channel that already exists. Not worth the maintenance cost until the default channel is shown to fail in practice. |

## Out of scope

- Designing a fix for either condition ahead of its trigger. Both currently lack the evidence a fix
  would need to be well-targeted — that is why they are watch conditions and not a plan with tasks.
- Re-deriving iter-185's decision not to pin `rust-toolchain.toml` or add `push: [main]` — both are
  settled in [[decision-log]] DEC-044 and are not reopened here.

## Acceptance Criteria [0/2]

- [ ] Watch condition 1 (merge-introduced red `main` survives a full canary cycle) has either fired
      (and been forked into its own plan) or has not fired since this plan was filed — checked
      against `gh run list --workflow=toolchain-watch.yml` history
- [ ] Watch condition 2 (a red canary unnoticed for a week) has either fired (and been forked into
      its own plan) or has not fired since this plan was filed — checked against consecutive
      canary run conclusions

As with [[iteration-192-live-sweep-watch-conditions-carried-forward]]'s seven conditions: neither
box here can be ticked by inspection. Each needs either an observed trigger or a deliberate,
written decision that the condition no longer applies (e.g. the canary itself was replaced). A tick
records that somebody looked — never that the condition was resolved.

## References

- [[iteration-185-main-red-under-clippy-1-98]] — origin of both conditions, PR #220's carry-over
  sweep
- [[decision-log]] — DEC-044, the reasoning both conditions were accepted against
- `.github/workflows/toolchain-watch.yml` — the canary these conditions watch
- [[iteration-192-live-sweep-watch-conditions-carried-forward]] — the precedent for how this repo
  folds open watch conditions forward instead of dropping them when a source plan closes
