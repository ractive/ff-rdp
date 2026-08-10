---
branch: iter-138/navigation-truthfulness-2
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-130-navigation-truthfulness.md
dogfood_path: |
  ff-rdp launch --headless --port 6100
  ff-rdp navigate https://www.bbc.com/this-page-does-not-exist-xyz --port 6100
  # → must report the 404 status, not a bare success envelope
  ff-rdp navigate https://en.wikipedia.org/wiki/Firefox --port 6100
  ff-rdp eval "history.pushState({},'','/wiki/Firefox?route=1')" --port 6100
  time ff-rdp back --port 6100
  # → must succeed promptly with the real URL, not block 10s and exit 124
  ff-rdp navigate 'https://www.gov.uk/#frag' --timeout 8000 --port 6100
  # → must succeed; fragment navigation is not a failure
first_call_sites: []
status: completed
---

# Iteration 138: navigation truthfulness II — HTTP status, SPA history, honest timeouts

Follow-up to [[iteration-130-navigation-truthfulness]], from [[dogfooding-session-63]].
Theme B is a **severity regression introduced by iter-130** and should be fixed first.

## Themes

### Theme A — `navigate` never reports HTTP status

The highest-value single fix in the session-63 backlog.

```
$ ff-rdp navigate https://www.bbc.com/this-page-does-not-exist-xyz
{"results":{"navigated":"...","committed_url":"...","ready_state":"complete","elapsed_ms":326}}
$ ff-rdp eval 'document.title'
"BBC - 404: Not Found"
```

404 and 503 pages return a normal success envelope. `network` cannot fill the gap — `status`
is null on performance-api rows (`"note":"method/status not available from performance-api
source"`). **There is currently no way in ff-rdp to learn the main document's HTTP status.**

Add `status` to navigate's results. If the status is genuinely unavailable for a given
navigation (cache hit, same-document), report `null` explicitly rather than omitting it —
consistent with iter-128's always-present-nullable-key convention.

### Theme B — same-document history traversal hard-fails (REGRESSION)

```
$ ff-rdp eval "history.pushState({},'','/wiki/Firefox?route=1')"
$ time ff-rdp back
{"error":"navigate: document.readyState did not reach 'complete' (with fresh navigation)
 within 2924ms — use --no-wait to skip or increase --timeout","error_type":"Timeout"}
10.109 total     # exit=124
$ ff-rdp eval "location.href"
"https://en.wikipedia.org/wiki/Firefox"     # the traversal SUCCEEDED
```

Reproduced 4/4 across two sites. iter-130 gave `back`/`forward`/`reload` navigate's envelope
by sharing its readiness wait — but a same-document `popstate` produces no fresh navigation
and no `readyState` transition, so the wait can never be satisfied. Before iter-130 this
returned a useless-but-successful `{"action":"back"}`; now a correct operation reports failure
with a non-zero exit. For an SPA-driving agent that is strictly worse.

Detect same-document traversal and complete on `popstate` (or equivalent) instead of waiting
for a document commit that will never come.

### Theme C — same-page fragment navigation falsely reports failure

`ff-rdp navigate 'https://www.gov.uk/#frag' --timeout 8000` burns the full timeout and returns
a Timeout error; `location.href` confirms the navigation succeeded. Same root cause family as
Theme B — a fragment change is a same-document navigation.

### Theme D — timeout messages report the wrong budget

`--timeout 8000` → 8.10 s wall, message says "within 2384ms". `--timeout 20000` → 20.19 s wall,
says 5907 ms. Consistently ~3× under. An agent sizing a retry from this message gets it wrong.
Report the real elapsed time, or name the sub-budget explicitly as a sub-budget.

### Theme E — `--no-wait` is advertised where it doesn't exist

The Theme B/C error text recommends `--no-wait`, but `back`/`forward`/`reload` don't accept it
(`error: unexpected argument '--no-wait' found`). Either add the flag to those commands or stop
recommending it. There is currently **no escape hatch at all** for the readiness wait on history
commands.

### Theme F — `back`/`forward` report a subframe URL as `committed_url`

```
$ ff-rdp eval 'location.pathname' → "/technology"
$ ff-rdp back --jq '.results.committed_url'
"https://a4621041136.cdn.optimizely.com/client_storage/a4621041136.html"
$ ff-rdp eval 'location.pathname' → "/news"     # the real result
```

Occurs in both connection modes. The traversal is correct; the reported URL comes from a
subframe context. `navigate` gets this right — reuse whatever it does.

