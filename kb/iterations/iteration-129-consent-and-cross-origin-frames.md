---
branch: iter-129/consent-and-cross-origin-frames
date: 2026-07-19
depends_on: []
dogfood_path: |
  ff-rdp launch --headless --auto-consent
  ff-rdp navigate https://www.theguardian.com
  ff-rdp eval 'document.documentElement.className.includes("sp-message-open")'
  # → false (CMP dismissed), page scrollable:
  ff-rdp scroll bottom --jq '.results.scrollHeight'
  # → substantially larger than the viewport height
first_call_sites: []
status: planned
---

# Iteration 129: consent handling + cross-origin frame reach

The single biggest blocker from [[dogfooding-session-62]] (finding 1, MAJOR): on
theguardian.com, `--auto-consent` (Consent-O-Matic) never records consent for the
Sourcepoint CMP in headless mode. The `sp_message_iframe_*` modal persists on every
page, `html.sp-message-open` sets `overflow:hidden`, so `scroll bottom` silently no-ops
(`atEnd:true`, `scrollHeight` == viewport) and content stays covered. Combined with
finding 6 — `click` cannot reach targets inside the cross-origin CMP iframe and times
out after 10 s with a generic "not ready" — there is **no CLI-native way to accept
consent**, and an agent is fully blocked on Sourcepoint-gated sites.

## Themes

- **A — research: frame targets over RDP.** Under Fission, cross-origin iframes get
  their own window-global targets via `watchTargets`. Establish how to enumerate the
  frame targets of the active tab and obtain a walker/console for a specific frame
  (Sourcepoint's `sp_message_iframe_*` as the concrete case). Write the outcome to
  `kb/research/frame-targets.md`; annotate any spec drift per the allow-spec-drift
  convention.
- **B — cross-origin frame reach for `click` (and `dom`).** When a selector matches
  nothing in the top document, search the tab's frame targets; act there when found.
  When a match exists only in a frame the command cannot act in, the error must say so
  (frame URL + suggestion) instead of a generic 10 s timeout.
- **C — native consent acceptance.** After navigation (when `--auto-consent` is
  active, or via an explicit `consent accept` subcommand), detect known CMPs —
  Sourcepoint first — and click the accept control inside the CMP frame using Theme B's
  mechanism. Keep a small per-CMP selector table; report which CMP was handled in the
  envelope. Document the Consent-O-Matic headless limitation.
- **D — scroll honesty on locked pages.** When `html`/`body` carries
  `overflow:hidden` and a scroll command moves nothing, emit a warning naming the
  locking element/class (e.g. `sp-message-open`) instead of a silent `atEnd:true`.

## Tasks

- [ ] A: frame-target enumeration spike against a local fixture page embedding a
      cross-origin iframe (e.g. https://example.com) + theguardian.com; record findings
      in `kb/research/frame-targets.md` with actor/method names.
- [ ] B: frame-aware selector resolution for `click`; on cross-frame-only matches,
      actionable error naming the frame; `dom` gets at minimum the improved error.
- [ ] C: CMP detection + accept flow (Sourcepoint selector set); wire into
      `--auto-consent` post-navigate and/or `consent accept`; envelope reports
      `{cmp: "sourcepoint", action: "accepted"}` or `{cmp: null}`.
- [ ] D: scroll-lock detection + warning in the scroll envelope.
- [ ] Update help/cookbook: consent workflow on CMP-gated sites.

## Acceptance Criteria [0/4]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_129_sourcepoint_consent (network-gated): navigate theguardian.com with the
      consent flow active → `document.documentElement.className` does NOT contain
      `sp-message-open`, and `scroll bottom` reaches a `scrollHeight` > 2× viewport
      height.
- [ ] live_129_click_cross_origin_frame: on a local fixture embedding a cross-origin
      iframe with a button, `click` either actuates the button (Theme B full reach) or
      fails with an error naming the frame URL — never a bare 10 s "not ready" timeout.
- [ ] live_129_scroll_lock_warning: on a fixture with `html{overflow:hidden}`,
      `scroll bottom` emits a warning identifying the scroll lock.
- [ ] live_129_consent_envelope: the consent flow's envelope reports the handled CMP
      (`sourcepoint`) on Guardian and `null` on a CMP-free page (example.com).

## Notes

Design-heavy iteration (frame targets are new protocol surface) — run with the opus
implement model. Research outcome may narrow Theme B (e.g. click-in-frame lands but
`dom` full traversal is deferred) — if so, file the carry-over plan before merge, per
discipline. Related platform constraint memory: no viewport actor
([[iteration-131-measurement-honesty]] covers the responsive side).
