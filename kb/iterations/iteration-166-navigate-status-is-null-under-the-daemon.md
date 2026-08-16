---
branch: iter-166/navigate-status-null
date: 2026-08-16
depends_on: []
dogfood_path: |
  # `navigate` promises the main document's HTTP status (iter-138 Theme A).
  # Measure whether it delivers one.
  ff-rdp launch --headless --debug-port 7401
  ff-rdp --port 7401 navigate https://example.com \
      --jq '.results | {committed_url, ready_state, status}'
  # → on main (2026-08-16, fresh daemon, Firefox 153):
  #   {"committed_url":"https://example.com/","ready_state":"complete","status":null}
  #   `status` must be 200. It is null on the FIRST navigate of a fresh daemon,
  #   so this is not a leftover-state effect.

  # The data is demonstrably reachable over the same connection:
  ff-rdp --port 7401 navigate https://example.com --with-network --jq '.results.network | length'
  # → 11 entries, including the main document. So the network-event resources
  #   DO arrive; plain `navigate` is failing to correlate one of them with the
  #   document it just committed.

  # Same question without the daemon — unmeasured as of filing, settle it first:
  ff-rdp --port 7401 --no-daemon navigate https://example.com \
      --jq '.results | {committed_url, status}'
  # → if direct mode reports 200, the defect is in the daemon's network-event
  #   delivery to a plain (non---with-network) navigate; if it is null there
  #   too, the defect is in `extract_document_status`'s matching.
status: planned
title: "Iteration 166: navigate reports status: null for a document it successfully loaded"
type: iteration
tags:
  - iteration
  - navigate
  - daemon
---

# Iteration 166: `navigate` reports `status: null` for a document it successfully loaded

Carry-over from [[iteration-164-two-failures-the-158-sweep-uncovered]], filed before that PR
merges per CLAUDE.md's carry-over rule. Found while verifying iter-164's daemon fix did not
regress `navigate`; **it is not caused by that iteration** — the same `status: null` appears on
the *first* navigate of a fresh daemon, which is a code path iter-164's change cannot reach (no
`unwatchResources` frame has been sent yet at that point).

## The defect

`navigate https://example.com` returns:

```json
{"committed_url": "https://example.com/", "ready_state": "complete", "status": null}
```

`ready_state: "complete"` and a correct `committed_url` prove the navigation succeeded, so
`status: null` is not "the page failed" — it is "we did not find out". iter-138 Theme A added a
`network-event` subscription to `wait_for_doc_complete` specifically so the main document's HTTP
status could be reported, and `extract_document_status` (`commands/navigate.rs`) exists to pull
it out. One of the two is not doing its job.

This is the [[iteration-160-envelope-honesty]] class of problem in its milder form: the field is
`null` rather than wrong, so nothing lies outright. But a caller scripting `navigate` cannot
distinguish "the server returned no status" from "the CLI did not observe one", and `null` is
the value it would also see for a genuine failure.

## Why it was not caught

No live test asserts `results.status` on a plain `navigate`. `live_138_navigation_truthfulness_2`
and `live_130_navigation_truthfulness` assert `committed_url` and `ready_state`; the status field
is only exercised through `--with-network`, which takes an entirely different code path (it
drains the daemon buffer rather than correlating a streamed event).

## Themes

### Theme A — establish where it breaks, daemon vs direct

Run the `dogfood_path` above, both routes, before touching anything. Three candidate causes, and
the measurement distinguishes them:

1. the daemon does not deliver `network-event` frames to a plain `navigate` (it manages
   `network-event` centrally and only streams on request — `commands/navigate.rs` calls
   `start_daemon_stream` for exactly this reason, so verify the stream is actually producing);
2. the events arrive but `extract_document_status` fails to match the main document (URL
   normalisation, redirect, or the event arriving after the wait returned);
3. the status only lands on the `network-event` *update* packet, which the wait may not consume.

Record which one it is in this plan before writing the fix.

#### Measured 2026-08-16, on `main` at 07a9c03, before any code change

Firefox 153 headless, fresh daemon on port 7401 (`ff-rdp launch --headless --debug-port 7401`):

```
$ ff-rdp --port 7401 navigate https://example.com --jq '.results | {committed_url, ready_state, status}'
{"committed_url":"https://example.com/","ready_state":"complete","status":null}

$ ff-rdp --port 7401 --no-daemon navigate https://example.com --jq '.results | {committed_url, ready_state, status}'
{"committed_url":"https://example.com/","ready_state":"complete","status":null}

$ ff-rdp --port 7401 navigate https://example.com --with-network --jq '.results | {status, n: (.network.entries|length)}'
{"status":null,"n":2}
```

So it is **not** a daemon defect: direct mode is null too, and so is `--with-network`, which
reaches the status through the entirely separate `extract_document_status` path. Three code
paths, one shared symptom — which rules out candidate 1 (the daemon stream *is* producing; the
`--with-network` capture proves the events arrive) and candidate 3 (the status is on the update
packet and *is* being consumed — see below).

The `--with-network` dump names the cause outright:

```
$ ff-rdp --port 7401 navigate https://example.com --with-network --jq '.results.network.entries'
[{"method":"GET","url":"https://example.com/","cause_type":"document","status":200,...},
 {"method":"GET","url":"data:,","cause_type":"img","status":200,...}]
```

The main document's resource is present, has `cause_type == "document"`, and carries
`status: 200`. What fails is the match: both `extract_document_event` and
`extract_document_status` select it with `r.url == requested_url`, an **exact string
comparison**, and Firefox canonicalises the requested `https://example.com` to
`https://example.com/` before issuing the request. `"https://example.com/" != "https://example.com"`,
so no resource is ever selected and `doc_status` stays `None` on every route.

