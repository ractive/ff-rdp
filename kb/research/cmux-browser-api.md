---
title: cmux Browser CLI API Reference
type: research
date: 2026-04-06
tags: [cmux, browser, cli, ux-reference, research]
status: completed
---

# cmux Browser CLI — UX Reference for ff-rdp

cmux provides a browser automation CLI via `cmux browser <subcommand>`. This is the UX model to follow for ff-rdp's command design.

## Addressing Model

cmux uses **surfaces** to identify browser tabs:

```bash
# By surface ref
cmux browser surface:1 navigate https://example.com

# By --surface flag
cmux browser --surface surface:1 snapshot

# Open creates a new surface
cmux browser open https://example.com
```

Surface IDs come from `cmux browser identify` or `cmux list-pane-surfaces`.

### ff-rdp Equivalent

We use `--tab` for targeting:

```bash
# By index (from ff-rdp tabs)
ff-rdp navigate https://example.com --tab 1

# By URL substring
ff-rdp eval 'document.title' --tab example.com

# Default: active/selected tab
ff-rdp eval 'document.title'
```

## cmux Browser Subcommands (Full List)

### Navigation
- `open [url]` — create browser split, optionally navigate
- `goto|navigate <url>` — navigate existing surface
- `back|forward|reload` — history navigation

### Reading State
- `snapshot [--interactive] [--compact] [--selector <css>]` — DOM snapshot
- `get <url|title|text|html|value|attr|count|box|styles>` — extract data
- `url|get-url` — current URL
- `console <list|clear>` — console messages
- `errors <list|clear>` — error messages

### Interaction
- `click|dblclick|hover|focus|check|uncheck|scroll-into-view [--selector <css>]`
- `type|fill [--selector <css>] [--text <text>]`
- `press|key|keydown|keyup [--key <key>]`
- `select [--selector <css>] [--value <value>]`
- `scroll [--selector <css>] [--dx <n>] [--dy <n>]`

### JavaScript
- `eval [--script <js> | <js>]` — execute JS

### Finding Elements
- `find <role|text|label|placeholder|alt|title|testid|first|last|nth>`

### Waiting
- `wait [--selector <css>] [--text <text>] [--url-contains <text>] [--function <js>] [--timeout <seconds>]`

### Screenshots
- `screenshot [--out <path>]`

### Data
- `cookies <get|set|clear>` — cookie management
- `storage <local|session> <get|set|clear>` — web storage

### Tabs
- `tab <new|list|switch|close|<index>>`

### Advanced
- `frame <main|selector>` — switch iframe context
- `dialog <accept|dismiss>` — handle alerts
- `network <route|unroute|requests>` — network interception
- `viewport <width> <height>` — resize
- `highlight [--selector <css>]` — visual highlight
- `state <save|load>` — save/restore browser state

## Key Design Patterns to Adopt

### 1. `--snapshot-after` flag

Many cmux commands accept `--snapshot-after` to capture state after an action:

```bash
cmux browser surface:1 click --selector "button.submit" --snapshot-after
```

**ff-rdp equivalent**: Could add `--eval-after <js>` to action commands, or a general `--then eval '<js>'` chaining mechanism.

### 2. CSS selectors as primary element targeting

cmux uses `--selector <css>` consistently. ff-rdp should do the same.

### 3. Separate `get` subcommand for data extraction

cmux has `get <url|title|text|html|value|attr|count|box|styles>` as a unified data extraction command. Worth considering for ff-rdp:

```bash
ff-rdp get url --tab 1
ff-rdp get title --tab 1
ff-rdp get text --selector "#content"
ff-rdp get html --selector "#content"
```

### 4. Wait conditions

cmux's `wait` command is well-designed:

```bash
cmux browser surface:1 wait --selector ".loaded" --timeout 10
cmux browser surface:1 wait --text "Success"
cmux browser surface:1 wait --function "() => document.readyState === 'complete'"
```

ff-rdp should match this:

```bash
ff-rdp wait --selector ".loaded" --timeout 10000
ff-rdp wait --text "Success"
ff-rdp wait --eval "document.readyState === 'complete'"
```

### 5. Console and errors as separate lists

cmux splits `console list` and `errors list`. ff-rdp could merge them with a `--level` filter:

```bash
ff-rdp console                    # all messages
ff-rdp console --level error      # errors only
ff-rdp console --pattern "API"    # filter by content
```

## Commands NOT Needed in ff-rdp (cmux-specific)

- `open-split` — cmux layout management
- `highlight` — visual overlay (cmux has a GUI)
- `screencast` — video recording
- `state save/load` — session persistence
- `network route/unroute` — request interception (complex, defer)
- `geolocation` / `offline` — device emulation
- `input mouse/keyboard/touch` — low-level input
- `addinitscript/addstyle` — page modification
