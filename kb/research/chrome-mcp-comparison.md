---
title: Chrome MCP vs ff-rdp Architecture Comparison
type: research
date: 2026-04-06
tags: [chrome, mcp, comparison, performance, research]
status: completed
---

# Chrome MCP vs ff-rdp Performance Comparison

## Chrome MCP Architecture (claude --chrome)

```
Claude Code CLI
  ↕ (tool call dispatch)
Deferred tool loader (loads schema on first use)
  ↕ (JSON-RPC)
MCP client
  ↕ (Native Messaging IPC)
Chrome Extension (service worker)
  ↕ (Chrome Extension APIs)
Chrome browser
```

### Why It's Slow

1. **Deferred tool loading**: Each Chrome tool must be loaded via ToolSearch before first use, adding a round-trip
2. **Native Messaging IPC**: Communication between Claude Code and Chrome extension via OS-level IPC pipe
3. **Extension service worker lifecycle**: Chrome extension service workers can go idle, requiring reconnection
4. **Multiple round-trips**: A typical debugging task needs 10-20 tool calls, each going through the full stack
5. **Serialization overhead**: JSON-RPC wrapping + MCP protocol overhead on every message
6. **No pipelining**: Must wait for each tool call to complete before issuing the next

### Typical Debugging Session

```
1. tabs_context_mcp     (load schema + call)  ~200ms
2. navigate             (load schema + call)  ~300ms
3. read_page            (load schema + call)  ~400ms  (large DOM)
4. read_console_messages (load schema + call) ~200ms
5. javascript_tool      (load schema + call)  ~200ms
6. read_network_requests (load schema + call) ~300ms
Total: ~1600ms+ for basic page inspection
```

### Available Chrome MCP Tools

| Tool | Purpose |
|------|---------|
| `navigate` | Go to URL |
| `read_page` | Get DOM content |
| `get_page_text` | Extract text |
| `find` | Find elements |
| `computer` | Click/scroll/interact |
| `form_input` | Fill forms |
| `javascript_tool` | Eval JS |
| `read_console_messages` | Console output |
| `read_network_requests` | Network traffic |
| `tabs_context_mcp` | List tabs |
| `tabs_create_mcp` | New tab |
| `gif_creator` | Record interaction |
| `resize_window` | Resize |
| `upload_image` | File upload |

## ff-rdp Architecture (proposed)

```
Claude Code CLI
  ↕ (single Bash tool call)
ff-rdp binary
  ↕ (TCP: length-prefixed JSON)
Firefox browser
```

### Why It's Faster

1. **Direct TCP**: No middleware, no IPC, no extension service worker
2. **Native binary**: Rust compiles to machine code, sub-ms startup
3. **Single invocation**: One Bash call replaces 3-5 MCP tool calls
4. **No schema loading**: CLI args are parsed locally, no deferred loading
5. **Minimal serialization**: Just `length:JSON` framing, no MCP/JSON-RPC wrapping
6. **Connection reuse within invocation**: One TCP connection serves multiple protocol messages

### Estimated Session

```
ff-rdp tabs                                    ~15ms
ff-rdp navigate https://example.com --tab 1    ~20ms
ff-rdp eval 'document.title' --tab 1           ~12ms
ff-rdp console --tab 1                         ~15ms
ff-rdp network --tab 1                         ~15ms
Total: ~77ms for same inspection
```

**~20x faster** for a typical debugging session.

### Additional Advantages

- **Pipe-friendly**: `ff-rdp eval '...' | jq '.results'` — composable with shell tools
- **No browser extension required**: Firefox's debug server is built-in
- **Works headless**: `firefox -headless --start-debugger-server 6000`
- **No account required**: Chrome MCP requires Anthropic Pro/Max plan; ff-rdp works with any Firefox
- **jq built-in**: `ff-rdp tabs --jq '.results[0].url'` — no external jq dependency

### Trade-offs

| Aspect | Chrome MCP | ff-rdp |
|--------|-----------|--------|
| Browser | Chrome (user's real session) | Firefox (separate debug instance) |
| Auth state | Uses existing logins | Separate Firefox profile |
| Visual feedback | Real-time in Chrome window | Headless possible |
| Setup | Install extension | Start Firefox with flag |
| Ecosystem | Part of Claude Code | Standalone tool |
