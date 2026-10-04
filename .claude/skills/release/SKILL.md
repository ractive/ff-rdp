---
name: release
description: "Cut an ff-rdp release: preflight (green main, green nightly, nothing open), version bump PR if needed, release notes generated from merged PRs since the last tag, an explicit owner confirmation, then `gh release create` and watching the shared pipeline to the end. Use when the user says /release, 'cut a release', 'release vX.Y.Z', 'ship a new version', or asks whether we can release."
---

# Release

One release = one tag `vX.Y.Z` on `main`. Publishing the GitHub release is the trigger for the
shared pipeline `ractive/release-workflows` (pinned in `.github/workflows/release.yml`); fixes
to the pipeline itself belong in that repo. The procedure below has **one hard stop**: the owner
confirms the notes and the version before anything is published. Everything before that stop is
read-only or lands through an ordinary PR.

## 1. Preflight (read-only; stop at the first failure and report it)

```bash
git fetch origin && git status --porcelain            # clean tree, on main
git log --oneline origin/main -1
gh pr list --state open --json number,title           # nothing open
gh run list --limit 3 --json name,conclusion,headSha  # last CI runs on main green
gh run list --workflow=live.yml --limit 1 --json conclusion,createdAt,url   # last nightly green
git describe --tags --abbrev=0                        # last tag, e.g. v0.3.0
grep -m1 '^version' Cargo.toml                        # workspace version
```

Decide the version with the owner's rule: pre-1.0, **removed or renamed flags bump the minor**;
fixes only bump the patch. Check for removals: `gh pr list --state merged --search
"merged:>$(git log -1 --format=%cs $(git describe --tags --abbrev=0))" --json title --jq '.[].title'`
and look for `!:`, "remove", "rename", "delete".

## 2. Version bump (only if `Cargo.toml` is behind)

Bump the workspace `version` in `Cargo.toml`, the `ff-rdp-core` requirement in
`crates/ff-rdp-cli/Cargo.toml` (`^0.3` does not accept `0.4.0`), `Cargo.lock` via
`cargo build -p ff-rdp-cli`, and any version string in `README.md`. Confirm
`./target/debug/ff-rdp --version`. Land it as a normal PR (`gh pr create --fill && gh pr merge
--merge --delete-branch --auto`); the pipeline's version-check compares the tag against
`ff-rdp-cli`'s version and refuses a mismatch.

## 3. Release notes (generated, then edited)

```bash
LAST=$(git describe --tags --abbrev=0)
SINCE=$(git log -1 --format=%cs "$LAST")
gh pr list --state merged --limit 300 --search "merged:>$SINCE" \
  --json number,title,mergedAt --jq 'sort_by(.mergedAt) | .[] | "- \(.title) (#\(.number))"'
```

Write `notes.md` in the scratchpad with these sections, in this order, dropping empty ones:
**Breaking / Removed** (removed commands and flags, renamed flags, changed output shapes — each
with the replacement), **Added**, **Changed**, **Fixed**, **Internal** (one line: tests, CI,
process). Lead with the one or two sentences a user needs ("the daemon is gone; capture network
with `--with-network`, `--follow`, or `--source performance-api`"). Group the PR list, do not
paste it. Link `kb/` notes that explain big changes.

Then print the notes and the intended tag in the chat and **stop**:

> Release `vX.Y.Z` from `main` at `<sha>` with the notes above — go?

Do not continue on anything but an explicit yes. "Looks good" is a yes; silence is not.

## 4. Publish (after the owner's yes)

```bash
gh release create vX.Y.Z --target main --title vX.Y.Z --notes-file notes.md
gh run list --workflow=release.yml --limit 1 --json databaseId,url
gh run watch <id> --exit-status
```

If a job fails: read its log (`gh run view <id> --log-failed`) before anything else. Known
quirks: the `aur` job is `continue-on-error` — read its log even when the run is green;
cross-compiled targets skip tests by design; Windows test hangs have happened on the runner.
Never delete and re-create a tag to retry — fix the cause (usually in `release-workflows`),
then re-run the failed jobs (`gh run rerun <id> --failed`).

## 5. After the pipeline is green

```bash
cargo install --path crates/ff-rdp-cli --locked && ff-rdp --version
brew update && brew info ff-rdp | head -3        # tap picked up the version (may lag minutes)
```

Run a two-minute smoke with the installed binary (`launch --headless`, `navigate`, `page-text`,
`screenshot`, `network --follow` for one navigation; kill the Firefox by pid). Add one line to
the `## Log` of `kb/research/reset-2026-10-execution.md` (or the current tracker) with the tag,
the release URL and the date, via a docs PR. Report: tag, release URL, pipeline run URL, which
distribution channels succeeded, installed version.

## Ground rules

- Read-only until step 4; step 4 only after the explicit yes.
- Do not edit `.github/workflows/release.yml` inputs to make a run pass; that is a change to
  discuss, not a retry.
- Secrets are repository secrets (`CARGO_TOKEN`, `HOMEBREW_TAP_TOKEN`, `SCOOP_BUCKET_TOKEN`,
  `WINGET_TOKEN`, `CLOUDSMITH_API_KEY`, `AUR_SSH_PRIVATE_KEY`); never print or copy them.
- `CONTRIBUTING.md` "Releasing" is the short form of this skill; keep the two in step.