### Theme G — `navigate --with-network` drops `committed_url` and `ready_state`

Both null with the flag, populated without it. Truthful navigation *or* network data, never
both. Note the irony recorded in session-63: the network entries were the only place a status
appeared (`303 → /verify_human`), revealing that the plain call's `committed_url` was itself
untruthful — which Theme A fixes.

## Acceptance Criteria [8/8]

- [x] live_138_navigate_reports_404: `navigate` to a known 404 reports status 404
- [x] live_138_navigate_reports_200: status 200 on a normal page (no false positives)
- [x] live_138_pushstate_back_succeeds: `back` across a pushState entry returns the real URL
      promptly with exit 0 — assert wall-clock well under the timeout
- [x] live_138_fragment_navigate_succeeds: `navigate` to `#frag` succeeds
- [x] live_138_timeout_message_matches_wall_clock: reported budget within tolerance of observed
      elapsed time
- [x] live_138_back_forward_committed_url_is_top_frame: `committed_url` matches
      `eval location.href` after traversal on a page with cross-origin subframes
- [x] live_138_with_network_keeps_envelope: `navigate --with-network` returns non-null
      `committed_url` and `ready_state` alongside network data
- [x] `e2e_no_wait_flag_consistency` (Theme E): the no-wait flag exists wherever it is
      recommended, or is not recommended where it does not exist

## Notes

- Theme B is a regression from a merged iteration — fix it first and state that plainly in the
  PR description.
- Themes B, C, F are one family (same-document / frame-context awareness); a shared fix is
  likely better than three special cases.

**Implementation findings (2026-08-10), following Run guidance rule 1 — mock tests did not catch
either of these; only live Firefox through the default daemon path did:**

- **The Theme A/G `status` field required daemon-stream plumbing, not just a subscribe.** The
  daemon manages `network-event` watching centrally and does not forward it to a client that
  only issues the generic `watchResources` RPC `navigate`'s `document-event` wait already used —
  `status` stayed `null` through the daemon no matter how long the wait was extended, while the
  identical navigation reported it correctly under `--no-daemon`. Fixed by wrapping the
  `document-event` wait with `start_daemon_stream`/`stop_daemon_stream("network-event")` (the
  same daemon API `navigate --with-network` already used) whenever `ctx.via_daemon`. Also found
  live: Firefox's netmonitor pipeline can lag a few ms behind the docshell's own `dom-complete`,
  so the wait needed a short (300 ms) bounded grace window after commit before finalizing
  `status`, and `resources-updated-array` entries are incremental — a later update without
  `status` must not overwrite an earlier one that had it (`extract_document_status` originally
  took "the most recent update record" instead of "the most recent value seen per field" and
  silently regressed a real 200 back to `null`).
- **Theme F's fix (`trust_event_url: false`) cannot eagerly refresh the console actor at
  `dom-loading`.** Doing so reintroduces the exact blocking-`getTarget`-swallows-an-in-flight-
  `dom-complete` race `poll_enabled: false` already existed to avoid for `back`/`forward`/
  `reload` — confirmed via a mock e2e regression, not live Firefox. The fix resolves lazily
  instead: eval against whatever actor is already cached at `dom-complete`, and only pay for a
  fresh `getTarget` if that first eval comes back empty (stale actor). The new unconditional
  same-document check (Themes B/C) has the identical hazard for a different reason — its own
  blocking eval can swallow an already-buffered `dom-complete` — and is guarded with a temporary
  `swap_event_sink`/replay around the call (`probe_same_document_commit_safe`), the same pattern
  `enumerate_frame_targets` (iter-129) uses.

## Run guidance (batch 138–142, from dogfooding session 63)

Non-negotiable working rules for whoever implements this plan:

1. **Do not trust the root cause stated above.** In iterations 135, 136 and 137 the real
   cause differed from the plan's hypothesis three times running, and twice it was our bug,
   not Firefox's. Reproduce the symptom and verify the mechanism **on the wire** (actual RDP
   packets / actual command output) before writing the fix. If the diagnosis here turns out
   to be wrong, fix the real cause and correct this section.
2. **A live test that passes `--no-daemon` proves nothing about the default path.** That is
   exactly how iter-129 shipped a feature that did not work at all. Every live test added
   here must exercise the default (daemon) path. iter-137 added the guard at
   `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs` with a shrink-only grandfather
   list — **do not add entries to that list.**
3. Evidence for every finding in this plan — exact command and exact output — is in
   [[dogfooding-session-63]]. Read it before diagnosing.
