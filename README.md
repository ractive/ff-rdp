# ff-rdp

[![crates.io](https://img.shields.io/crates/v/ff-rdp-cli.svg)](https://crates.io/crates/ff-rdp-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A fast Rust CLI for the Firefox Remote Debugging Protocol. Communicates directly over TCP with Firefox's built-in debugger for minimal latency.

## Installation

### Homebrew (macOS & Linux)

```sh
brew trust --formula ractive/tap/ff-rdp   # Homebrew 6+: third-party taps need one-time trust
brew install ractive/tap/ff-rdp
```

Covers macOS (Apple Silicon) and Linux (x86_64 and ARM64). The Linux bottles are statically linked (musl), so they run on any glibc or musl distribution.

Homebrew 6 introduced [tap trust](https://docs.brew.sh/Tap-Trust): formulae
from third-party taps refuse to load until trusted. `brew trust --formula`
scopes the trust to just this formula; `brew trust ractive/tap` trusts the
whole tap instead.

### apt (Debian / Ubuntu)

```sh
curl -sLf 'https://dl.cloudsmith.io/public/ractive/ff-rdp/cfg/setup/bash.deb.sh' | sudo bash
sudo apt install ff-rdp
```

This one-time setup script registers the [Cloudsmith](https://cloudsmith.io/~ractive/repos/ff-rdp/packages/)-hosted apt repository; afterwards `apt upgrade` picks up new releases automatically.

### dnf / yum / zypper (Fedora / RHEL / openSUSE)

```sh
curl -sLf 'https://dl.cloudsmith.io/public/ractive/ff-rdp/cfg/setup/bash.rpm.sh' | sudo bash
sudo dnf install ff-rdp   # or: yum install ff-rdp / zypper install ff-rdp
```

### Scoop (Windows)

```powershell
scoop bucket add ractive https://github.com/ractive/scoop-bucket
scoop install ff-rdp
```

### winget (Windows)

```powershell
winget install ractive.ff-rdp
```

### Cargo (from crates.io)

```sh
cargo install ff-rdp-cli   # installs the `ff-rdp` binary
```

### Manual download

Download pre-built binaries from the [GitHub Releases](https://github.com/ractive/ff-rdp/releases) page. Archives are named `ff-rdp-v<version>-<target>.{tar.gz,zip}` and bundle the binary, `LICENSE`, `README`, and shell completions. Targets:

- **Linux** — x86_64 (glibc and static musl), ARM64 (static musl)
- **macOS** — Apple Silicon (aarch64)
- **Windows** — x86_64 and ARM64

Standalone `.deb` and `.rpm` packages (x86_64) are attached to each release as well; they install `ff-rdp` plus bash/zsh/fish completions system-wide.

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

When `ff-rdp launch` finds the requested port already in use it fails
immediately — before spawning Firefox — naming the occupying process and its
PID, and hinting at `--debug-port`, `--replace` and `doctor`. That is a
*different* failure from Firefox spawning fine but not opening its debug port
in time, which reports `Firefox (pid N) did not open debug port P within Ss`.
Keeping the two apart matters: pre-iter-158 both printed "is the port already
in use?", which sent users hunting for a process that did not exist.

`launch` waits **30 s** by default for that port to open. Raise it with
`--launch-timeout <secs>` or `FF_RDP_LAUNCH_TIMEOUT_SECS` (the flag wins; a
malformed env value falls back to 30 s rather than failing the launch). The
effective bound is reported as `meta.launch_wait_secs`. This is deliberately
not the global `--timeout`, which is a per-socket-operation deadline. The
previous hardcoded 5 s bound failed 5/5 launches at load average 6.8 —
Firefox was measured binding its debug port at 7 s under contention.

Every known-failure-mode error in ff-rdp ends with a `hint:` line that names
the next concrete command to run — connection-related ones name `doctor`
first.

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

# The capture source is always the one you asked for. The default is the RDP
# resource watcher — the only source that carries method, status, content_type
# and transfer_size. `meta.source` names where the rows came from; an empty
# watcher buffer reports zero watcher rows rather than silently answering from
# somewhere else (iter-159 removed that substitution).
ff-rdp network                          # watcher (default)
ff-rdp network --source performance-api # explicit opt-out: fewer fields

# Navigate and capture all network traffic in one shot
ff-rdp navigate https://example.com --with-network

# Dismiss a cookie banner and capture the network in the same call — the
# consent click happens inside the capture window.
ff-rdp navigate https://www.theguardian.com --with-network --auto-consent

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

# Plain sleep, no condition or Firefox connection needed (--time is a legacy alias)
ff-rdp wait --sleep-ms 2000

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
# (a --profile directory that does not exist yet is created)
ff-rdp launch --profile /path/to/profile --debug-port 9222

# Allow longer for the debug port to open on a heavily loaded machine
ff-rdp launch --headless --launch-timeout 60

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

# Generate a shell completion script (bash, zsh, fish, elvish, powershell)
ff-rdp completions zsh > ~/.zsh/completions/_ff-rdp
eval "$(ff-rdp completions zsh)"          # or load straight into the current shell
```

The `.deb`/`.rpm` packages already install bash/zsh/fish completions system-wide, so `completions` is mainly for other shells or for refreshing the script after an upgrade.

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
- The daemon transparently proxies RDP frames and also exposes a `"daemon"` virtual actor for draining buffered events, for the recorded frame-target snapshot (`click --frame`, `consent accept`), and for status
- Firefox RDP replies carry no request id, so the daemon lets **one** client have Firefox-bound requests in flight at a time. Concurrent invocations **queue** for that channel and all succeed; a client that waits out the queue budget gets a `daemon_busy` error naming the wait and the cap. Use `--no-daemon` for a private connection when you want true parallelism.
- Daemon exits automatically after 5 minutes of inactivity (configurable via `--daemon-timeout`)
- The watcher resource subscriptions (`network-event`, `console-message`, `error-message`) belong to the **daemon**, not to any one CLI invocation. The daemon therefore drops a proxied client's `unwatchResources` for those types (it already does the same for `unwatchTargets`) — otherwise one command's teardown would destroy Firefox's `NetworkObserver` on the shared connection, taking the session's URL block-list and throttling config with it (iter-164)
- Auto-start waits up to **20 s** for the freshly spawned daemon to register; override with `FF_RDP_DAEMON_START_TIMEOUT_MS`. If it gives up, the command still runs over a direct connection — `meta.route` reports `"direct"` and, under `--verbose`, `meta.daemon_fallback` names why daemon mode degraded instead of silently discarding the reason (iter-164)

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
- Every managed profile carries an `.ff-rdp-owner-pid` marker. It is written
  the instant the profile directory is created, holding the launching **ff-rdp
  process's own** PID, and rewritten with the **Firefox** PID as soon as the
  browser is spawned (iter-175). Before that, the marker was written only after
  the spawn, so a launch that died in between — a failed spawn, a browser that
  exited immediately, a killed CLI — left a directory nothing could attribute
  and only the seven-day mtime gate could ever reclaim. Any age-gated prune —
  the automatic launch sweep and `profiles prune --older-than` — first checks
  whether that owner process is still alive and, if so, keeps the profile
  regardless of age. This is a positive "still in use" signal that closes the
  gap where a fully-idle-but-running Firefox could look stale by mtime alone.
- A marker naming a **dead** PID is reclaimed by the very next `launch`
  immediately, without waiting out the age threshold — a dead owner is
  definitive proof of abandonment, not just "old enough to guess at" (iter-142;
  fixes an observed 62 profiles / 2.7 GB accumulating in a single day, all
  younger than the old 7-day gate).
- The marker is a **pair**: alongside the PID, `.ff-rdp-owner-start` records
  that process's OS start time, and both are written together — the old token
  is cleared first, so the pair can never describe two different processes. A
  profile directory outlives the Firefox that owned it, so its PID marker does
  too — and once the OS recycles that PID, a bare liveness
  check reports the abandoned profile as still in use, so no age-gated prune
  will ever reclaim it. Comparing the recorded start time against the live
  PID's makes the check an *identity* check: a recycled PID no longer passes
  for the original owner (iter-171). A profile written by an older ff-rdp has
  no start marker and keeps the previous PID-only behaviour.
- A `launch` that fails **after** creating its profile directory removes that
  directory on the way out (iter-175) — spawn failure, a Firefox that exits
  immediately, a debug port that never opens, or a `--auto-consent` extension
  install that fails. A directory the user passed via `--profile` is never
  touched by this.
- Directories left by a *pre-iter-175* failed launch are reclaimed too: a
  managed directory with no owner marker at all, holding nothing but the
  `user.js` ff-rdp writes before the spawn, is proof that no Firefox ever
  opened it, so the next `launch` removes it after a ten-minute race grace
  instead of after seven days. Age gating is unchanged for every other
  directory — a profile Firefox actually used still keeps the full
  `FF_RDP_PROFILE_PRUNE_DAYS` threshold, and `profiles prune --older-than`
  stays a pure age query.
- Every `ff-rdp launch` also sweeps `~/.ff-rdp/` housekeeping files: stale
  per-port spawn locks, the per-port registry write locks (iter-172), the
  legacy port-less `daemon.spawn.lock` name,
  `daemon.<port>.throttle.json` state files whose recorded daemon PID is no
  longer alive (iter-142), and `launch-record.<port>.json` files whose
  recorded PID is no longer alive (iter-186). Previously this only ran on the
  rare daemon-autostart path, so a session that reused an already-running
  daemon never triggered it at all.
- Launch records needed their own sweep because nothing else reclaimed them.
  `daemon stop` deletes the record only on a **clean** stop, and reading a
  record with a dead PID deletes it only when *that same port* is read again —
  and ports come from an ephemeral `bind(:0)`, so that port essentially never
  recurs. Measured on one dev machine: 4803 records / 20 MB accumulated over
  ten days; the first launch carrying the iter-186 sweep took that to 14
  files / 1.0 MB, and three further launches left it at 13, 13, 13. A record
  whose PID is still alive is never removed, so a running instance's record
  survives a sweep that happens while it is up.
- The daemon registry `daemon.<port>.json` is only ever published by an atomic
  `rename`, and the lock that serializes writers lives in a **sibling**
  `daemon.<port>.write.lock` (iter-172). Locking the published path itself —
  which is what earlier builds did — meant a zero-byte record existed for the
  whole span between taking the lock and the rename, and a client that read it
  in that window gave up on the daemon and silently ran the command over a
  direct connection instead. A zero-byte record left behind by such a build is
  now read as "no daemon registered" rather than as a parse error, so the next
  invocation starts a daemon normally instead of degrading forever.
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

Releases run through the shared [`ractive/release-workflows`](https://github.com/ractive/release-workflows) pipeline. The repo-local [`release.yml`](.github/workflows/release.yml) is a thin caller that pins the shared workflow version and passes the ff-rdp-specific inputs (targets, package names, winget identifier, Cloudsmith repo, completions packaging).

1. Bump the workspace version in `Cargo.toml` (`[workspace.package] version`) and commit.
2. Cut the release: `gh release create vX.Y.Z --generate-notes` (the tag must match the workspace version).

On a published release the shared pipeline builds every target, publishes `ff-rdp-core` and `ff-rdp-cli` to crates.io, updates Homebrew / Scoop / winget, uploads the versioned archives, SBOMs, and `.deb`/`.rpm` packages to the GitHub release, and pushes the `.deb`/`.rpm` to the hosted Cloudsmith apt/rpm repos.

**Dry-run rehearsal:** `gh workflow run release.yml` (a `workflow_dispatch`) builds, tests, and packages everything as workflow artifacts without touching any external channel — use it to validate a release before tagging.

**Recovery workflows** (both `workflow_dispatch`):

- [`cloudsmith-republish.yml`](.github/workflows/cloudsmith-republish.yml) — re-push an existing release's `.deb`/`.rpm` to Cloudsmith (the release-time Cloudsmith step is non-blocking).
- The shared repo's `publish-crates.yml` re-publishes a crate to crates.io if that step failed during a release.

[`live.yml`](.github/workflows/live.yml) runs the Firefox-dependent live tests on each release and weekly as a drift canary (not per-PR).

[`toolchain-watch.yml`](.github/workflows/toolchain-watch.yml) runs `cargo fmt --check` and `cargo clippy -D warnings` against `main` weekly (and on `workflow_dispatch`). `ci.yml` lints on `pull_request` only, so a new stable release can red-line `main` with no commit pushed and nothing would notice until an unrelated PR absorbed the cost — which is exactly what happened when 1.98.0 shipped. See `kb/decision-log.md` DEC-044.

### Verifying release artifacts

Every release binary is signed via Sigstore-backed [build provenance attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds). To verify a downloaded artifact:

```sh
gh attestation verify ff-rdp-v0.3.0-aarch64-apple-darwin.tar.gz --owner ractive
```

Each native target also ships a CycloneDX SBOM for both `ff-rdp-cli` and `ff-rdp-core` (the `*.cdx.json` files on the release).

The same verification command runs as a PR-time smoke check in [`ci.yml`](.github/workflows/ci.yml) (`verify-attestation` job), so a regression in the attestation pipeline is caught before release.

## Package repository hosting

[![OSS hosting by Cloudsmith](https://img.shields.io/badge/OSS%20hosting%20by-cloudsmith-blue?logo=cloudsmith&style=flat-square)](https://cloudsmith.com)

Package repository hosting is graciously provided by [Cloudsmith](https://cloudsmith.com).
Cloudsmith is the only fully hosted, cloud-native, universal package management solution, that
enables your organization to create, store and share packages in any format, to any place, with total
confidence.

## License

MIT
