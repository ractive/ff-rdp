# ff-rdp

A fast Rust CLI for the Firefox Remote Debugging Protocol. Communicates directly over TCP with Firefox's built-in debugger for minimal latency.

## Status

**Early development** — all planned commands working: `tabs`, `navigate`, `eval`, `dom`, `page-text`, `console`, `network`, `perf`, `click`, `type`, `wait`, `cookies`, `storage`, `screenshot`, `launch`, `inspect`, `sources`, `reload`, `back`, `forward`.

## Requirements

- Firefox with remote debugging enabled:
  ```bash
  firefox --start-debugger-server 6000
  ```
- Rust toolchain (for building from source)

## Building

```bash
cargo build --release
```

The binary is at `target/release/ff-rdp`.

## Usage

```
ff-rdp [OPTIONS] <COMMAND>

Commands:
  tabs        List open browser tabs
  navigate    Navigate to a URL (with --with-network for traffic capture)
  eval        Evaluate JavaScript in the target tab
  dom         Query DOM elements by CSS selector (--outer-html, --inner-html, --text, --attrs)
  page-text   Extract visible page text (document.body.innerText)
  console     Read console messages (with --level and --pattern filters)
  network     Show network requests (with --filter, --method filters)
  perf        Query Performance API entries and Core Web Vitals
  click       Click an element matching a CSS selector
  type        Type text into an input element matching a CSS selector
  wait        Wait for a condition to become true (polls every 100ms)
  cookies     List cookies via StorageActor (includes httpOnly, secure, sameSite)
  storage     Read web storage (localStorage or sessionStorage)
  screenshot  Capture a screenshot (requires Firefox drawWindow support)
  inspect     Inspect a remote JavaScript object by its grip actor ID
  sources     List JavaScript/WASM sources loaded on the page
  launch      Launch Firefox with remote debugging enabled
  reload      Reload the page
  back        Go back in history
  forward     Go forward in history

Options:
  --host <HOST>              Firefox debug server host [default: localhost]
  --port <PORT>              Firefox debug server port [default: 6000]
  --tab <TAB>                Target tab by index (1-based) or URL substring
  --tab-id <TAB_ID>          Target tab by exact actor ID
  --jq <JQ>                  jq filter expression applied to output
  --timeout <TIMEOUT>        Operation timeout in milliseconds [default: 5000]
  --no-daemon                Don't use or start a daemon (direct Firefox connection)
  --daemon-timeout <SECS>    Daemon idle timeout in seconds [default: 300]
```

All output is JSON with a standard envelope (`results`, `total`, `meta`). Use `--jq` to filter:

