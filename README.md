# ff-rdp

A fast Rust CLI for the Firefox Remote Debugging Protocol. Communicates directly over TCP with Firefox's built-in debugger for minimal latency.

## Installation

### Homebrew (macOS & Linux)

```sh
brew tap ractive/tap
brew install ractive/tap/ff-rdp
```

### Scoop (Windows)

```powershell
scoop bucket add ff-rdp https://github.com/ractive/scoop-ff-rdp
scoop install ff-rdp
```

### winget (Windows)

```powershell
winget install ractive.ff-rdp
```

### Cargo (from crates.io)

```sh
cargo install ff-rdp-cli
```

### Manual download

Download pre-built binaries from the [GitHub Releases](https://github.com/ractive/ff-rdp/releases) page. Binaries are available for Linux (x86_64, ARM64, glibc and musl), macOS (Apple Silicon), and Windows (x86_64, ARM64).

## First contact (for AI agents)

If anything goes wrong, run `ff-rdp doctor` first — it pinpoints connection,
port, and version issues in one shot. The probes are:

1. **Daemon registry** — is a daemon running and reachable?
2. **Port owner** — who is listening on `--port` (PID, process, uptime)?
3. **RDP handshake** — can we receive a Firefox greeting?
4. **Tabs** — how many tabs are exposed by the connected target?
5. **Firefox version compatibility** — within the tested range?

A typical first-time session looks like:

```bash
ff-rdp launch --headless --temp-profile   # start a fresh Firefox
ff-rdp doctor                             # confirm everything is healthy
ff-rdp navigate https://example.com       # do work
```

When `ff-rdp launch` finds the requested port already in use it now fails
loudly with the listener's PID and a hint pointing at `doctor`. Every
known-failure-mode error in ff-rdp ends with a `hint:` line that names the
next concrete command to run — connection-related ones name `doctor` first.

## Requirements

- Firefox with remote debugging enabled:
  ```bash
  firefox --start-debugger-server 6000
  ```
- Rust toolchain (for building from source)

## Build

```sh
cargo build --release
```

## Usage

Run `ff-rdp --help` for the full command surface and options.

Key global flags: `--host`, `--port`, `--tab`, `--jq`, `--timeout`, `--format text`, `--no-daemon`.

`--no-daemon`: connect directly to Firefox, bypassing the background daemon. The daemon (default) keeps a persistent Firefox connection and buffers events for streaming commands (`--follow`). Use `--no-daemon` for one-off commands or to debug daemon issues.

All output is JSON with a standard envelope (`results`, `total`, `meta`). Use `--jq` to filter:

```bash
# List tab URLs
ff-rdp tabs --jq '.results[].url'

# Navigate to a URL
ff-rdp navigate https://example.com

# Evaluate JavaScript and extract the result
ff-rdp eval 'document.title' --jq '.results'

# Eval from a file (avoids shell quoting issues with ?. or template literals)
ff-rdp eval --file script.js

# Eval from stdin
echo 'document.querySelectorAll("a").length' | ff-rdp eval --stdin

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

# Read console messages (errors only); output includes summary.total, summary.shown, summary.by_level
ff-rdp console --level error

# Filter console messages by pattern; limit to 20 results
ff-rdp console --pattern "TypeError" --limit 20

# Check how many messages matched vs how many were shown
ff-rdp console --limit 10 --jq '.summary'

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

# Query Performance API resource timing entries (default: --type resource)
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

# Full-page screenshot (captures entire scrollable document)
ff-rdp screenshot --full-page --output full.png

# Screenshot at explicit height
ff-rdp screenshot --viewport-height 2000 --output tall.png

# Get computed color for an element
ff-rdp computed h1 --prop color

# Get all non-default computed styles for a selector
ff-rdp computed .card

# Get the full resolved style object
ff-rdp computed button --all

# Launch Firefox with debugging enabled
ff-rdp launch

# Launch headless Firefox with temporary profile
ff-rdp launch --headless --temp-profile

# Launch with a specific profile and debug port
ff-rdp launch --profile /path/to/profile --debug-port 9222

# List temporary profiles managed by ff-rdp (path, count, total size)
ff-rdp profiles list

# Remove stale temporary profiles (default: older than 7 days)
ff-rdp profiles prune

# Preview what --all would remove, then remove everything
ff-rdp profiles prune --all --dry-run
ff-rdp profiles prune --all

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

# Reload and wait until network is idle (replaces sleep)
ff-rdp reload --wait-idle
ff-rdp reload --wait-idle --idle-ms 1000 --reload-timeout 30000
```

## Using ff-rdp from Claude Code

ff-rdp ships a Claude Code skill, **`ff-rdp-debug`**, that turns
ff-rdp into a symptom-routed debugger for web bugs. Install it once and
it's available in any repo on your machine:

```sh
ff-rdp install-skill --claude
# → installs the ff-rdp-debug skill to ~/.claude/skills/ff-rdp-debug/
# Skill is then available in any repo on your machine.
```

Inside Claude Code, trigger it with `/ff-rdp-debug` or natural-language
prompts like "debug this page", "login doesn't work", "why is X
failing in the browser". The skill routes the symptom to one of 10
deterministic playbooks (Set-Cookie strip, ChunkLoadError, React
controlled-input, consent banner, …) and runs probe commands against a
live Firefox tab. See `kb/skills/ff-rdp-debug.md` for the full skill
guide.

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

**Temporary profile cleanup:**
- `ff-rdp daemon stop` attempts to delete the temporary profile directory it
  launched Firefox with (never a directory passed via `--profile`). Cleanup
  runs only after the daemon has confirmed the stop; the stop JSON reports
  whether it happened via `profile_removed` / `profile_removed_path`.
- `ff-rdp launch` prunes orphaned `ff-rdp-profile-*` directories left behind
  by crashes or `kill -9`: entries older than `FF_RDP_PROFILE_PRUNE_DAYS`
  (default 7) are removed, at most `FF_RDP_PROFILE_PRUNE_MAX` (default 50)
  per launch. A directory only counts as stale when both its own mtime and
  its newest top-level file mtime are past the threshold — a profile that a
  long-running Firefox is still writing into is not treated as an orphan.
- Every managed profile carries an `.ff-rdp-owner-pid` marker (the launching
  Firefox's PID), written right after launch. Any age-gated prune — the
  automatic launch sweep and `profiles prune --older-than` — first checks
  whether that owner process is still alive and, if so, keeps the profile
  regardless of age. This is a positive "still in use" signal that closes the
  gap where a fully-idle-but-running Firefox could look stale by mtime alone.
- `ff-rdp profiles list` / `ff-rdp profiles prune` inspect and reclaim the
  profile directory explicitly; `ff-rdp doctor` warns when the profile store
  grows past 100 entries or 1 GiB. `profiles prune --all` skips the age gate
  entirely — do not run it while a Firefox launched by ff-rdp is still using
  one of these profiles. `--all` still removes a live-owner profile (it is the
  explicit escape hatch) but logs a warning per directory and lists each such
  basename under `removed_live` in the JSON output.

## Security

ff-rdp has the same power as Firefox DevTools — it can read httpOnly cookies, execute arbitrary JavaScript, capture screenshots, and navigate to URLs. The security model is "same as opening DevTools": the user is the operator.

**Transport:** Firefox RDP uses plaintext TCP with no TLS. By default ff-rdp connects to `localhost` only. For remote debugging, use SSH tunneling (`ssh -L 6000:localhost:6000 remote-host`) rather than exposing the debug port directly.

**URL validation:** The `navigate` command rejects `javascript:` and `data:` URLs by default to prevent accidental code execution in the page context. Allowed schemes are `http:`, `https:`, `file:`, and `about:`. Use `--allow-unsafe-urls` to bypass this check if needed.

**Daemon trust model:** The daemon listens on `127.0.0.1` (loopback only). Any local process can connect and send RDP commands through it — the same trust boundary as Firefox DevTools. The registry file (`~/.ff-rdp/daemon.json`) is created with owner-only permissions (0600 on Unix).

**Regex limits:** The `--pattern` flag (used by `console` and `sources` commands) applies a 1 MiB NFA size limit to prevent denial-of-service from pathological regular expressions.

**Not designed for untrusted networks.** Do not expose the Firefox debug port to the network. All RDP traffic (page content, cookies, eval results) is transmitted in plaintext.

## Architecture

- **ff-rdp-core** — Protocol library: blocking TCP transport, length-prefixed JSON framing, typed errors
- **ff-rdp-cli** — CLI binary: clap args, jq output pipeline, command dispatch, daemon proxy

## Releasing

1. Bump the version in `Cargo.toml`
2. Commit: `git commit -am "Bump version to X.Y.Z"`
3. Create a GitHub release with tag `vX.Y.Z` (must match `Cargo.toml`)

The [release workflow](.github/workflows/release.yml) automatically builds binaries for all platforms, publishes to crates.io, and updates Homebrew/Scoop/winget.

### Verifying release artifacts

Every release binary is signed via Sigstore-backed [build provenance attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds). To verify a downloaded artifact:

```sh
gh attestation verify ff-rdp-v0.2.0-aarch64-apple-darwin.tar.gz --owner ractive
```

The same command runs as a PR-time smoke check in [`ci.yml`](.github/workflows/ci.yml) (`verify-attestation` job), so a regression in the attestation pipeline is caught before release.

## Package repository hosting

[![OSS hosting by Cloudsmith](https://img.shields.io/badge/OSS%20hosting%20by-cloudsmith-blue?logo=cloudsmith&style=flat-square)](https://cloudsmith.com)

Package repository hosting is graciously provided by [Cloudsmith](https://cloudsmith.com).
Cloudsmith is the only fully hosted, cloud-native, universal package management solution, that
enables your organization to create, store and share packages in any format, to any place, with total
confidence.

## License

MIT
