---
title: "Dogfooding Session: nova-template jsonforms-index styling iteration"
date: 2026-04-13
type: dogfooding
status: completed
site: localhost:3000 (Next.js dev, comparis nova-template)
commands_tested:
  - tabs
  - navigate
  - reload
  - screenshot
  - eval
  - console
task: Iterate on a card-grid layout, verify visually + via DOM inspection
---

# Dogfooding Session: nova-template jsonforms-index styling iteration

Context: I was restyling a Next.js showcase page (`/nova-template/jsonforms-index`)
to use `@comparis/nova-patterns` components — card grid, `<Tag>`, typography,
theme tokens. Used `ff-rdp` to verify rendering and debug why my grid collapsed
to a single column. Found the root cause (Emotion-based `<Box>` was swallowing my
`style` prop), fixed it, and reloaded to confirm.

## What worked well

- **`navigate` + `reload` + `screenshot`** — tight visual loop. Worked first try.
  Being able to grab a PNG and read it back into the assistant context made the
  "does it look right?" question one-shot.
- **`console --level error`** surfaced an Emotion warning
  (`"Using kebab-case for css properties in objects is not supported. Did you mean dataTest?"`)
  that was a hint toward the real bug. Filtering by level was essential — the
  raw console dump was ~500 messages of React dev noise.
- **`eval` with `getComputedStyle`** was the key diagnostic. Being able to ask
  "what's the *resolved* `grid-template-columns` on this element?" pinpointed the
  single-column collapse in seconds. Faster than any DevTools session would be
  over an SSH terminal.
- **`tabs`** at the start correctly showed the previous tab state (stale
  connection-failed URL), which saved me from opening a new tab blindly.

## Issues / friction encountered

### 1. `eval` rejects source containing optional chaining (`?.`) via CLI arg — SyntaxError at col 1

Queries I tried:
```
ff-rdp eval 'getComputedStyle(document.querySelectorAll("a")[0]?.parentElement).display'
```
Result:
```json
{"name":"SyntaxError","stack":"@debugger eval code:1:1\n"}
```

Rewriting without `?.` (e.g. `x && x.parentElement`) worked. Suspect the shell
is stripping or mangling the character, OR the debugger's JS evaluator is on an
older ECMAScript level. Either way:

- **Ask:** either document the supported syntax, or add a `--file` / `--stdin`
  flag to `eval` so scripts with special characters can be passed without shell
  quoting nightmares. I tried `--file` and `--stdin`; neither exists (there is
  only `--fields`, which is unrelated).

### 2. Multi-statement JS in `eval` is fragile

A statement like `var x = ...; JSON.stringify({...})` consistently fails with
SyntaxError when passed as a single argument. I had to refactor to a single
expression (ternary) to get it through. Inherited from #1 — a `--file`
flag would sidestep this entirely.

### 3. `screenshot` has no full-page flag

`--full-page` was rejected. My page scrolled below the fold, so the first
screenshot cut off half of the 9-card grid. I worked around it by letting
the viewport height default, but for long pages this is a real limitation.

**Ask:** add `--full-page` (emit `document.scrollingElement.scrollHeight`
screenshot) or at least `--viewport-height N` to size the capture.

### 4. `navigate` help confused me on the flag name

I tried `ff-rdp navigate --url "..."`; it rejected and hinted
`'--url' as a value, use '-- --url'`. The positional `<URL>` form
works, but the hint is slightly misleading — it sounds like I should
add `-- --url` which isn't what I actually want. Minor, but
a short clarifying sentence in help output would help.

### 5. `console --limit` default is too high

`ff-rdp console` returned 495 messages with `--limit 20` (seemingly
capped differently, or limit applies to something else). I ended up
needing `--level error` to find real errors. A clear per-invocation
summary (counts by level) before the message array would save grep work.

## Enhancements I'd have reached for

- **`watch-console` / streaming** — I reloaded the page and needed to know
  when HMR finished compiling. Ended up with `sleep 5`. A
  `ff-rdp wait --console-includes "compiled"` would've been neater.
  (Related: [[backlog/issues/daemon-realtime-watcher-events]])
- **`check-computed` or `assert`** — inline shortcut for
  `getComputedStyle(sel)[prop]`. I wrote the JS four times in this session
  for `display`, `gridTemplateColumns`, parent width, etc. A command like
  `ff-rdp computed --selector ".grid" --prop grid-template-columns`
  would be the single most reused call.
- **`screenshot --annotate`** — highlight an element (box + label) before
  capture. Would have let me send a single PNG confirming *the grid
  container* rendered, instead of "here's the page, look at it yourself".
- **`reload --wait-idle`** — reload then block until network idle + HMR
  done. The manual `sleep 5` is guesswork.

## Root cause of my actual bug (unrelated to ff-rdp, but documenting because ff-rdp found it)

`@comparis/nova-patterns`' `<Box>` component spreads *all* non-margin/padding
props into the Emotion `css()` call — including `style`. So
`<Box style={{gridTemplateColumns: "..."}}>` does NOT set an inline style;
it gets serialized as `{ style: { gridTemplateColumns: ... } }` into the
emotion CSS object where Emotion silently drops it. The fix was to pass
`gridTemplateColumns` directly as a prop (it's in `ExtendedProps` via
`CSSProperties`). Without `eval + getComputedStyle` this would have been
a much longer hunt.

## Verdict

ff-rdp made this a fast loop. Top blockers were:
1. `eval` argument parsing (optional chaining, multi-statement)
2. No full-page screenshot
3. No computed-style shortcut (I kept reinventing it)

Everything else was nice-to-have.
