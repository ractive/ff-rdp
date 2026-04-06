# ff-rdp

A fast Rust CLI for the Firefox Remote Debugging Protocol. Communicates directly over TCP with Firefox's built-in debugger for minimal latency.

## Status

**Early development** — core commands working: `tabs`, `navigate`, `eval`, `dom`, `page-text`, `console`, `network`, `reload`, `back`, `forward`. Remaining commands (`screenshot`) are planned.

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
  network     Show network requests (with --filter, --method, --cached filters)
  screenshot  Capture a screenshot
  reload      Reload the page
  back        Go back in history
  forward     Go forward in history

Options:
  --host <HOST>        Firefox debug server host [default: localhost]
  --port <PORT>        Firefox debug server port [default: 6000]
  --tab <TAB>          Target tab by index (1-based) or URL substring
  --tab-id <TAB_ID>    Target tab by exact actor ID
  --jq <JQ>            jq filter expression applied to output
  --timeout <TIMEOUT>  Operation timeout in milliseconds [default: 5000]
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

# Retrospective network data via Performance API
ff-rdp network --cached

# Navigate and capture all network traffic in one shot
ff-rdp navigate https://example.com --with-network

# Find failed requests during navigation
ff-rdp navigate https://example.com --with-network \
  --jq '.results.network[] | select(.status >= 400)'

# Reload, go back, go forward
ff-rdp reload
ff-rdp back
ff-rdp forward
```

## Architecture

- **ff-rdp-core** — Protocol library: blocking TCP transport, length-prefixed JSON framing, typed errors
- **ff-rdp-cli** — CLI binary: clap args, jq output pipeline, command dispatch

## License

MIT
