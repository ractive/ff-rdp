# ff-rdp

A fast Rust CLI for the Firefox Remote Debugging Protocol. Communicates directly over TCP with Firefox's built-in debugger for minimal latency.

## Status

**Early development** — project scaffolding and transport layer complete. Commands are not yet functional.

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
  navigate    Navigate to a URL
  eval        Evaluate JavaScript in the target tab
  page-text   Get page information
  console     Read console messages
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
ff-rdp tabs --jq '.results[].url'
```

## Architecture

- **ff-rdp-core** — Protocol library: async TCP transport, length-prefixed JSON framing, typed errors
- **ff-rdp-cli** — CLI binary: clap args, jq output pipeline, command dispatch

## License

MIT
