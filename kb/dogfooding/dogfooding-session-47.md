---
title: "Dogfooding Session 47 — iter-61g verification on Hacker News"
type: dogfooding
date: 2026-05-17
status: completed
site: https://news.ycombinator.com
commands_tested: [tabs, navigate, page-text, sources, network, snapshot, screenshot, perf, a11y, dom, computed, eval]
tags:
  - dogfooding
  - iter-61g
  - navigate-blocking
  - network-scoping
  - sources-fallback
  - regression-verification
---

# Dogfooding Session 47

Verification run for [[iteration-61g-session-48-deferred]] (PR #72,
commit `edda8e2`) against Hacker News. Headline: **navigate-blocking
works and is a clear win, network scoping holds in the happy path,
the sources walker fallback "appears to work" on HN but does so by a
different path than advertised (and the underlying CSP-eval probe
claim does not match observed behaviour).** Three new bugs surfaced
on the side, including one that breaks the navigate-by-default
contract for re-navigating to the same URL, and one that contradicts
iter-61f's "computed --prop is repeatable" commit message.

## What's New Since Last Session

- **iter-61e** (`fc589ca`): `deviceActor` version parser fix +
  screenshot error message tweak. Did not move the needle on the
  underlying headless-screenshot issue (still broken on this
  Firefox build — see #5).
- **iter-61f** (`98b2131`): four session-48 ergonomic fixes — click
  selector-aware errors, `computed --prop` repeatable, `snapshot`
  default depth 6→10, dogfood-skill prompt drift. The `computed
  --prop` repeat claim does **not** match shipped code (see #4).
- **iter-61g** (`edda8e2`): the three deferred session-48 items.
  This session is the verification pass.

## Regression Checks — iter-61g session-48 items

| # | Item | Pre-iter-61g | Current | Verdict |
|---|------|--------------|---------|---------|
| 1 | `navigate` waits for commit, returns `committed_url`/`ready_state`/`elapsed_ms` | fire-and-forget; agents read stale page-text | Blocking works; payload carries `committed_url: "https://news.ycombinator.com/"`, `ready_state: "complete"`, `elapsed_ms: 789`. `navigate→page-text` chain reflects HN immediately, no sleep. | ✅ **Works** |
| 1b | `--no-wait` preserves old shape | n/a (was default) | `{"navigated": "..."}` only — matches pre-iter-61g. | ✅ Works |
| 1c | `--wait-for selector:.athing` | n/a | Returns `{wait_for: {predicates, waited: true, elapsed_ms: 1}}`. Selector-aware. | ✅ Works |
| 1d | Re-navigate to same URL | worked (fire-and-forget) | `navigate https://news.ycombinator.com` twice in a row — second call times out at 5000 ms with `page did not commit`. **Regression introduced by iter-61g.** | ❌ **Broken** (see #1) |
| 1e | `--wait-for` / `--wait-text` timeout error message | n/a | Generic `operation timed out — try increasing --timeout`. The plan AC promised "navigated to X but `<selector>` did not appear within Yms". | ⚠️ **Partial** (see #2) |
| 2 | `network` scoped to current navigation | mixed entries from prior pages | After `navigate B`, `network` shows only B's requests; `meta.since: {index: -1, url, sequence}` carries the boundary. Switching example.com → HN: HN's network output contains zero example.com entries. | ✅ **Works** |
| 2b | `--since all` cumulative | n/a | Flag present per `--help`. Shape question raised below (see #6). | ⚠️ Partial |
| 7 | `sources` fallback on CSP-eval-blocked pages | returned 1 source on HN | Returns 1 source on HN today — but HN's current HTML actually has only 1 `<script>` tag (external `hn.js`, no inline). So the count is correct but **not because the walker fallback fired** — `meta.fallback_method: "js-eval"`, which the plan said should be skipped under CSP. See #3. | ⚠️ **Partial** (works coincidentally) |

## Smoke Test Results

| Command | Status | Notes |
|---------|--------|-------|
| `tabs` | ✅ | 2 tabs; HN + Consent-O-Matic options. |
| `navigate https://news.ycombinator.com` | ✅ | 789 ms first time, 312 ms with `--wait-for`. |
| `navigate <same URL again>` | ❌ | Times out at 5000 ms. See #1. |
| `page-text` | ✅ | Reflects current page (HN content present). |
| `perf vitals` | ✅ | All four core vitals returned, all "good"; LCP marked approximate. |
| `a11y contrast --fail-only` | ✅ | 193 contrast violations on HN. |
| `dom stats` | ✅ | `inline_script_count: 0` confirms the "no inline scripts on HN" finding. |
| `snapshot --max-chars 1000` | ✅ | `meta.depth: 6` reported. (See gripe in #7 — depth knob name.) |
| `screenshot -o /tmp/dogfood47.png` | ❌ | "screenshot actor unavailable on Firefox unknown; minimum supported version: 120." Same deferred issue as session 46 (#3). |
| `computed "a" --prop color --prop font-size` | ❌ | "cannot be used multiple times". Iter-61f's commit message says this is fixed. See #4. |
| `sources` (HN) | ⚠️ | 1 result, but `fallback_method: "js-eval"` despite CSP-blocked eval; see #3. |
| `sources` (github.com) | ✅ | 20+ results, `fallback_method: "js-eval"`, all external `.js`. |
| `network` (after navigate) | ✅ | Scoped; `meta.since` carries boundary. |
| `eval 'document.querySelectorAll("script").length'` | ❌ | `EvalError: call to eval() blocked by CSP` (expected on HN, but see #3 — `sources` fallback uses the same path). |

## Findings

### What Works Well

- **`navigate` blocking is a genuine UX upgrade.** The chain
  `navigate URL && page-text` finally works without a sleep, and
  the new `committed_url` / `ready_state` / `elapsed_ms` payload is
  exactly what an agent needs to confirm landing. The `--wait-for
  selector:.athing` form is one call instead of `navigate` + `wait
  --selector`.
- **`--no-wait` cleanly preserves the old shape** for callers that
  scripted against the fire-and-forget contract — payload is
  literally just `{"navigated": "..."}` again.
- **Network boundary scoping holds in the happy path.** No more
  cross-page leakage in `network` summary mode; `meta.since` even
  exposes the sequence number so an agent can correlate to follow
  events.
- **`navigate --help` is now excellent** — the new behaviour,
  flags, examples, and full output shape are all in the long help.
  This is the gold standard the other commands should match.

### Issues Found

#### 1. Re-navigating to the current URL times out under the new blocking default — **major**

```text
$ ff-rdp navigate https://news.ycombinator.com
{ "results": { "committed_url": "https://news.ycombinator.com/", "ready_state": "complete", "elapsed_ms": 632 }, "total": 1 }

$ ff-rdp navigate https://news.ycombinator.com
error: navigate: page did not commit within 5000ms — use --no-wait to skip commit check or increase --timeout
(exit 124)
```

Also reproduced going example.com → example.com after explicit
`tabs` showed `Example Domain`. Firefox does perform the
re-navigation (you can see the page reload in non-headless mode) but
the URL doesn't *change*, so the commit-detector waits for an event
that never comes.

Suggested fix: the commit poll should also accept "URL is already
the requested URL AND `readyState` is `complete` AND a new top-level
docShell load was observed since dispatch" — i.e. correlate against
`docShell.loadGroup` activity, not just URL transitions. As a
weaker fallback, treat "URL matches and readyState went from
`loading` back to `complete`" as a commit.

Workarounds today: `--no-wait` (loses the elapsed_ms / ready_state
data), or `reload` (different semantics from re-navigating).

#### 2. `--wait-for` / `--wait-text` timeout error is generic, not selector-aware — **minor**

```text
$ ff-rdp navigate https://example.com --wait-text "nonexistent text 123" --timeout 3000
error: operation timed out — try increasing --timeout

$ ff-rdp navigate https://example.com --wait-for "selector:.nonexistent" --timeout 3000
error: operation timed out — try increasing --timeout
```

The iter-61g plan AC A2 promised:
> "navigated to X but `.athing` did not appear within Yms"

What ships is the generic `operation timed out` — the same
transport-level fallback that iter-61f explicitly fixed for
`click` / `type` / `scroll`. The selector-aware wrapping that
iter-61f added to interactive verbs needs to be applied to
`navigate --wait-for` and `navigate --wait-text` too.

#### 3. `sources` fallback path-of-record disagrees with the iter-61g design — **moderate**

Plan task C2 explicitly says:

> Before invoking the `js-eval` fallback, probe whether the page
> CSP allows `eval` ... If eval is blocked, skip directly to the
> WalkerActor fallback rather than emitting a CSP exception that
> contaminates the result.

But on HN (CSP-eval-blocked, confirmed by `ff-rdp eval` returning
`EvalError: call to eval() blocked by CSP`):

```text
$ ff-rdp sources
{ "meta": { "fallback": true, "fallback_method": "js-eval" }, ... }
```

`fallback_method` is `js-eval`, **never** `walker-actor` in this
session. Two hypotheses:

a. The js-eval path runs in the debugger context which **is**
   exempt from page CSP. In that case the eval fallback "works"
   on HN even though page-side `eval` is blocked, and the CSP
   probe / walker-actor code path is dead. The plan's premise was
   wrong.
b. The js-eval path is genuinely returning what page-side
   `document.scripts` says, and HN's current HTML has only the
   single `hn.js` script tag (no inline `<script>` blocks).
   `dom stats` confirms `inline_script_count: 0`.

Either way, **the iter-61g plan was based on a stale snapshot of
HN** (the doc says "4+ inline `<script>` blocks"; today's HN has
zero). The shipped code does not exercise the walker-actor path on
any site I tested. C3 says there's a unit test fixture with three
script tags + eval-blocking CSP — that's the only place the new
code path is actually proven, and it isn't exercised by any real
site I could probe.

Recommendation: either find a real site where the walker-actor
fallback fires and add it to the regression matrix, or be honest
in the help text that the fallback is theoretical until proven.
Also: surface in `meta` *which* CSP probe outcome led to which
fallback choice, so the next dogfooder can see the decision tree.

#### 4. `computed --prop` is not repeatable despite iter-61f commit message — **moderate**

```text
$ ff-rdp computed "a" --prop color --prop font-size
error: the argument '--prop <NAME>' cannot be used multiple times
```

iter-61f commit `98b2131` says:
> "computed --prop is now repeatable. ... Post-fix: repeated
> --prop returns a computed: {name: value, ...} object filtered to
> just the requested names."

But `crates/ff-rdp-cli/src/cli/args.rs:954`:

```rust
prop: Option<String>,
```text

— no `ArgAction::Append`, no `Vec<String>`. The flag is still
single-valued. Either the commit message is wrong about what
landed, or the args.rs change was reverted somewhere. Worth a
focused unit test:

```rust
#[test]
fn computed_prop_is_repeatable() {
    let cli = Cli::try_parse_from(["ff-rdp","computed","h1","--prop","color","--prop","font-size"]);
    assert!(cli.is_ok(), "{:?}", cli);
}
```

Comma-separated `--prop "color,font-size"` parses without an error
but returns `"results": ""` (empty string) — silently wrong.

#### 5. Headless `screenshot` still broken — **major (deferred from sessions 44–46)**

Same as session 46 issue #3. Identical error message, identical
underlying cause (Firefox version not advertised in the RDP
greeting). Not an iter-61g regression — just unchanged.

#### 6. `network --since all` output shape varies — **minor / suspect**

Initial run:
```text
$ ff-rdp network --since all --jq '.results.total_requests'
# error: cannot index [array] with "total_requests"
```

`--since all` returned `.results` as an **array of detail entries**
instead of the summary `{by_cause_type, slowest, total_requests}`
object. Default (no flag) returned the summary object. But on a
later run, both shapes were arrays. Probably depends on
`--detail` defaulting, but the inconsistency is jarring — an
agent that scripts `.results.total_requests` against default will
break the moment `--since all` is added.

Either: (a) `--since all` should preserve summary shape, or (b)
docs should explicitly warn it switches mode. Easy fix.

#### 7. Snapshot default-depth knob isn't visible / configurable — **minor**

`snapshot --max-depth N` is rejected with `unexpected argument
--max-depth ... tip: a similar argument exists: '--max-chars'`.
Per iter-61f the default depth went 6→10, but there's no flag to
override it. Long help also doesn't mention the depth value. If
depth is going to be a tuning knob it deserves a flag; if not, at
least document the constant.

### Feature Gaps

- A `--reload` semantic on `navigate` (or first-class `reload`) that
  consistently re-fires the commit detector — would close finding
  #1 by giving an explicit path for "I really do mean go-to-current".
- `sources --no-fallback` to force the SourceActor path and report
  what the protocol actually returned, separately from any fallback
  layer. Today there's no way to tell whether the SourceActor
  returned 1 source or zero sources before the fallback ran.
- For `navigate --wait-for`, accept a list of predicates (ANY-of)
  not just one. Many pages have several "I am ready" signals; right
  now you have to pick one.

## Summary

- **13 commands exercised; 9 pass, 4 fail.**
- **iter-61g verdict**:
  - Item #1 (navigate blocking): **works** with one new regression
    (re-navigate to same URL hangs — finding #1) and one missed AC
    (selector-aware timeout message — finding #2).
  - Item #2 (network scoping): **works**; one shape inconsistency
    around `--since all` (finding #6).
  - Item #7 (sources fallback): **partial** — the count happens to
    be correct on HN today (1 source, 0 inline) but the
    walker-actor fallback path is never observed firing on real
    pages, and the plan's premise about HN inline scripts no
    longer matches reality (finding #3).
- **Key takeaway**: navigate-blocking is the kind of fix that pays
  for itself on every subsequent session — but the same-URL
  regression undermines its "safe default" framing. A 1-day
  follow-up (61h?) tackling #1 + #2 + #4 would close the loop on
  what iter-61g promised. The `sources` fallback story needs a
  real-world verification site, not just a unit-test fixture, to
  trust the design claim.

## References

- [[dogfooding-session-46]] — previous session (verified iter-61c).
- [[iteration-61g-session-48-deferred]] — the fix bundle this
  session verifies. Commit `edda8e2` (PR #72).
- [[iteration-61f-session-48-ergonomics]] — the ergonomic fixes
  that closed session-48 #3/#4/#5/#6. **Note**: this file is
  referenced in iter-61g's `depends_on` but the actual KB file
  could not be located in `kb/iterations/`; the commit message
  on `98b2131` is the canonical source for what was claimed.
- Commits: `edda8e2` (61g merge), `1c7b2fb` (61g impl), `98b2131`
  (61f), `fc589ca` (61e).