```bash
# List tab URLs
ff-rdp tabs --jq '.results[].url'

# Navigate to a URL
ff-rdp navigate https://example.com

# Evaluate JavaScript and extract the result
ff-rdp eval 'document.title' --jq '.results'

# Target a specific tab by URL substring
ff-rdp eval 'location.href' --tab example.com

# Query DOM elements by CSS selector (default: outerHTML)
ff-rdp dom "h1"

# Get text content of matching elements
ff-rdp dom "ul li" --text

# Get element attributes as JSON
ff-rdp dom "a" --attrs

# Extract all visible page text
ff-rdp page-text

# Count characters in page text
ff-rdp page-text --jq '.results | length'

# Read console messages (errors only)
ff-rdp console --level error

# Filter console messages by pattern
ff-rdp console --pattern "TypeError"

# Show network requests
ff-rdp network

# Filter network by URL substring
ff-rdp network --filter api

# Filter network by HTTP method
ff-rdp network --method POST

# Navigate and capture all network traffic in one shot
ff-rdp navigate https://example.com --with-network

# Find failed requests during navigation
ff-rdp navigate https://example.com --with-network \
  --jq '.results.network[] | select(.status >= 400)'

# Query Performance API resource timing entries
ff-rdp perf

# Page load waterfall (DNS, TLS, TTFB, DOM timings)
ff-rdp perf --type navigation

# First Paint and First Contentful Paint timestamps
ff-rdp perf --type paint

# Largest Contentful Paint
ff-rdp perf --type lcp

# Cumulative Layout Shift entries
ff-rdp perf --type cls

# Long tasks (>50ms)
ff-rdp perf --type longtask

# Filter resource entries by URL substring
ff-rdp perf --filter "api/"

# Core Web Vitals summary with ratings (LCP, CLS, TBT, FCP, TTFB)
ff-rdp perf vitals

# Extract a single metric
ff-rdp perf vitals --jq '.results.lcp_ms'

# Click a button
ff-rdp click "button.submit"

# Type into an input (clear first with --clear)
ff-rdp type "input[name=email]" "user@example.com"
ff-rdp type "input[name=email]" "new@example.com" --clear

# Wait for an element to appear (default timeout: 5000ms)
ff-rdp wait --selector ".results"

# Wait for text to appear on the page
ff-rdp wait --text "Success" --wait-timeout 10000

# Wait for a JavaScript expression to become truthy
ff-rdp wait --eval "document.readyState === 'complete'"

# List cookies
ff-rdp cookies

# Filter cookies by name
ff-rdp cookies --name "session_id"

# Dump all localStorage
ff-rdp storage local

# Get a specific sessionStorage key
ff-rdp storage session --key "token"

# Capture a screenshot (saves PNG)
ff-rdp screenshot --output page.png

# Launch Firefox with debugging enabled
ff-rdp launch

# Launch headless Firefox with temporary profile
ff-rdp launch --headless --temp-profile

# Inspect a remote object grip (from eval output)
ff-rdp inspect server1.conn0.child2/obj19

# Recursive inspection (depth 2)
ff-rdp inspect server1.conn0.child2/obj19 --depth 2

# List all loaded JavaScript sources
ff-rdp sources

# Filter sources by URL substring
ff-rdp sources --filter vendor

# Filter sources by regex pattern
ff-rdp sources --pattern "cdn\.example\.com"

# Reload, go back, go forward
ff-rdp reload
ff-rdp back
ff-rdp forward
```

## Daemon Mode

By default, the first CLI invocation auto-starts a background daemon that holds a persistent Firefox RDP connection and buffers watcher events. Subsequent invocations connect through the daemon for faster execution and cross-command workflows.

**How it works:**
- First `ff-rdp` call spawns a daemon process (`ff-rdp _daemon`) in the background
- The daemon connects to Firefox, subscribes to watcher resources (network, console, errors), and listens on a random TCP loopback port
- Subsequent CLI calls connect to the daemon instead of Firefox directly
- The daemon transparently proxies RDP frames and also exposes a `"daemon"` virtual actor for draining buffered events
- Daemon exits automatically after 5 minutes of inactivity (configurable via `--daemon-timeout`)

**Cross-command workflows (enabled by daemon):**
```bash
# Navigate, then inspect network traffic as separate commands
ff-rdp navigate https://example.com
ff-rdp network

# Object grips from eval survive across invocations
ff-rdp eval 'document.querySelector("h1")'
ff-rdp inspect server1.conn0.child2/obj19
```

**Disabling the daemon:**
```bash
# Connect directly to Firefox (original behavior)
ff-rdp --no-daemon eval "1+1"
```

**Registry and logs:**
- Registry file: `~/.ff-rdp/daemon.json` (PID, port, Firefox target)
- Log file: `~/.ff-rdp/daemon.log`
- Stale registry files are cleaned up automatically when the daemon PID is dead

**Troubleshooting:**
- If the daemon seems stuck, delete `~/.ff-rdp/daemon.json` to force a fresh start
- Use `--no-daemon` to bypass the daemon and test direct connectivity
- Check `~/.ff-rdp/daemon.log` for daemon-side errors

## Architecture

- **ff-rdp-core** — Protocol library: blocking TCP transport, length-prefixed JSON framing, typed errors
- **ff-rdp-cli** — CLI binary: clap args, jq output pipeline, command dispatch, daemon proxy

## License

MIT