Confirmed by supplying the canonical form by hand:

```
$ ff-rdp --port 7401 navigate https://example.com/ --jq '.results | {committed_url, status}'
{"committed_url":"https://example.com/","status":200}
```

**Cause: candidate 2 — `extract_document_status`/`extract_document_event` fail to match the main
document because of URL normalisation.** Candidates 1 and 3 are disproved above. The defect is
route-independent, which is why the plan's daemon-vs-direct framing (and its title) turned out to
be the wrong axis: the trailing slash, not the daemon, is what decides it.

### Theme B — fix it, and make `null` mean something

Whatever the cause, `status: null` must afterwards mean "the server sent no status", not "we
did not look". If a status genuinely cannot be observed for some navigation shapes (e.g. a
`data:` URL, or a bfcache restore with no network request at all), say so in the envelope
rather than emitting a bare `null`.

### Theme C — pin it with a live test on both routes

The reason this survived is that no test asserted the field on the plain path. Fix that for the
daemon route *and* the `--no-daemon` route — per CONTRIBUTING's daemon-parity rule, a feature
tested on only one of the two is how iteration-129 shipped broken.

### Theme B — as built

`CommitInfo` grew a `status_reason`, emitted as an always-present envelope key that is `null`
exactly when `status` is not, and otherwise one of three strings:

| `status_reason` | means |
|---|---|
| `not_observed` | this route never subscribed to `network-event`: `--no-wait`, or `back`/`forward`/`reload` |
| `no_document_request` | the document committed without issuing a request of its own — `about:blank`, bfcache, same-document nav |
| `no_status_reported` | the document's request was identified but Firefox never reported a status for it |

All three were produced live before being asserted (daemon route, Firefox 153):

```
$ ff-rdp --port 7401 navigate https://example.com --no-wait --jq '.results|{status,status_reason}'
{"status":null,"status_reason":"not_observed"}
$ ff-rdp --port 7401 navigate about:blank --allow-unsafe-urls --jq '.results|{status,status_reason}'
{"status":null,"status_reason":"no_document_request"}
```

`data:text/html,<h1>hi</h1>` turned out **not** to be a `no_document_request` case, contrary to
the theme's own guess: Firefox synthesises a network-event for it and reports `status: 200`. The
example in the theme text is wrong; `about:blank` is the real instance of that shape.

### Theme C — as built

`crates/ff-rdp-cli/tests/live/live_166_navigate_document_status.rs`, four tests: the two AC legs
plus `live_166_navigate_status_reflects_the_server` (a local fixture server, so a 200 that is
always 200 cannot pass — an unknown path must report the server's 404, on both routes) and
`live_166_null_status_carries_a_reason`.

## Acceptance Criteria [4/4]

- [x] live_166_navigate_reports_document_status: a live test asserts
      `navigate https://example.com` returns `results.status == 200` over the **daemon** route
      [2026-08-16: passes; also covers the trailing-slash form and `--with-network`, both of which
      reported `null` on main]
- [x] live_166_navigate_status_direct_parity: the same assertion over `--no-daemon`, so the two
      routes cannot diverge again unnoticed [2026-08-16: passes — and this leg was not a
      formality, `--no-daemon` reported `null` on main exactly like the daemon route]
- [x] unit_166_status_null_is_distinguishable: `null` is reserved for "the navigation produced
      no HTTP status" and a navigation whose status could not be observed says so explicitly
      (a `status_reason`, or an equivalent named field) — asserted without Firefox
      [2026-08-16: `status_reason`, three variants, wire strings pinned in the same test]
- [x] the cause is recorded in this plan (which of Theme A's three candidates it turned out to
      be), before the fix, with the measurement that settled it [2026-08-16: candidate 2, recorded
      in Theme A above and committed before the first line of the fix was written]

## Carry-over

Both filed into [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] before this PR
merges. See the PR body's `## Carry-over` table for the full enumeration, including the sweep
lines this iteration did not fix.

- **Theme A of 169** — the residual `live_138_navigate_reports_404` flake. iter-166 cut it from
  3-in-12 to 1-in-12 by re-deriving the grace budget from `status_reason`, but the remaining
  failures exhaust the full 2000 ms window with `no_status_reported`, so the update is lost in
  delivery rather than late. Not fixable inside this plan's scope.
- **Theme B of 169** — `back`/`forward`/`reload` emit no `status`/`status_reason` key at all.
  Recorded as out of scope in DEC-040.
- **Theme C of 169** — the second sweep's 7 `ff-rdp-core` failures. The hand-started Firefox on
  port 6000 was dead by the time the core tier ran (instant `ConnectionRefused`, PID gone); it
  survived the first sweep and, restarted, gave 9/9 immediately. "Environmental" is a diagnosis,
  not a disposition: something in the CLI tier kills a Firefox it does not own, and that is a
  scoping defect with its own row rather than a footnote.

## Notes

- Do **not** fix this by dropping the `status` field. It is the only thing in `navigate`'s
  default envelope that reports what the *server* said, as opposed to what the document ended up
  looking like.
- Related: [[iteration-164-two-failures-the-158-sweep-uncovered]] (where it was observed),
  [[iteration-160-envelope-honesty]] (same class), and
  [[analysis-2026-08-13-what-ff-rdp-became]] §3.2, whose `network` watcher regression is a
  *different* subsystem defect and is not in this plan.
