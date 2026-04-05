---
title: "Iteration 7: Should-Have Features"
type: iteration
date: 2026-04-06
tags: [iteration, screenshot, cookies, storage, launch]
status: planned
branch: iter-7/extras
---

# Iteration 7: Should-Have Features

Screenshots, cookie access, web storage, and a convenience launcher.

## Tasks

- [ ] Implement `ff-rdp-cli/src/commands/screenshot.rs` — `ff-rdp screenshot [--path <file>] [--selector <css>] [--tab ...]`
- [ ] Screenshot via eval: canvas-based capture or screenshotActor if available
- [ ] Implement `ff-rdp-cli/src/commands/cookies.rs` — `ff-rdp cookies [--tab ...] [--domain <d>] [--name <n>]`
- [ ] Cookies via eval: `document.cookie` parsing (note: HttpOnly cookies not accessible this way)
- [ ] Implement `ff-rdp-cli/src/commands/storage.rs` — `ff-rdp storage <local|session> [--tab ...] [--key <k>]`
- [ ] Storage via eval: `JSON.parse(JSON.stringify(localStorage))` / `sessionStorage`
- [ ] Implement `ff-rdp launch` — start Firefox with correct flags: `firefox --start-debugger-server <port> [-headless] [--profile <path>]`
- [ ] Launch should detect Firefox binary location per platform (macOS: /Applications, Linux: PATH, Windows: Program Files)
- [ ] Launch should optionally create a temporary profile for clean debugging sessions

## Acceptance Criteria

- `ff-rdp screenshot` saves PNG to current directory with auto-generated filename
- `ff-rdp screenshot --path /tmp/page.png` saves to specified path
- `ff-rdp cookies` lists all accessible cookies as JSON
- `ff-rdp cookies --name "session_id"` extracts a specific cookie
- `ff-rdp storage local` dumps localStorage as JSON object
- `ff-rdp storage session --key "token"` gets a specific sessionStorage value
- `ff-rdp launch` starts Firefox and prints the connection info (host:port)
- `ff-rdp launch --headless` starts headless Firefox
