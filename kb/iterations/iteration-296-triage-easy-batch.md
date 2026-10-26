---
branch: iter-296/triage-easy-batch
date: 2026-10-26
depends_on: []
status: done
type: iteration
title: "Iteration 296: Easy fixes from the 2026-10-26 issue triage"
tags:
  - iteration
  - a11y
  - screenshot
  - launch
  - click
---

# Iteration 296: Easy fixes from the 2026-10-26 issue triage

The 2026-10-26 triage of `kb/backlog/issues/` reproduced each report against ff-rdp 0.4.2
(86baf13d) and Firefox 158.0.1. This iteration lands every report classed easy, plus the launch
change that keeps a crashed headless Firefox from opening a crash-reporter dialog on the desktop.
Root causes below are the triage's; re-verify each before changing code.

## Tasks [10/10]

### Page view (`crates/ff-rdp-cli/src/commands/page_view_js.rs`)

- [x] [[a11y-summary-banner-for-nested-header]]: `LANDMARKS_BLOCK_JS` (~275-287) maps every
      `header`→banner and `footer`→contentinfo. Skip both when an ancestor is
      `article,aside,main,nav,section` or the matching ARIA roles.
- [x] [[a11y-summary-headings-not-in-document-order]]: the heading collector (~219-225) runs one
      `querySelectorAll` per level. Use a single `h1,h2,h3,h4,h5,h6` pass in document order and
      take the level from the tag. This also changes `navigate --with-page`.
- [x] [[navigate-with-page-title-is-og-title]]: `result.title` comes only from Readability
      (~341), so it is the `og:title` or missing when Readability fails. Always set
      `title = document.title`, and put Readability's title in `reader_title`.

### Screenshot (`crates/ff-rdp-cli/src/commands/screenshot.rs`)

- [x] [[screenshot-output-root-default-path]]: with `--output-root` and no `-o`,
      `resolve_output_path` (~954) returns a cwd-relative name that then "escapes" the root. When
      a root is set and no `-o` is given, default to `<root>/screenshot-<ts>.png`.
- [x] [[screenshot-full-page-layout-shift]]: `freeze_fixed_and_sticky_js` (~771-791) sets
      `position: absolute` on sticky elements, which takes them out of flow (content jumps up)
      and shrinks them to fit their content (header narrows). Freeze sticky elements as
      `position: relative; top: auto`. For fixed elements, keep absolute and pin width/height to
      the bounding rect.
- [x] [[screenshot-window-size-full-page]] part 1: drop `conflicts_with = "full_page"` on
      `--window-size` (`cli/args.rs` ~2202). With `--full-page`, the batch capture passes
      `--window-size=<W>` only, which makes Firefox capture the full page. When Firefox writes no
      PNG (seen on very tall pages: "Failed to allocate a surface"), return a clear error. Part 2
      (usage errors as a JSON envelope) is out of scope; it goes to the backlog.

### Launch, navigate and click

- [x] [[navigate-file-url-crashes-firefox]] (do this first): `navigate --allow-file-urls file://…`
      crashes the whole browser ("Illegal load attempt of file: URL from webIsolated"). Refuse
      `file:` in `validate_content_navigation_url` (`commands/url_validation.rs` ~48-65), the
      same way it already refuses `about:support`. Point the error at
      `ff-rdp launch --allow-file-urls --url file://…`, which was verified to work. Check whether
      `nav_action`, `tabs` or other paths open URLs without that validator. Update the
      `--allow-file-urls` help. Unit and e2e tests only; no live test that reproduces the crash.
- [x] Launch sets `MOZ_CRASHREPORTER_DISABLE=1` in the environment of the Firefox it starts, so
      a crashing headless instance never shows a crash-reporter dialog. On 2026-10-26 four such
      dialogs landed on the owner's desktop during triage.
- [x] [[launch-window-size-exits]]: does not reproduce (5/5 launches succeeded). The redacted
      38 bytes of stderr were just `*** You are running in headless mode.` Add a hint to the
      "exited immediately" error (`launch.rs` ~1252 / `launch/startup.rs` `summary()`) that names
      how to see Firefox's stderr (`--log-level debug`, `FF_RDP_TRACE_RAW=1`). Then close the
      issue as cannot-reproduce.
- [x] [[click-without-user-activation]] interim: add one sentence to the `click` help saying the
      page gets no user activation, so `window.open`, clipboard and fullscreen calls are refused.
      Trusted input stays open as a backlog item; the issue file stays open.

## Bookkeeping (same commit)

- Set `status: resolved` and `resolved_by: <PR>` on each fixed issue file, and add a ticked
  `kb/backlog.md` line for each.
- Add unticked backlog lines, each with `(iter-296 triage)`, for:
  - [[click-without-user-activation]] trusted input.
  - [[screenshot-narrow-viewport-live-state]] (research spike: resize `gBrowser.selectedBrowser`
    from the chrome console, the way Responsive Design Mode does).
  - [[screenshot-window-size-full-page]] part 2 (clap usage errors as a JSON envelope; needs a
    decision-log entry).
  - Routing `file:` navigation through the parent process instead of refusing it (optional
    follow-up to [[navigate-file-url-crashes-firefox]]).
- Commit the new issue file `navigate-file-url-crashes-firefox.md`, resolved by this PR.

## Acceptance criteria [9/9]

- [x] `navigate --allow-file-urls file:///etc/hosts` returns an error envelope that names
      `launch --url`, and the Firefox it targets is still running afterwards.
- [x] `a11y summary` on a page with a page-level `<header>` and `<header>`s inside `<article>`s
      reports exactly one banner landmark.
- [x] `a11y summary` lists headings in document order (`Title, A, A.1, A.2, B, B.1` for the
      outline page).
- [x] `navigate --with-page` reports `page.title` equal to `document.title` on a page whose
      `og:title` differs.
- [x] `screenshot --output-root DIR` with no `-o` writes the file inside DIR.
- [x] `screenshot --full-page` on a page with a 72px sticky header shows the header at full
      width and the content below it at y ≥ 72, as in the viewport capture.
- [x] `screenshot --window-size 390x844 --full-page` writes a PNG 390 px wide and taller than
      844 px.
- [x] A Firefox started by `ff-rdp launch` has `MOZ_CRASHREPORTER_DISABLE=1` in its environment.
- [x] `ff-rdp click --help` states that the click gives the page no user activation.
