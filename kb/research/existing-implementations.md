---
title: Existing Firefox RDP Client Implementations
type: research
date: 2026-04-06
tags: [geckordp, foxdriver, firefox-devtools-mcp, research]
status: completed
---

# Existing Firefox RDP Implementations

Three reference implementations were cloned to ~/devel/ for study.

## geckordp (Python) — Primary Reference

**Repo**: ~/devel/geckordp (https://github.com/jpramosi/geckordp)
**Version**: 1.0.3 | **Language**: Python | **License**: MIT

### Why It's the Best Reference

Most complete RDP client. Implements all major actors with clean, readable code. The Python source maps almost 1:1 to the protocol messages we need to send.

### Key Files

| File | What to learn |
|------|---------------|
| `geckordp/rdp_client.py` | TCP framing, send/recv, event dispatch |
| `geckordp/buffers.py` | LinearBuffer for TCP stream reassembly |
| `geckordp/actors/root.py` | Root actor: listTabs, getRoot, getProcess |
| `geckordp/actors/web_console.py` | evaluateJSAsync, startListeners, getCachedMessages |
| `geckordp/actors/descriptors/tab.py` | getTarget, getWatcher, getFavicon |
| `geckordp/actors/targets/window_global.py` | navigateTo, reload, goBack, goForward, focus |
| `geckordp/actors/watcher.py` | watchResources, watchTargets |
| `geckordp/actors/network_event.py` | getRequestHeaders, getResponseContent, etc. |
| `geckordp/actors/inspector.py` | getWalker, getPageStyle |
| `geckordp/actors/walker.py` | querySelector, querySelectorAll, document |
| `geckordp/actors/node.py` | getUniqueSelector, modifyAttributes |
| `geckordp/actors/thread.py` | attach, resume, frames, interrupt, sources |
| `geckordp/actors/screenshot.py` | Screenshot capture |

### Connection Flow (from geckordp)

```python
# 1. Connect
client = RDPClient()
client.connect("localhost", 6000)

# 2. Get root info
root = RootActor(client)
root_info = root.get_root()

# 3. List tabs
tabs = root.list_tabs()
tab_actor = tabs[0]["actor"]

# 4. Get target
tab = TabActor(client, tab_actor)
target_info = tab.get_target()
console_actor = target_info["consoleActor"]

# 5. Evaluate JS
console = WebConsoleActor(client, console_actor)
result = console.evaluate_js_async("document.title")
```

### Event Handling

- Events registered by actor ID + event type
- Handler dictionary: `__event_handlers[event_type][actor_id] = [handlers]`
- Events detected by presence of `type` field in response
- Universal listeners available for debugging (see all messages)

### Platform Support

- Ubuntu 24.04: confirmed
- Windows, macOS: untested (marked "?")
- Requires Firefox 136.0+, Python 3.10+

## foxdriver (Node.js) — High-Level API

**Repo**: ~/devel/foxdriver (https://github.com/saucelabs/foxdriver)
**Language**: JavaScript/Node.js | **By**: Sauce Labs

### Architecture

Higher-level API than geckordp. Provides:
- `Browser` class: connects and manages tabs
- `Tab` class: wraps target actor
- Individual actor classes for console, network, etc.

### Useful Patterns

- Clean async/await API design
- Event emitter pattern for notifications
- Tab discovery and management
- Good reference for API ergonomics (what a user-friendly interface looks like)

### Limitations

- Less actively maintained than geckordp
- Doesn't cover newer actors (Watcher, TargetConfiguration)
- Node.js only

## firefox-devtools-mcp (TypeScript) — Mozilla Official

**Repo**: ~/devel/firefox-devtools-mcp (https://github.com/mozilla/firefox-devtools-mcp)
**Version**: 0.9.1 | **Language**: TypeScript | **By**: Mozilla

### Key Insight: Uses WebDriver BiDi, NOT raw RDP

Despite the name, this MCP server uses Selenium WebDriver with WebDriver BiDi protocol, NOT the raw Remote Debugging Protocol. This adds significant overhead:

```
AI tool → MCP server (Node.js) → Selenium → Marionette/BiDi → Firefox
```

vs our approach:

```
ff-rdp binary → TCP → Firefox RDP
```

### Capabilities Exposed (30+ tools)

Good reference for what capabilities an AI debugging tool needs:
- Page management (list, navigate, create, close)
- DOM interaction via unique IDs
- Network request capture
- Console message logging
- Screenshot capture
- JS evaluation
- WebExtension management
- Firefox preference management

### What to Learn From It

- Tool naming and descriptions (optimized for LLM understanding)
- Which capabilities are most used by AI assistants
- Error handling patterns for browser automation
- Not a protocol reference (it doesn't use RDP directly)

## No Rust Implementation Exists

None of the search results found a Rust library for Firefox RDP. ff-rdp would be the first Rust implementation. The closest Rust work is:
- `cdp` crate: Chrome DevTools Protocol in Rust (different protocol entirely)
- `tokio-cdp`: Async CDP client for Rust

These confirm the Rust ecosystem is ready for browser automation (tokio, serde_json, etc.) but Firefox RDP is uncharted territory.
