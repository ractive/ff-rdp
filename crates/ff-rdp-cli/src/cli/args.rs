use clap::{ArgGroup, Parser, Subcommand, ValueEnum};
use regex::Regex;

const AFTER_LONG_HELP: &str = "\
EXIT CODES:
  0    Success
  1    Runtime error (command failed, Firefox returned an error, etc.)
  2    Usage error (bad arguments, unknown flag, etc.)
  3    Connection failure (Firefox not running or unreachable)
  124  Timeout (operation exceeded --timeout)

COMMAND REFERENCE:
  Launch & connect:
    ff-rdp launch [--headless] [--profile PATH | --temp-profile] [--auto-consent] [--port PORT]
    ff-rdp doctor                  # diagnose connection, port, tabs, version
    ff-rdp tabs

  Navigate & wait:
    ff-rdp navigate <URL> [--with-page] [--with-network] [--wait-text T | --wait-selector S] [--wait-timeout MS]
    ff-rdp reload [--with-page] [--wait-idle [--idle-ms MS] [--reload-timeout MS]]
    ff-rdp back | forward [--with-page]
    ff-rdp wait --selector S | --text T | --eval JS [--wait-timeout MS]

  Page content:
    ff-rdp eval <SCRIPT> | --file PATH | --stdin [--stringify] [--no-isolate]
    ff-rdp page-text [--query TEXT [--context N]] [--max-chars N | --full]
    ff-rdp dom <SEL> [--text | --attrs | --text-attrs | --inner-html | --count] [--query TEXT]
    ff-rdp dom stats
    ff-rdp dom tree [SEL] [--depth N] [--max-chars N]
    ff-rdp snapshot [--depth N] [--max-chars N] [--query TEXT]

  Find, don't guess (iter-211):
    --query TEXT (case-insensitive substring) or --query-regex PATTERN on
    page-text, snapshot, `a11y summary` and dom returns only the matching part
    of the page, with meta.matches counting the hits. page-text is capped at
    8000 characters by default — meta.total_chars always reports the full
    length, --full lifts the cap. Reach for --query instead of piping
    page-text through `head`: the answer is usually further down.

  Interaction:
    ff-rdp click <SEL> | --ref <REF> [--with-page] [--dispatch pointer|legacy|click-only] [--no-wait] [--settle]
    ff-rdp click <SEL> --wait-for-network <pattern> [--network-timeout MS]
    ff-rdp click <SEL> --wait-for selector:<css> --wait-for text:<substr>
    ff-rdp type <SEL> <TEXT> [--submit] [--with-page] [--clear] [--no-wait] [--settle] [--wait-for ...]

  Act and see (iter-210):
    --with-page on navigate/click/type/reload/back/forward/scroll returns the
    resulting page under results.page — headings, landmarks, and interactive
    elements carrying `ref` handles for `click --ref` / `type --ref`. Same view
    and shape as `ff-rdp a11y summary`, which (with `snapshot`) now registers
    refs too. Collected after the action settles, so a click that navigates
    reports the destination page.

  Scrolling:
    ff-rdp scroll to <SEL> [--block top|center|bottom] [--smooth] [--no-wait] [--settle]
    ff-rdp scroll by [--dy PX | --page-down | --page-up] [--dx PX] [--smooth]
    ff-rdp scroll top | bottom
    ff-rdp scroll container <SEL> [--dy PX] [--to-end | --to-start]
    ff-rdp scroll until <SEL> [--direction up|down] [--timeout MS]
    ff-rdp scroll text <TEXT>

  CSS & styles:
    ff-rdp computed <SEL> [--prop NAME | --all]
    ff-rdp styles <SEL> [--properties P1,P2 | --applied | --layout]
    ff-rdp cascade <SEL> [--prop NAME | --all]    # explain which rule wins
    ff-rdp geometry <SEL>... [--include-hidden]
    ff-rdp responsive <SEL>... [--widths W1,W2,...]

  Page-environment emulation:
    ff-rdp emulate [--color-scheme light|dark|none] [--user-agent S] [--dppx F]
                   [--print on|off] [--touch on|off] [--js on|off]
                   [--offline on|off] [--cache on|off] [--reset]

  PWA & manifest:
    ff-rdp manifest                         # parsed Web App Manifest + conformance errors

  Accessibility:
    ff-rdp a11y [--depth N] [--selector SEL] [--interactive] [--critical]
    ff-rdp a11y --native  # opt in to Firefox's platform accessibility tree (meta.source tells you which tree you got)
    ff-rdp a11y contrast [--selector SEL] [--fail-only]  # total=results, sampled=elements checked
    ff-rdp a11y summary

  Performance:
    ff-rdp perf [--type TYPE] [--filter URL] [--group-by domain]
    ff-rdp perf vitals | summary | audit
    ff-rdp perf compare <URL>... [--label L1,L2,...]

  Monitoring:
    ff-rdp console [--level LEVEL] [--pattern REGEX] [--follow]
    ff-rdp network [--filter URL] [--method M] [--follow]
    ff-rdp network --detail [--headers]    # include request+response headers per entry
    ff-rdp network --security              # per-request TLS/cert detail + insecure_requests count

  Storage:
    ff-rdp cookies [--name NAME]
    ff-rdp storage local|session [--key KEY]

  Screenshot & debug:
    ff-rdp screenshot [-o PATH | --base64] [--full-page | --viewport-height PX]
    ff-rdp inspect <ACTOR_ID> [--depth N]
    ff-rdp sources [--filter URL | --pattern REGEX]

  Skills (Claude Code):
    ff-rdp install-skill --claude [--user | --project] [<skill-name>]
    ff-rdp install-skill --claude --dry-run
    ff-rdp install-skill --claude --list
    ff-rdp install-skill --claude --uninstall <name>
    ff-rdp install-skill --claude --from-dir <path> [<name>]

  Profile maintenance:
    ff-rdp profiles list
    ff-rdp profiles prune [--older-than 7d | --all] [--dry-run]

AI AGENT TIPS:
  - Use --format text instead of JSON for 3-10x fewer tokens
  - Use eval --stringify '<script>' to get actual values instead of actor grip
    metadata; it accepts multi-statement scripts, exactly like bare eval (iter-161)
  - Use styles --properties color,display,font-size (bare styles dumps ~500 properties)
  - Use a11y summary for a flat list instead of the full tree (can be 400+ lines)
  - Use snapshot --depth 3 for a quick page overview
  - Use dom \"sel\" --text-attrs to get both text content and attributes together
  - Follow the contextual hints (-> lines) for suggested next commands

COOKBOOK:
  # Launch Firefox (safe alongside your normal browser)
  ff-rdp launch
  ff-rdp launch --headless
  ff-rdp launch --headless --auto-consent

  # Navigate and verify
  ff-rdp navigate https://example.com --wait-text \"Welcome\"
  ff-rdp eval \"document.title\"
  ff-rdp dom \"h1\" --text

  # Fill and submit a form (auto-wait + pointer events by default)
  ff-rdp type \"input[name=email]\" \"user@example.com\" --clear
  ff-rdp type \"input[name=password]\" \"secret\" --clear
  ff-rdp click \"button[type=submit]\" --wait-for text:Dashboard
  ff-rdp click --selector \"button[type=submit]\"          # flag alias
  ff-rdp click \"button[type=submit]\" --wait-for-network \"/api/login\"
  ff-rdp click \"button[aria-haspopup]\" --dispatch pointer  # Radix/Headless-UI dropdowns
  ff-rdp click \"button\" --no-wait                          # pre-iter-59 fire-and-forget

  # Full page audit
  ff-rdp navigate https://example.com --with-network
  ff-rdp perf audit
  ff-rdp a11y contrast --fail-only
  ff-rdp network --detail --limit 10
  ff-rdp screenshot -o audit.png

  # Performance
  ff-rdp perf vitals --jq '.results.lcp_ms'
  ff-rdp perf --all --jq '[.results | sort_by(-.duration_ms) | limit(5;.) | {url,duration_ms}]'
  ff-rdp perf compare https://a.example https://b.example --label \"Before,After\"

  # Network debugging
  ff-rdp network --detail --jq '[.results[] | select(.status >= 400) | {url,status}]'
  ff-rdp network --follow --filter \".js\"

  # Console monitoring
  ff-rdp console --level error --jq '.results[].message'
  ff-rdp console --follow --level error

  # Scrolling (overflow containers, lazy-loaded content)
  ff-rdp scroll by --page-down
  ff-rdp scroll container \".sidebar\" --to-end
  ff-rdp scroll until \".load-more-sentinel\" --timeout 10000
  ff-rdp scroll text \"Contact Us\"

  # Accessibility
  ff-rdp a11y summary --format text
  ff-rdp a11y contrast --fail-only    # total = AA failures; sampled = elements checked
  ff-rdp a11y contrast --fail-only --jq '{total, shown: (.results|length), sampled}'
  ff-rdp a11y --interactive --jq '[.. | select(.role? == \"link\") | .name]'
  ff-rdp a11y --jq '.meta.source'     # \"native\" or \"js-fallback\" — always present
  ff-rdp a11y --native --jq '.result.role'  # opt in to the real platform tree ([\"document\", ...])

  # DOM and CSS inspection
  ff-rdp dom \"a[href]\" --text-attrs
  ff-rdp dom stats --jq '.results.node_count'
  ff-rdp computed h1 --prop color
  ff-rdp styles \"h1\" --properties color,display,font-size
  ff-rdp geometry \".modal\" \".overlay\" --jq '.results.overlaps'

  # Responsive testing
  ff-rdp responsive \"h1\" \"nav\" \".sidebar\" --widths 320,768,1440

  # Screenshot for AI vision
  ff-rdp screenshot --base64

  # Install the ff-rdp-debug Claude Code skill
  ff-rdp install-skill --claude
  ff-rdp install-skill --claude --project
  ff-rdp install-skill --claude --dry-run
  ff-rdp install-skill --claude --list

OUTPUT FORMAT (iter-60 compact defaults):
  Default JSON: {\"results\": ..., \"total\": N}  (meta omitted when empty)
  --verbose restores meta.connection (host, port, pid, uptime) to the envelope
  meta.route (iter-128, all commands since iter-134) is always present on
    every browser-touching command's envelope — \"daemon\" or \"direct\" —
    regardless of --verbose, so you can tell how a command executed without a
    separate `daemon status` call
  Truncated output adds: {\"truncated\": true, \"hint\": \"showing 20 of 84, use --all\"}
  --format json  (default) machine-readable JSON — the stable API contract
  --format text  human-readable tables and trees
  --format html  raw HTML passthrough (dom and snapshot only — pre-iter-60 shape)
  --jq can be combined with --format text: jq runs first, text rendering applies
  Use --jq to filter the envelope: --jq '.results[0]', --jq '.total'
    --jq is a VIEW: it never changes the envelope's shape. `network` was the one
    exception until iter-160 (--jq silently switched results object -> array);
    pass --detail there, as on every other list command.
  Use --detail for per-entry output on list commands (default is summary view)
  Contextual hints suggest follow-up commands: \"hints\": [...] in JSON, -> lines in text
  Hints default: on for --format text, off for JSON. Override: --hints / --no-hints
  --jq always suppresses hints (pipeline needs clean data)

  dom default output: ARIA-tree JSON {ref, role, name, level, state, tag, attrs}
  dom --format html: legacy raw HTML strings (escape hatch for HTML diffing)

TROUBLESHOOTING:
  When stuck, run `ff-rdp doctor` first — it probes daemon, port owner,
  RDP handshake, tab count, and Firefox version in one command.

  Common failure modes:
    \"port N is already in use\"      -> ff-rdp doctor   # who is on the port
    \"no tabs available\"             -> ff-rdp doctor   # is Firefox even talking
    \"could not connect to Firefox\"   -> ff-rdp doctor   # is the listener up
    \"actor error from server1...\"    -> ff-rdp doctor   # stale connection?
    Connection timeout / hang        -> ff-rdp doctor   # then increase --timeout

  Zero results:
    network returns 0 -> page loaded before connection; use navigate --with-network
    console returns 0 -> use --follow to stream, or eval 'console.log(\"test\")'
    cookies returns 0 -> consent banner may be blocking; use launch --auto-consent

  Connection errors:
    \"could not connect\" -> run ff-rdp launch first (safe alongside normal browser)
    Timeout -> increase --timeout or check --port matches the launched instance";

/// Default value of the global `--timeout` flag, in milliseconds.
///
/// Named (rather than inlined in the `#[arg]` attribute) because the error
/// path needs the same number: `ProtocolError::Timeout` carries no duration,
/// so `AppError::from` reports the socket read deadline instead — see
/// `crate::error::socket_timeout_ms` (iter-137 Theme B).
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// Log verbosity level for `--log-level`.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Build the version string displayed by `ff-rdp --version`.
///
/// When the binary was built from a git checkout, includes the short sha and
/// commit date: `0.3.0 (abc123def456 2026-05-26)`.  When `+dirty` is appended
/// to the sha it means the working tree had uncommitted changes at build time.
///
/// When the sha is empty (crates.io tarball, offline build, or
/// `CARGO_FF_RDP_FORCE_NO_GIT=1`), returns the bare `CARGO_PKG_VERSION`.
pub fn build_version_string() -> &'static str {
    const SHA: &str = env!("FF_RDP_BUILD_VERSION_SHA");
    const DATE: &str = env!("FF_RDP_BUILD_DATE");
    const PKG: &str = env!("CARGO_PKG_VERSION");

    if SHA.is_empty() {
        PKG
    } else {
        // SAFETY: `concat!` on static strings produces a `'static str`.
        // We use a `Box::leak` to build the string once at first call.
        // This is intentional: version strings are compared/printed rarely.
        use std::sync::OnceLock;
        static VERSION: OnceLock<String> = OnceLock::new();
        VERSION
            .get_or_init(|| format!("{PKG} ({SHA} {DATE})"))
            .as_str()
    }
}

#[derive(Parser)]
#[command(
    name = "ff-rdp",
    about = "Firefox Remote Debugging Protocol CLI\n\nCommand groups (see `ff-rdp <cmd> --help` for details):\n  Inspect    dom, styles, computed, cascade, a11y, snapshot, page-text, perf\n  Navigate   navigate, reload, click, type, screenshot\n  Trace      console, network, eval\n  Lifecycle  launch, daemon\n\nQuick start:  ff-rdp launch          # start Firefox with debugging enabled\n              ff-rdp navigate <URL>   # open a page",
    long_about = "Firefox Remote Debugging Protocol CLI

Command groups (use `ff-rdp <cmd> --help` for details on any command):
  Inspect    dom, styles, computed, cascade, a11y, snapshot, page-text, perf
  Navigate   navigate, reload, click, type, screenshot
  Trace      console, network, eval
  Lifecycle  launch, daemon

Quick start:
  ff-rdp launch                   Launch a new Firefox instance with remote debugging
  ff-rdp launch --headless        Launch headless (no visible window)
  ff-rdp navigate https://example.com

'ff-rdp launch' starts a separate Firefox process that won't interfere with
any already-running Firefox windows — it uses a temporary profile and
the -no-remote flag automatically.",
    after_help = "Tip: Run 'ff-rdp launch' first to start Firefox with remote debugging.\n     It won't affect any existing Firefox windows — safe to run alongside\n     your normal browser.",
    after_long_help = AFTER_LONG_HELP,
    version = build_version_string()
)]
pub struct Cli {
    /// Firefox debug server host
    #[arg(long, default_value = "localhost", global = true)]
    pub host: String,

    /// Firefox debug server port
    #[arg(long, default_value_t = 6000, global = true)]
    pub port: u16,

    /// Target tab by index (1-based) or URL substring
    #[arg(long, global = true)]
    pub tab: Option<String>,

    /// Target tab by exact actor ID
    #[arg(long, global = true)]
    pub tab_id: Option<String>,

    /// jq filter expression applied to output
    #[arg(long, global = true)]
    pub jq: Option<String>,

    /// When using --jq, treat a missing path (null result) as an error: exits non-zero
    /// with "error: jq path '<path>' not found in input" on stderr.
    /// By default missing paths produce no output (silent omit).
    #[arg(long, global = true, requires = "jq")]
    pub jq_strict: bool,

    /// Operation timeout in milliseconds
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_MS, global = true)]
    pub timeout: u64,

    /// Connect directly to Firefox, bypassing the daemon. Use for one-off commands or fresh connections. The daemon (default) keeps a persistent connection and buffers events for streaming commands (--follow).
    #[arg(long, global = true)]
    pub no_daemon: bool,

    /// Daemon idle timeout in seconds
    #[arg(long, default_value_t = 300, global = true)]
    pub daemon_timeout: u64,

    /// Allow javascript: and data: URL schemes in navigate (unsafe)
    #[arg(long, global = true)]
    pub allow_unsafe_urls: bool,

    /// Allow file:// URL schemes for `navigate` and `perf compare` (off by
    /// default; local files become exfiltratable via subsequent page-text /
    /// eval / screenshot). Independent of --allow-unsafe-urls — that flag
    /// only opens javascript:/data:, not file:.
    #[arg(long, global = true)]
    pub allow_file_urls: bool,

    /// Limit number of results returned (per-command defaults apply)
    #[arg(long, global = true)]
    pub limit: Option<usize>,

    /// Return all results, overriding any default limit
    #[arg(long, global = true, conflicts_with = "limit")]
    pub all: bool,

    /// Sort results by field name (a name present on no result entry is an
    /// error, not a silent no-op)
    #[arg(long, global = true)]
    pub sort: Option<String>,

    /// Sort ascending (default is per-command)
    #[arg(long, global = true, conflicts_with = "desc")]
    pub asc: bool,

    /// Sort descending (default is per-command)
    #[arg(long, global = true, conflicts_with = "asc")]
    pub desc: bool,

    /// Comma-separated list of fields to include in each result entry (a name
    /// present on no result entry is an error, not silently dropped)
    #[arg(long, global = true, value_delimiter = ',')]
    pub fields: Option<Vec<String>>,

    /// Show detailed individual entries instead of summary mode
    #[arg(long, global = true)]
    pub detail: bool,

    /// Output format: "json" (default) or "text" for human-readable tables
    #[arg(long, default_value = "json", global = true)]
    pub format: String,

    /// Show contextual hints suggesting follow-up commands (default: on for text, off for json)
    #[arg(long, global = true, conflicts_with = "no_hints")]
    pub hints: bool,

    /// Suppress contextual hints
    #[arg(long, global = true, conflicts_with = "hints")]
    pub no_hints: bool,

    /// Restore full meta.connection envelope (host, port, pid, uptime) in JSON output.
    /// Also enables internal debug messages (fallback paths, protocol quirks) to stderr.
    /// Also enabled when the RUST_LOG environment variable is set.
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Set the log level for structured tracing output to stderr.
    ///
    /// "trace" enables per-packet wire dumps (ff_rdp_core::transport=trace).
    /// Set FF_RDP_TRACE_RAW=1 to disable redaction of sensitive fields in trace output.
    /// Overrides the RUST_LOG environment variable when specified.
    #[arg(long, global = true, value_name = "LEVEL")]
    pub log_level: Option<LogLevel>,

    /// Maximum Firefox RDP frame payload size in mebibytes (1 MiB = 1024 × 1024 B).
    ///
    /// Default is 256 MiB, which accommodates heap-snapshot dumps and large
    /// network response bodies.  Lower to harden against malformed peers;
    /// raise to receive larger legitimate frames.  Applied once at startup.
    /// Must be ≥ 1 — `0` is rejected (see `validate()` on this struct) so
    /// the OOM guard can't be silently disabled by an operator typo
    /// (`set_max_frame_bytes(0)` resets to the default rather than
    /// rejecting all frames).
    #[arg(long, global = true, value_name = "MB", default_value_t = 256)]
    pub max_frame_mb: usize,

    /// Threshold in bytes above which un-keyed string values in trace output
    /// are replaced with `<redacted len=N>`.
    ///
    /// Sensitive-keyed values (cookie, authorization, set-cookie, password,
    /// auth-token, x-auth-token, text, expression) are always redacted
    /// regardless of this setting.  Default 256.  Must be ≥ 1 — `0` is
    /// rejected because `set_redact_threshold(0)` resets to the default
    /// rather than redacting every string.
    #[arg(long, global = true, value_name = "BYTES", default_value_t = 256)]
    pub redact_threshold: usize,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Returns `true` when internal debug messages should be printed to stderr.
    ///
    /// Enabled by `--verbose`, `--log-level`, or by having `RUST_LOG` set
    /// (the latter implies that the caller already opted into structured logging output).
    pub fn is_verbose(&self) -> bool {
        self.verbose || self.log_level.is_some() || std::env::var("RUST_LOG").is_ok()
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// List open browser tabs
    #[command(long_about = "List open browser tabs.

The Consent-O-Matic options tab `launch --auto-consent` leaves open
(`moz-extension://.../options.html`, title \"Consent-O-Matic Options\" or a
known localized equivalent) is filtered out of the listing (iter-144) — it
is never counted in `total` or in `--tab N` indices.

Output: {\"results\": [{\"url\": \"...\", \"title\": \"...\", \"actor\": \"...\", \"selected\": true}], \"total\": N, \"meta\": {...}}")]
    Tabs,
    /// Navigate to a URL
    #[command(long_about = "Navigate to a URL.

By default, navigate blocks until the new document is committed (URL changes and
readyState reaches 'interactive' or 'complete'), or the --timeout budget expires.
The result includes 'committed_url', 'ready_state', 'elapsed_ms', and 'status' so
agents can confirm what page actually loaded — including the main document's real
HTTP status (iter-138): a 404 or 503 page still commits successfully (readyState
reaches 'complete'), so 'status' is the only reliable way to detect it without a
follow-up 'network' call. 'status' is always present, and `null` when there is no
HTTP status to report — in which case 'status_reason' (also always present, and
`null` exactly when 'status' is not) says which kind of `null` it is:
  not_observed        this route never subscribed to network events, so no status
                      could have been seen: --no-wait, or `back`/`forward`/`reload`
  no_document_request the document committed without issuing a request of its own —
                      about:blank, a bfcache restore, a same-document navigation
  no_status_reported  the document's request was identified but Firefox never
                      reported a status for it (response line not in yet, or the
                      channel failed)
Before iter-166 'status' was `null` for ordinary pages that plainly returned 200:
the main document was matched by an exact string comparison against the URL as
typed, and Firefox requests the canonical form ('https://example.com' becomes
'https://example.com/'), so nothing ever matched. On a redirect, 'status' is the
status of the document that COMMITTED, not of the redirect hop.

Same-document navigations — SPA `history.pushState`/`popstate` traversal (via
`back`/`forward`), and same-page fragment navigation (`#frag`) — never fire the
usual document-commit signal Firefox uses for a full page load; this is detected
and resolved directly rather than waiting out the full --timeout (iter-138).

Use --no-wait to restore the old fire-and-forget behaviour (returns immediately after
the navigate request is acknowledged, without waiting for the document to commit).

The URL is a positional argument (not a flag). There is no --url option.

Examples:
  ff-rdp navigate https://example.com
  ff-rdp navigate https://example.com --with-network
  ff-rdp navigate https://example.com --wait-text \"Welcome\"
  ff-rdp navigate https://example.com --wait-for selector:.athing
  ff-rdp navigate https://example.com --no-wait
  ff-rdp navigate https://www.theguardian.com --auto-consent

--auto-consent (iter-129): after the document commits, run the same
CMP-detection-and-accept flow as `ff-rdp consent accept` and add
`results.consent = {\"cmp\": \"sourcepoint\"|\"bbc\"|null, \"action\": \"accepted\"|null,
\"status\": \"accepted\"|\"detected_not_actioned\"|\"no_cmp_detected\"}` (all three keys
always present, never omitted; `status` added in iter-160). Combinable with
--with-network since iter-159, and the same three keys appear there.
Best-effort — a detection failure prints a warning but does not fail the
navigate, and unlike `consent accept` the navigate's exit code is UNCHANGED by
the consent outcome: a page with no cookie banner is not a failed navigation.
This is the CLI-native complement to `launch --auto-consent` (the
Consent-O-Matic extension), which does not reliably work headless against
Sourcepoint-gated sites.

Output: {\"results\": {\"navigated\": \"...\", \"status\": 200|null, \"status_reason\": null|\"not_observed\"|\"no_document_request\"|\"no_status_reported\", \"committed_url\": \"...\", \"ready_state\": \"...\", \"elapsed_ms\": N}, \"total\": 1, \"meta\": {...}}

--with-network output: results.network is ONE canonical object on every path (quiet or busy page, --detail/--jq or default, --all or capped); 'committed_url'/'ready_state'/'status' are also present alongside it (iter-138 — previously dropped, forcing a choice between truthful navigation info and network data):
  {\"navigated\": \"...\", \"network\": {\"entries\": [...], \"shown\": N, \"total\": N, \"truncated\": bool, \"total_requests\": N, \"total_transfer_bytes\": N, \"by_cause_type\": {...}, \"slowest\": [...], \"timeout_reached\": false}, \"committed_url\": \"...\", \"ready_state\": \"...\", \"status\": 200|null, \"status_reason\": null|\"not_observed\"|\"no_document_request\"|\"no_status_reported\"}
  entries is capped at 20 by default (use --all to expand); summary fields always reflect the FULL capture.
  Note (iter-126): previously results.network was a BARE ARRAY in non-truncated detail mode (and --all), so .results.network.entries / .total_requests threw \"cannot index array\" on quiet pages. It is now always the object above; consumers of the old bare-array form should read .results.network.entries.")]
    Navigate(NavigateArgs),
    /// Evaluate JavaScript in the target tab
    #[command(long_about = "Evaluate JavaScript in the target tab.

Three input modes (exactly one required):
  Positional:  ff-rdp eval 'document.title'
  From file:   ff-rdp eval --file script.js
  From stdin:  echo 'document.title' | ff-rdp eval --stdin

Prefer --file or --stdin for scripts that contain shell metacharacters,
optional chaining (?.), template literals, or multi-line statements — shell
quoting can mangle them and produce a SyntaxError at column 1.

Scripts are routed through Firefox's Debugger.evalInGlobal, which bypasses
page CSP (iter-93). That call does NOT give each evaluation a fresh scope —
it evaluates in the tab's own global lexical environment — so from iter-93 to
iter-164 a top-level `const`/`let`/`class` survived until navigation and
re-running the same script failed with `redeclaration of const x`, even
though this help claimed otherwise. iter-165 restores the promised contract:
a script that DECLARES something at top level (`const`, `let`, `class`, `var`
or `function`) now runs inside a per-call IIFE — the same wrap --stringify
and `await` scripts already used — so declarations never leak across calls
and repeating an `eval` is idempotent. `var` and `function` are
function-scoped by that wrap too and likewise stop leaking. To publish state
deliberately, assign it to the page global (`window.mine = ...` or a bare
`mine = ...`); property writes are not declarations and are unaffected.

A script that declares nothing cannot leak anything, so it is sent verbatim
exactly as before — including a lone expression, and including statement
forms like `if (1) { 2 }`, whose script completion value is preserved. A
declaration nested inside a block, a loop head or a function body is already
scoped there and does not trigger the wrap either.

Pass --no-isolate to opt out and share ONE scope across calls: a plain
synchronous script is then sent unwrapped, exactly as it was before iter-165,
so declarations accumulate in the tab's global lexical environment (useful
when building helpers up over several calls). --no-isolate does not change
--stringify or `await` scripts — their wrap is required for those to work at
all, so their declarations never leak either way.

Top-level `await` works (iter-132): `ff-rdp eval 'await Promise.resolve(41) + 1'`
resolves to 42. Scripts containing `await` are transparently wrapped in an
async IIFE before evaluation — no `--async` flag or extra syntax needed. A
single-expression script auto-returns its value. A multi-statement script
(statements separated by `;` OR by a plain newline — iter-142 fixed
ASI-separated scripts being misclassified and leaking the wrapper into a
SyntaxError) also auto-returns its value if the LAST statement is a bare
expression, e.g. `let x = await foo(); x + 1` returns `x + 1` — every
earlier statement still runs unwrapped, so an explicit `return` earlier in
the script keeps working. Only when the last statement is not a bare
expression (a declaration, a control-flow construct) does the script need
its own explicit `return` to surface a value (it still runs either way — no
SyntaxError).

That statement split understands JS literals: a `;` or newline inside a
string, a template literal, a regular-expression literal or a `//` / `/* */`
comment is not a statement separator, and a backslash escape does not end the
literal it sits in (iter-167 — `eval --stringify '/a;b/.test(\"a;b\")'` used to
fail with \"unterminated regular expression literal\"). iter-170 closed the two
gaps that left open: a `${...}` interpolation is scanned as the code it is, so
a backtick or quote inside one no longer ends the template early
(`eval --stringify 'const s = `a${\"`\"}b`; s'` used to yield `undefined`), and
whether a `/` after `}` divides or opens a regex is now decided from what the
matching `{` opened — a block (`if (n) {} /a;b/.test(\"a;b\")`, which used to be
a SyntaxError) or an object literal (`const o = {v:8}; o.v / 2`, still
division). A block also ends its own statement, so no `;` or newline is needed
after `}`.

iter-176 closed the three positions that left unjudged, after measuring that
each turned valid JavaScript into a SyntaxError (and a class declaration into
a silent `undefined`): an arrow function's `{` body, a `class` body and a
labelled block are now read as the blocks they are, so
`eval --stringify 'class K { m(){ return 9 } } new K().m()'` returns 9 instead
of `undefined`, and `const n = 1; outer: { break outer } n` returns 1 instead
of a SyntaxError. Class *expressions* still divide: `const C = class {} / 2`
is one statement, as in JS.

It is still a scanner rather than a full JS parser. A label nested inside an
object literal or a ternary stays unjudged — the conservative answer, which
can only cost a wrap.

Since iter-165 that same last-statement rule also governs a plain (non-await)
script that declares something, because that script now runs in the per-call
IIFE described above: `eval 'const x = 1; x'` returns 1, while a declaring
script whose LAST statement is not a bare expression — `eval 'const x = 1; if
(x) { 2 }'` — yields `undefined` rather than 2. Add an explicit `return`, or
pass --no-isolate, to get the script completion value back. Declaration-free
scripts are untouched by this rule; they never enter the wrap.

Output: {\"results\": <value>, \"total\": 1, \"meta\": {...}}

A string result is always returned in full. Firefox inlines only the first
~1000 characters of a long string and hands back a `longString` grip for the
rest; ff-rdp fetches the remainder before printing (iter-161), so
`eval '\"x\".repeat(5000)'` yields all 5000 characters and never a truncated
preview.

When the result is a non-primitive (object, array), Firefox returns actor grip
metadata (actor IDs, class names) instead of the actual values. Use --stringify
to wrap the value in JSON.stringify() and get the real data back.

--stringify accepts exactly what bare eval accepts, including multi-statement
scripts and top-level `await`: `eval --stringify 'const o = {a:1}; o'` returns
{\"a\":1} (iter-161 — this used to fail with \"expected expression, got keyword
'const'\"). The same last-statement rule as above decides what is returned.

Pass --unwrap when the expression itself already returns a JSON-encoded string
(e.g. `localStorage.getItem('user')` or a server endpoint that returns text):
ff-rdp will parse it client-side and put the structured object/array into
`results`. Primitive or non-JSON strings are passed through unchanged.

NOTE: `window.resizeTo()` is a silent no-op in headless Firefox — evaluating
it will not resize the viewport or change subsequent `innerWidth` reads. For
a real window size, launch with `ff-rdp launch --window-size WxH` (see its
--help for the ~500px live-viewport floor).")]
    Eval(EvalArgs),
    /// Extract visible page text (document.body.innerText)
    #[command(long_about = "Extract visible page text (document.body.innerText).

Capped at 8000 characters by default (iter-211): `meta.total_chars` always
reports the full innerText length and `meta.truncated` says whether anything
was cut. `--full` lifts the cap; `--max-chars N` moves it.

`--query TEXT` returns only the lines containing TEXT plus `--context N`
lines either side (default 2), with `meta.matches` / `meta.shown` counting
hits. Use it instead of piping the whole article through `head` — the answer
is usually further down than the first hundred lines.

Output: {\"results\": \"<page text as a plain string>\", \"total\": 1, \"meta\": {\"total_chars\": N, \"truncated\": bool, ...}}")]
    PageText(PageTextArgs),
    /// Query DOM elements by CSS selector
    #[command(long_about = "Query DOM elements by CSS selector.

Default output (ARIA-tree JSON): {\"results\": [{\"ref\":\"e1\",\"role\":\"heading\",\"name\":\"...\",\"level\":1,\"tag\":\"h1\",\"attrs\":{...}}, ...], \"total\": N}

Since iter-61i, `results` is **always an array** regardless of match count (0 → [], 1 → [item], N → [item, ...]). Agent recipes like `--jq '.results[0]'` work uniformly.

Each element has: ref (stable ID), role (ARIA semantic role), name (accessible name), tag, attrs (actionable only), state, level (headings).

For input/textarea/select elements, a top-level `value` field carries the
live `.value` DOM property — distinct from `attrs.value`, which is the
static HTML `value` attribute (getAttribute) and does not change when
script or user input edits the field. Use `value` to see the current
form-field content without an extra `eval` round-trip; `attrs.value`
reflects only what the HTML markup originally declared.

Use --format html for raw HTML strings in each array slot.
Use --first to revert to the legacy single-value shape (object/string/null, total: 0 or 1).
With --count: {\"results\": {\"count\": N}, \"total\": N, \"meta\": {...}}

See also:
  ff-rdp styles <SEL>    — declared (matched) CSS rules for an element.
  ff-rdp computed <SEL>  — resolved computed style values for an element.")]
    Dom(DomArgs),
    /// Read console messages
    #[command(long_about = "Read console messages.

Default: 50 messages, sorted by timestamp (newest first).
Output always includes a `summary` field with totals and per-level counts so
callers can tell at a glance whether the filter caught what they expected.

The command primes Firefox's message cache (startListeners) before reading it,
so messages a separate earlier `ff-rdp eval 'console.log(...)'` emitted are
visible on a fresh --no-daemon connection. Use --follow to stream live messages.

Output: {\"results\": [{\"level\": \"...\", \"message\": \"...\", \"source\": \"...\", \"line\": N, \"timestamp\": N}], \"summary\": {\"total\": N, \"shown\": Z, \"by_level\": {...}, \"matched\": M}, \"total\": N, \"meta\": {...}}")]
    Console(ConsoleArgs),
    /// Show network requests captured by the WatcherActor.
    ///
    /// In direct mode (--no-daemon), only requests made after connection are
    /// reliably captured. When no live events are found, falls back to the
    /// Performance API for historical resource data. Use the daemon (default)
    /// for continuous buffering, or `navigate --with-network` to capture
    /// requests triggered by a navigation.
    #[command(long_about = "Show network requests captured by the WatcherActor.

In direct mode (--no-daemon), only requests made after the connection is
established are reliably captured. When no live network events are available
(e.g. the page finished loading before ff-rdp connected), the command
automatically falls back to the Performance API to retrieve historical
resource timing data. Fallback entries have source=performance-api in the
output metadata and method=null/status=null (method and status are not
available from the Performance API).

Recommended workflows:
  - Daemon mode (default): run `ff-rdp` without --no-daemon so the daemon
    buffers events continuously across commands.
  - Navigate with capture: use `ff-rdp navigate --with-network <url>` to
    start network monitoring before the page load begins.

The --filter and --method flags narrow results after capture; they do not
affect which requests Firefox records.

Navigation scoping (daemon mode only):
  By default, `ff-rdp network` returns only entries captured since the most
  recent navigation — so requests from previous pages don't appear.
  Use --since to change the scope:
    --since -1   current navigation (default)
    --since -2   one navigation back
    --since all  the full cumulative buffer (pre-61g behaviour)
  --since requires the daemon: nav-scoped filtering reads the daemon's
  navigation-boundary buffer. With --no-daemon (or if the daemon can't be
  reached) an explicit --since fails with error_type \"since_requires_daemon\"
  rather than silently returning the unfiltered buffer.

Source (--source watcher, the default):
  --source watcher          the RDP resource watcher — the only source with
                            method/status/content_type/transfer_size. An empty
                            buffer reports 0 watcher rows; it is never swapped
                            for a different dataset. (`auto` is a deprecated
                            alias of `watcher`.)
  --source performance-api  only Resource Timing; identical in both connection
                            modes, no headers/security detail, and incompatible
                            with --since (error_type
                            \"since_requires_watcher_source\")

iter-159 removed the implicit fallback. `auto` used to mean \"watcher if it
produced anything, else the Performance API\", which made the same page report
different row counts in the two connection modes — and, worse, made a daemon
whose watcher had stopped delivering anything at all look like a page with no
HTTP metadata. `meta.source` names the source that was actually read.

Field fidelity by source:
  watcher:         method, status, content_type, duration_ms, size_bytes, transfer_size all available.
                   (iter-128: content_type is backfilled from the response's Content-Type
                   header in --detail mode when the watcher's own mimeType update hasn't
                   landed yet — a single extra request only when the field is still null.)
  performance-api: method=null, status=null; duration_ms, transfer_size available via Resource Timing API

`hint` is always present (iter-128) — null when there's nothing to report, a
string when results are truncated, a timeout fired, or the capture was empty.

`meta.route` (iter-128; all browser-touching commands since iter-134) is
always present — \"daemon\" or \"direct\" — regardless of --verbose, so you
can tell how this command executed without a separate `daemon status` call.

Default: 20 results, sorted by duration (slowest first).
Output (summary mode): {\"results\": {\"total_requests\": N, \"total_transfer_bytes\": N, \"by_cause_type\": {...}, \"slowest\": [...], \"timeout_reached\": false, \"hint\": null}, \"total\": N, \"meta\": {\"route\": \"daemon\", ...}}
Output (--detail): {\"results\": [{\"url\": \"...\", \"method\": \"GET\", \"status\": 200, \"duration_ms\": N, ...}], \"total\": N, \"total_requests\": N, \"total_transfer_bytes\": N, \"by_cause_type\": {...}, \"slowest\": [...], \"timeout_reached\": false, \"hint\": null, \"meta\": {\"route\": \"daemon\", ...}}
  Note (iter-126): detail mode carries the summary fields (total_requests, total_transfer_bytes, by_cause_type, slowest) alongside the results array, so --detail is a strict superset of the summary envelope.
  Reaching the entry list: pass --detail (or --all/--headers/--security/--sort/--limit/--fields).
  BREAKING (iter-160): --jq no longer switches this command into detail mode. `network --jq
  \'.results | type\'` now answers \"object\", the same as plain `network` — previously it
  answered \"array\", so the filter changed the document it was filtering. --jq is a view over
  the envelope on all 30 commands; network was the only one where it was also a mode switch.
  Migration: add --detail to any --jq invocation that expected the entry list.
Output (--detail --headers): adds {\"headers\": {\"request\": [{\"name\": \"...\", \"value\": \"...\"}], \"response\": [...]}} per entry.
--format text truncates long `url` cells with a middle ellipsis (iter-128) so a
single ~900-char tracking URL can't blow the table out to thousands of columns.")]
    Network(NetworkArgs),
    /// Query browser Performance API entries and Core Web Vitals
    #[command(
        long_about = "Query browser Performance API entries and Core Web Vitals.

Default: 20 resources, sorted by duration (slowest first).
Output: {\"results\": [{\"url\": \"...\", \"duration_ms\": N, \"transfer_size\": N, ...}], \"total\": N, \"meta\": {...}}"
    )]
    Perf(PerfArgs),
    /// Capture a screenshot
    #[command(long_about = "Capture a screenshot.

By default the screenshot is captured at the current viewport size over the
live RDP session. Use --full-page to capture the entire scrollable document
(up to document.scrollingElement.scrollHeight) or --viewport-height N for an
explicit override. NOTE: headless `window.resizeTo()` is a silent no-op —
it does not change what a plain `screenshot` captures.

--window-size WxH (iter-133) switches to a different capture mode entirely:
a one-shot `firefox --headless --window-size --screenshot` subprocess in a
fresh scratch profile, giving an EXACT WxH PNG with no floor — the only true
sub-500px mobile raster path (the live RDP capture above inherits whatever
window `launch` created, which clamps below ~500px CSS width). This
re-navigates the current tab's URL from scratch: cookies/localStorage/session
state from the live tab are NOT carried over. No density knob here —
`layout.css.devPixelsPerPx` was tested against this exact capture path and
found to have zero effect on the output raster (Firefox 153.0.3); use
`emulate --dppx` for the LIVE RDP session's devicePixelRatio instead (a
different, unrelated mechanism). Mutually exclusive with
--full-page/--viewport-height.

Output: {\"results\": {\"path\": \"...\", \"width\": N, \"height\": N}, \"total\": 1, \"meta\": {...}}
With --base64: {\"results\": {\"base64\": \"...\"}, \"total\": 1, \"meta\": {...}}
With --window-size: {\"results\": {\"path\"|\"base64\": ..., \"width\": N, \"height\": N, \"capture\": \"batch-window-size\"}, \"total\": 1, \"meta\": {...}}")]
    Screenshot(ScreenshotArgs),
    /// Click an element matching a CSS selector
    #[command(long_about = "Click an element matching a CSS selector.

Auto-waits for the element to exist, be visible, and have a stable bounding rect
before dispatching the full pointer-event sequence (pointerover, pointerenter,
pointerdown, pointerup, click). This matches the behaviour expected by modern
component libraries such as Radix UI and Headless UI.

The selector can be supplied positionally or via --selector:
  ff-rdp click 'button[type=submit]'
  ff-rdp click --selector 'button[type=submit]'

Both forms are interchangeable; supplying both at once is an error.

Use --ref <id> to click an element by its ARIA-tree ref ID (e.g. 'e3' from a
previous dom or snapshot call in the same daemon session).  Mutually exclusive
with positional selector and --selector.  Not available with --no-daemon.

Dispatch modes (--dispatch):
  pointer     Full pointer+mouse event sequence (default — Radix/Headless-UI compatible)
  legacy      Mouse-event sequence only (mouseover, mouseenter, mousedown, mouseup, click)
  click-only  Synthetic .click() only (pre-iter-59 behaviour, fastest)

Cross-origin / iframe reach (iter-129): if the selector isn't found in the top
document, click automatically enumerates the tab's frame targets (including
cross-origin, out-of-process iframes such as consent-management-platform
overlays) and retries inside each one until a match clicks successfully. Use
--frame <url-substring> to target a specific frame directly and skip both the
top-level attempt and the scan (e.g. --frame sourcepoint). If the selector
matches nowhere, the error names how many frames were tried and their URLs
instead of a bare timeout.

When a selector matches more than one element, the default (flag-less)
behaviour clicks DOM-order index 0 — which may be hidden. Use --visible to
click the first non-hidden match instead, or --index N to pick a specific
match (0-based). Mutually exclusive with each other. On success, the output
gains {\"match_count\": N, \"chosen_index\": N}; on failure (no visible match /
index out of range), the error names the match count. If the plain selector
times out because it resolved to a hidden element, the error itself suggests
--visible/--index with the observed match count.

Reachability (iter-160): before dispatching anything, click hit-tests the
element's centre point with document.elementFromPoint. The point must resolve to
the element, a descendant of it (a <span> inside a <button> is the normal case),
or an ancestor of it (an ancestor cannot obscure its own descendant). If the
centre starts outside the viewport the element is scrolled into view first —
below the fold is not an obstruction.

  results.matched     the selector resolved to an element
  results.reachable   true | false | null (hit test could not decide — e.g. a
                      cross-origin iframe document that was never laid out; the
                      events ARE dispatched and the envelope says so)
  results.obscured_by the covering element, e.g. \"div#veil\" (null on success)

A genuinely covered target is a FAILED action, not an informational result: exit
1 with error_type \"click_obscured\" (or \"click_offscreen\" when the centre is
still outside the viewport after scrolling), and matched/reachable/obscured_by
appear at the top level of the error envelope. The removed `entered` field meant
only \"querySelector matched\" while its name claimed the pointer could enter;
read `matched` instead.

Ceiling: events remain isTrusted: false and e.clientX/e.clientY remain 0. The hit
test decides WHETHER to dispatch; it does not give the events real coordinates.

Output: {\"results\": {\"clicked\": true, \"matched\": true, \"reachable\": true, \"obscured_by\": null, \"tag\": \"...\", \"text\": \"...\", \"frame_url\": null}, \"total\": 1, \"meta\": {\"frame_url\": null, ...}}
`frame_url` is always present (never omitted) — null when the click landed on
the top-level document, the frame's URL string when it landed inside a frame.
With --wait-for-network: adds {\"network\": {\"url\": \"...\", \"method\": \"...\", \"status\": N, ...}} to results.")]
    Click(ClickArgs),
    /// Type text into an input element matching a CSS selector
    #[command(long_about = "Type text into an input element matching a CSS selector.

Selector and text can be supplied positionally or via flags:
  ff-rdp type 'input[name=email]' 'user@example.com'
  ff-rdp type --selector 'input[name=email]' --text 'user@example.com'

Both forms work identically; mixing positional and flag for the same value errors.

Use --ref <id> to target an element by its ARIA-tree ref ID (daemon mode only).
Mutually exclusive with positional selector and --selector.

Auto-waits for the element to be focusable (exists, visible, not disabled, is an
input/textarea/contenteditable) before typing. Use --no-wait to skip this.

The value is set via the native HTMLInputElement/HTMLTextAreaElement/HTMLSelectElement
prototype setter so React/Vue/Svelte value trackers are invalidated. Each character
is typed as a keydown -> keypress -> keyup sequence with the value applied
incrementally (iter-160), so a combobox that opens on keydown or a search box that
debounces keyup responds as it would to a typist; `input` and `change` are dispatched
once at the end, as before.

Synthetic-input ceiling (reported as \"synthetic\": true in the output):
  Firefox exposes no trusted-input surface over the devtools RDP — Marionette and
  WebDriver BiDi are peer protocols, not layers reachable through it — so every
  event `type` dispatches carries isTrusted: false. A page that filters on
  e.isTrusted will ignore them, and a handler that calls preventDefault() on
  keydown will NOT suppress the character: ff-rdp assigns the value directly, and
  a synthetic preventDefault cannot cancel that assignment. This makes key-driven
  UIs respond; it does not make `type` indistinguishable from a user.

When a selector matches more than one element (e.g. two `input[name=keywords]`
on the same page, one hidden), the default (flag-less) behaviour types into
DOM-order index 0. Use --visible to target the first non-hidden match instead,
or --index N for a specific match (0-based). Mutually exclusive with each
other. On success, the output gains {\"match_count\": N, \"chosen_index\": N}.

Output: {\"results\": {\"typed\": true, \"synthetic\": true, \"tag\": \"INPUT\", \"value\": \"...\"}, \"total\": 1, \"meta\": {...}}")]
    Type(TypeArgs),
    /// Wait for a condition to become true (polls every 100ms), or sleep for a fixed duration.
    /// Exactly one of --selector, --text, --eval, --ref, or --sleep-ms must be specified.
    #[command(
        long_about = "Wait for a condition to become true (polls every 100ms), or sleep for a fixed duration.

Exactly one of --selector, --text, --eval, --ref, or --sleep-ms must be specified.

Use --ref <id> to wait for an element identified by its ARIA-tree ref ID
(daemon mode only). Equivalent to --selector but uses a stable ref handle.

Use --sleep-ms <N> for a plain delay with no condition and no Firefox
connection at all — e.g. `ff-rdp wait --sleep-ms 2000`. Prefer a real
condition (--selector/--text/--eval/--ref) whenever one exists; a fixed
sleep is always a guess about how long something takes. --timeout-ms does
not apply to --sleep-ms, which always runs for exactly its own duration.

Output: {\"results\": {\"matched\": true, \"elapsed_ms\": N, \"condition\": \"selector|text|eval|sleep\"}, \"total\": 1, \"meta\": {...}}"
    )]
    Wait(WaitArgs),
    /// List cookies via the Firefox StorageActor (includes httpOnly, secure, sameSite, etc.)
    #[command(
        long_about = "List cookies via the Firefox StorageActor (includes httpOnly, secure, sameSite, etc.).

Output: {\"results\": [{\"name\": \"...\", \"value\": \"...\", \"domain\": \"...\", \"path\": \"...\", \"secure\": true, \"httpOnly\": true}], \"total\": N, \"meta\": {...}}"
    )]
    Cookies(CookiesArgs),
    /// Read web storage (localStorage or sessionStorage)
    #[command(long_about = "Read web storage (localStorage or sessionStorage).

Output: {\"results\": [{\"key\": \"...\", \"value\": \"...\"}], \"total\": N, \"meta\": {...}}
With --key: {\"results\": {\"key\": \"...\", \"value\": \"...\"}, \"total\": 1, \"meta\": {...}}")]
    Storage(StorageArgs),
    /// Inspect accessibility tree and check WCAG compliance
    #[command(long_about = "Inspect accessibility tree and check WCAG compliance.

Output: {\"results\": {\"role\": \"...\", \"name\": \"...\", \"children\": [...]}, \"total\": 1, \"meta\": {..., \"source\": \"native\"|\"js-fallback\"}}
With a11y summary: {\"results\": [{\"role\": \"...\", \"name\": \"...\", \"level\": N}], \"total\": N, \"meta\": {...}}
With a11y contrast: {\"results\": [{\"selector\": \"...\", \"ratio\": N, \"aa_normal\": bool, ...}], \"total\": N, \"sampled\": M, \"capped\": bool, \"source\": \"js-fallback\", \"meta\": {..., \"source\": \"js-fallback\"}}
  a11y contrast `total` = returned results (AA failures under --fail-only, else all checks); `sampled` = elements examined. Pre-iter-127 `total` reported the sample size.
  `meta.source` (iter-143) is always present on a11y/a11y --critical/a11y contrast: \"native\" means the real Firefox platform accessibility tree (roles like \"document\"/\"paragraph\"); \"js-fallback\" means a DOM-derived approximation (roles like \"generic\"), with `meta.source_reason` naming why. `a11y --native` opts in to the native tree (never the default — DEC-027) by enabling Firefox's accessibility service for the duration of the call.
  `meta.service_left_enabled` / `meta.service_restore_error` (iter-149) are always present on plain `a11y`: `service_left_enabled` is true only when `--native` enabled the platform accessibility service and could not restore it afterward (the service stays enabled for as long as this command's connection is open, normally just until it exits), with `service_restore_error` naming the failure; both stay false/null when nothing needed restoring. The walked tree is still returned in `results` even when the restore failed.")]
    A11y(A11yArgs),
    /// Reload the page
    #[command(long_about = "Reload the page.

Without --wait-idle, the command blocks until the reload commits — same
{committed_url, ready_state, elapsed_ms, status, status_reason} envelope as
`navigate`/`back`/`forward` (iter-130, completed by iter-169), so all four
navigation verbs are interchangeable for a caller that just wants to know
where the page landed and what the server said.

`status` is the main document's HTTP status; it is null exactly when
`status_reason` is not, and `status_reason` names which case it was:
`not_observed` (--no-wait, or --wait-idle, neither of which correlates a
document request), `no_document_request` (nothing was fetched — a BFCache
restore, a data:/about: URL, a same-document navigation), or
`no_status_reported` (the request was identified but Firefox never reported a
status for it).

With --wait-idle, the command instead blocks until network activity has been
idle for --idle-ms (default 500) or the --reload-timeout expires (default
10000) — a different envelope (`reloaded`/`idle_at_ms`/`requests_observed`)
geared at network-quiescence rather than commit timing.

Pass --hard for a cache-bypassing reload (Firefox `options.force`, the
protocol equivalent of Cmd-Shift-R / `LoadFlags::BYPASS_CACHE`).  Default
remains a soft reload.

Pass --no-wait to dispatch the reload and return immediately without waiting
for it to commit (iter-138 Theme E) — the same escape hatch `navigate`
already has. Conflicts with --wait-idle (which is itself a different kind of
wait).

Examples:
  ff-rdp reload
  ff-rdp reload --hard
  ff-rdp reload --no-wait
  ff-rdp reload --wait-idle
  ff-rdp reload --hard --wait-idle --idle-ms 1000 --reload-timeout 30000

Output (plain):    {\"results\": {\"action\": \"reload\", \"committed_url\": \"...\", \"ready_state\": \"complete\", \"elapsed_ms\": N, \"status\": 200|null, \"status_reason\": null|\"not_observed\"|\"no_document_request\"|\"no_status_reported\"[, \"force\": true]}, \"total\": 1, \"meta\": {...}}
Output (--no-wait): {\"results\": {\"action\": \"reload\", \"status\": null, \"status_reason\": \"not_observed\"[, \"force\": true]}, \"total\": 1, \"meta\": {...}}
Output (wait-idle): {\"results\": {\"reloaded\": true, \"idle_at_ms\": N, \"requests_observed\": M, \"status\": null, \"status_reason\": \"not_observed\"[, \"force\": true]}, \"total\": 1, \"meta\": {...}}")]
    Reload(ReloadArgs),
    /// Go back in history
    #[command(long_about = "Navigate back in browser history.

Blocks until the navigation commits, returning the same navigate-style
envelope as `navigate`/`forward`/`reload` (iter-130, completed by iter-169) —
a caller doesn't need a follow-up `eval location.href` to know where `back`
landed, nor a follow-up `network` call to know what the server said.

A history traversal served from BFCache issues no request at all, which is
reported honestly as `status: null` with `status_reason:
\"no_document_request\"` rather than as an unexplained null.

Pass --no-wait to dispatch and return immediately without waiting for the
navigation to commit (iter-138 Theme E) — the same escape hatch `navigate`
already has.

Output:              {\"results\": {\"action\": \"back\", \"committed_url\": \"...\", \"ready_state\": \"complete\", \"elapsed_ms\": N, \"status\": 200|null, \"status_reason\": null|\"not_observed\"|\"no_document_request\"|\"no_status_reported\"}, \"total\": 1, \"meta\": {...}}
Output (--no-wait): {\"results\": {\"action\": \"back\", \"status\": null, \"status_reason\": \"not_observed\"}, \"total\": 1, \"meta\": {...}}")]
    Back(BackForwardArgs),
    /// Go forward in history
    #[command(long_about = "Navigate forward in browser history.

Blocks until the navigation commits, returning the same navigate-style
envelope as `navigate`/`back`/`reload` (iter-130, completed by iter-169),
including the main document's `status` and the `status_reason` that explains
a null one.

Pass --no-wait to dispatch and return immediately without waiting for the
navigation to commit (iter-138 Theme E) — the same escape hatch `navigate`
already has.

Output:              {\"results\": {\"action\": \"forward\", \"committed_url\": \"...\", \"ready_state\": \"complete\", \"elapsed_ms\": N, \"status\": 200|null, \"status_reason\": null|\"not_observed\"|\"no_document_request\"|\"no_status_reported\"}, \"total\": 1, \"meta\": {...}}
Output (--no-wait): {\"results\": {\"action\": \"forward\", \"status\": null, \"status_reason\": \"not_observed\"}, \"total\": 1, \"meta\": {...}}")]
    Forward(BackForwardArgs),
    /// Inspect a remote JavaScript object by its grip actor ID
    #[command(long_about = "Inspect a remote JavaScript object by its grip actor ID.

Actor IDs appear in eval results when the return value is a non-primitive
(e.g. {\"type\": \"object\", \"actor\": \"server1.conn0.child0/obj12\", ...}).
Use --depth to control how many levels of nested objects are resolved.

Output: {\"results\": {\"actor\": \"...\", \"prototype\": {...}, \"ownProperties\": {...}}, \"total\": 1, \"meta\": {...}}")]
    Inspect(InspectArgs),
    /// List JavaScript/WASM sources loaded on the page
    #[command(long_about = "List JavaScript/WASM sources loaded on the page.

Output: {\"results\": [{\"url\": \"...\", \"actor\": \"...\", \"isBlackBoxed\": bool}], \"total\": N, \"meta\": {...}}
--format text truncates long `url` cells with a middle ellipsis (iter-128) so a
single very long source URL can't blow the table out to thousands of columns.")]
    Sources(SourcesArgs),
    /// Dump structured page snapshot for LLM consumption: DOM tree with semantic roles,
    /// key attributes, interactive elements, and text content
    #[command(
        long_about = "Dump structured page snapshot for LLM consumption: DOM tree with semantic roles, key attributes, interactive elements, and text content.

Output: {\"results\": {\"tag\": \"HTML\", \"children\": [...], ...}, \"total\": 1, \"meta\": {...}}"
    )]
    Snapshot(SnapshotArgs),
    /// Internal: run as background daemon (not for direct use)
    #[command(name = "_daemon", hide = true)]
    DaemonInternal,

    /// Manage the background daemon process
    #[command(long_about = "Manage the background daemon process.

The daemon keeps a persistent Firefox connection and buffers events across
commands. It starts automatically on the first command that needs it.

Output (status): {\"results\": {\"running\": bool, \"pid\": N, \"port\": N, \"uptime_seconds\": N, \"connections\": N, \"buffer_sizes\": {...}}, \"total\": 1, \"meta\": {...}}
Output (stop):   {\"results\": {\"stopped\": bool}, \"total\": 1, \"meta\": {...}}")]
    Daemon {
        #[command(subcommand)]
        daemon_command: DaemonCommand,
    },
    /// Get element geometry: bounding rects, position, z-index, visibility, overflow,
    /// with automatic overlap detection between elements
    #[command(
        long_about = "Get element geometry: bounding rects, position, z-index, visibility, overflow.

Automatically detects overlaps between queried elements.

By default, hidden and zero-sized elements are excluded from results (elements with
display:none, visibility:hidden, opacity:0, or a zero bounding rect). Pass --include-hidden
to receive those elements as well.

NOTE: behavior change — prior versions included hidden elements by default and required
--visible-only to filter them. Scripts relying on the old default must add --include-hidden.

Output: {\"results\": {\"elements\": [{\"selector\": \"...\", \"rect\": {...}, \"visible\": bool, \"z_index\": N}], \"overlaps\": [...]}, \"total\": 1, \"meta\": {...}}"
    )]
    Geometry(GeometryArgs),
    /// Test responsive layout across viewport widths: resize to each width,
    /// collect geometry + computed styles for the given selectors, then restore
    /// the original viewport size.  Returns results keyed by breakpoint width.
    #[command(long_about = "Test responsive layout across viewport widths.

Simulates each width by constraining the page layout (inline CSS on <html>/<body>),
collects element geometry at each breakpoint, then restores the original viewport.

IMPORTANT — layout-only emulation: over the RDP protocol ff-rdp cannot resize the
real top-level window, so CSS @media queries continue to evaluate against the
physical viewport width. Geometry is correct for the requested width, but media
queries may not flip. Every breakpoint therefore carries a `media_query_check`
object — {requested, inner_width, matches} — where `matches` is
matchMedia(\"(width: <requested>px)\").matches. When it is false a warning is
attached; pass --strict to make a mismatch exit non-zero.

For TRUE viewport emulation (not this command's layout-only CSS-width
constraint), use `ff-rdp launch --window-size WxH` before navigating — real
innerWidth and real media queries above the ~500px live-viewport floor. For
a true sub-500px mobile screenshot, use `ff-rdp screenshot --window-size WxH`
instead (see either --help for details). `responsive` remains the right tool
for geometry/breakpoint auditing across many widths in one call.

By default, hidden and zero-sized elements are excluded from results at each breakpoint.
Pass --include-hidden to receive those elements as well.

Output: {\"results\": {\"breakpoints\": [{\"width\": 320, \"viewport\": {\"width\": N, \"height\": N}, \"media_query_check\": {\"requested\": 320, \"inner_width\": N, \"matches\": bool}, \"elements\": [{\"selector\": \"...\", \"rect\": {...}, \"visible\": bool}]}, ...], \"original_viewport\": {\"width\": N, \"height\": N}, \"warnings\": [...]}, \"total\": N, \"meta\": {...}}")]
    Responsive(ResponsiveArgs),
    /// Emulate the page environment via the target-configuration actor
    #[command(
        long_about = "Emulate the page environment (server-side) via the Firefox \
target-configuration actor.

Each flag maps to one field of the actor's configuration and only the flags you
pass are applied — a call patches the live configuration rather than replacing it.

  --user-agent <S>          override navigator.userAgent / User-Agent header
  --color-scheme light|dark|none   simulate prefers-color-scheme (none = system)
  --dppx <F>                override window.devicePixelRatio (positive number)
  --print on|off            toggle @media print simulation (compose with screenshot)
  --touch on|off            toggle touch-event simulation
  --js on|off               enable/disable JavaScript (server reloads the document)
  --offline on|off          take the tab offline (navigator.onLine, fetch failures)
  --cache on|off            'off' disables the HTTP cache (cold-load perf)
  --reset                   restore every field to its default (cannot combine with others)

LIFETIME: configuration lives as long as the RDP connection that set it. Under
the daemon that means until the daemon restarts; with --no-daemon the one-shot
process disconnects immediately and the setting is discarded — the envelope then
carries a `lifetime_warning`. Disabling JavaScript or --reset triggers a
server-side document reload; reload/re-probe to observe the effect.

Examples:
  ff-rdp emulate --color-scheme dark --user-agent 'ff-rdp-test/1.0'
  ff-rdp emulate --dppx 2 --touch on
  ff-rdp emulate --js off        # then `ff-rdp reload` before probing
  ff-rdp emulate --reset

Output: {\"results\": {\"applied\": {<wire-field>: <value>, ...}, \"reset\": bool, \
\"lifetime_warning\"?: \"...\", \"note\"?: \"...\"}, \"total\": 1, \"meta\": {...}}"
    )]
    Emulate(EmulateArgs),
    /// Throttle the network and/or block request URLs (network-parent actor)
    #[command(
        long_about = "Throttle network speed and/or block request URLs (server-side) via the \
Firefox network-parent actor.

Throttling and blocking are configured on the parent-process network-parent
actor obtained from the watcher.  A positional PROFILE sets a throttling tier;
`--block` replaces the URL block-list.  At least one must be supplied.

  throttle slow-3g          ~400 kbit/s, 400 ms latency
  throttle fast-3g          ~1.6 Mbit/s, 150 ms latency
  throttle off              clear throttling (full speed)
  throttle status           report the profile last applied via the daemon
  throttle --block <PAT>    block requests whose URL matches PAT (repeatable)
  throttle --unblock        clear the URL block-list

PROFILE and --block compose: `throttle slow-3g --block '*.png'` throttles AND
blocks in one call.  Blocked requests fail with NS_ERROR_ABORT and show up as
errored entries in `network` output while other requests succeed.

PREREQUISITE: this command subscribes to network-event resources first (the
network-parent actor throws \"Not listening for network events\" otherwise).

LIFETIME: throttling and blocking live as long as the RDP connection that set
them.  Under the daemon that means until the daemon restarts; with --no-daemon
the one-shot process disconnects immediately and the setting is discarded — the
envelope then carries a `lifetime_warning`.  Both survive `navigate` and
`reload` under the daemon (iter-164: they used not to — navigate's resource
teardown destroyed the block-list on the shared connection, so `--block` was
accepted, echoed here, and then not enforced).

STATUS (iter-131): `throttle status` reports the profile the daemon last
applied. Firefox's network-parent actor has no getter for the active
throttling state, so this is client-side bookkeeping (a small file next to the
daemon registry), not a live read from the browser — it reports `null` when no
daemon is running, no `throttle <profile>` has been applied since it started,
or the daemon has since restarted (which itself clears Firefox's throttling).
Read-only: combining `status` with --block/--unblock is rejected.

CACHE CAVEAT: throttling does not bypass the HTTP cache — a `reload` while
throttled may still be served from cache and look far faster than the profile
alone would suggest. Use `reload --hard` to force a network fetch.

Examples:
  ff-rdp throttle slow-3g
  ff-rdp throttle fast-3g --block 'ads.example.com' --block '*.gif'
  ff-rdp throttle off
  ff-rdp throttle status
  ff-rdp throttle --unblock

Output (set): {\"results\": {\"profile\": \"slow-3g\"|\"fast-3g\"|\"off\"|null, \
\"blocked_urls\": [\"...\"]|null, \"lifetime_warning\"?: \"...\"}, \"total\": 1, \"meta\": {...}}
Output (status): {\"results\": {\"profile\": \"slow-3g\"|\"fast-3g\"|null, \
\"note\"?: \"...\", \"cache_caveat\": \"...\"}, \"total\": 1, \"meta\": {...}}"
    )]
    Throttle(ThrottleArgs),
    /// Fetch and validate the page's Web App Manifest (PWA-readiness audit)
    #[command(
        long_about = "Fetch and validate the current page's Web App Manifest via the Firefox \
manifest actor's fetchCanonicalManifest (the WHATWG \"obtain a manifest\" algorithm).

Returns the parsed manifest plus its conformance `errors` array in a single call.
A page that links no manifest is NOT an error: the envelope reports
`manifest: null` with a `reason` and exits 0, so scripts can branch on presence
without parsing error output.

Examples:
  ff-rdp manifest
  ff-rdp manifest --jq '.results.manifest.name'
  ff-rdp manifest --jq '.results.errors'

Output: {\"results\": {\"manifest\": {<parsed manifest> | null}, \"url\": \"...\"|null, \
\"errors\": [...], \"reason\"?: \"...\"}, \"total\": 1, \"meta\": {...}}"
    )]
    Manifest,
    /// Quick wrapper around getComputedStyle for CSS debugging
    #[command(
        long_about = "Quick wrapper around getComputedStyle() for CSS debugging.

Returns non-default computed style properties for every element matching the
selector. Multi-match behaviour mirrors `dom`: one entry per matching element,
each with {selector, index, computed: {...}}.

  ff-rdp computed h1
  ff-rdp computed h1 --prop color
  ff-rdp computed .card --all

Output (multi-match): {\"results\": [{\"selector\": \"...\", \"index\": 0, \"computed\": {...}}], \"total\": N, \"meta\": {...}}
Output (--prop): single string value per match
Output (--all): full resolved-style object per match (dumps every property)"
    )]
    Computed(ComputedArgs),
    /// Inspect CSS styles for an element matching a CSS selector
    #[command(
        long_about = "Inspect CSS styles for an element matching a CSS selector.

When a selector matches more than one element, use --visible to inspect the
first non-hidden match instead of the default DOM-order index 0, or --index N
for a specific match (0-based). Mutually exclusive with each other; resolved
before styles are read, so the reported selector/rules are for the chosen
element only.

Output (computed):  {\"results\": [{\"selector\": \"...\", \"computed\": {\"color\": \"...\", ...}}], \"total\": N, \"meta\": {...}}
Output (--applied): {\"results\": [{\"selector\": \"...\", \"rules\": [{\"selector\": \"...\", \"properties\": [...]}]}], \"total\": N, \"meta\": {...}}
Output (--layout):  {\"results\": [{\"selector\": \"...\", \"box\": {\"margin\": {...}, \"border\": {...}, \"padding\": {...}, \"content\": {...}}}], \"total\": N, \"meta\": {...}}"
    )]
    Styles(StylesArgs),
    /// Explain *why* a CSS property has the value it does (cascade view)
    #[command(
        long_about = "Show the ordered list of CSS rules that determine a property's value.

For the first element matching SELECTOR, returns each rule that declares the
property in cascade order, annotated with origin (ua/user/author/inline),
matched selector specificity, stylesheet:line, declaration value, and an
!important flag.  The rule whose declaration wins gets `winner: true`.

Output: {\"results\": [{\"selector\": \"...\", \"property\": \"...\", \"computed\": \"...\",
                       \"rules\": [{...}, {...}]}], \"total\": N, \"meta\": {...}}

Examples:
  ff-rdp cascade 'dialog#lightbox' --prop display
  ff-rdp cascade h1 --prop color
  ff-rdp cascade '.btn'                # all properties declared on the element"
    )]
    Cascade(CascadeArgs),
    /// Scroll the page or a specific element
    #[command(long_about = "Scroll the page or a specific element.

Subcommands:
  scroll to <SELECTOR>       Scroll element into viewport
  scroll by                  Scroll viewport by pixels or a page
  scroll top                 Scroll to the very top of the page
  scroll bottom              Scroll to the very bottom of the page
  scroll container <SEL>     Scroll an overflow container
  scroll until <SELECTOR>    Scroll until element is visible
  scroll text <TEXT>         Find text and scroll to it")]
    Scroll {
        #[command(subcommand)]
        scroll_command: ScrollCommand,
    },
    /// Launch Firefox with remote debugging enabled
    #[command(
        long_about = "Launch a new Firefox instance with remote debugging enabled.

This is safe to run while your normal Firefox browser is open — it always
uses the -no-remote flag and a separate profile, so the new instance is
fully independent and won't interfere with existing windows.

By default a temporary profile is created with the necessary devtools prefs
enabled. Use --profile to reuse an existing profile, or --temp-profile to
make the temporary profile explicit.

--window-size WxH (iter-133) forwards `-width`/`-height` to Firefox, giving
this launched instance a real window size. Widths >= ~500px CSS pixels get
TRUE viewport emulation: real `window.innerWidth`, real `@media` query
evaluation, real layout — this is the only ff-rdp path that does. Below
~500px, Firefox clamps the LIVE debugger-server instance's viewport up to
that floor (empirically confirmed on macOS; see
kb/research/viewport-emulation.md) — the requested size still appears in
the envelope, alongside a warning, but `eval innerWidth` will read ~500,
not your smaller request. For a true sub-500px mobile SCREENSHOT (not a
live session), use `ff-rdp screenshot --window-size WxH` instead, which
has no floor. There is no RDP actor that sizes a viewport (RDM does it via
parent-chrome CSS, unreachable over the wire) — `--window-size` is a real
Firefox window-feature flag, not protocol-level emulation.

--launch-timeout SECS (iter-158) bounds how long `launch` waits for Firefox to
open its debug port after spawning. Default 30 s; `FF_RDP_LAUNCH_TIMEOUT_SECS`
overrides the default, and the flag overrides the env var. A malformed env
value falls back to 30 s rather than failing the launch. This is deliberately
NOT the global --timeout (a 10 s per-socket-operation deadline): Firefox was
measured binding its debug port at 7 s under load, and the previous hardcoded
5 s bound failed 5/5 contended launches. The effective bound is reported as
`meta.launch_wait_secs`. If the port is occupied by another process before the
spawn, `launch` fails immediately naming that process and PID instead of
waiting out the bound.

`launch` is a NO-OP when the port is already held by a Firefox ff-rdp itself
launched (iter-210): it exits 0 and reports that instance —
`results.already_running: true` with the existing `pid`, `port` and `profile`
— rather than failing, so an agent that is unsure whether it already has a
browser can just run `launch` and carry on. `already_running` is present on
both paths (`false` on a real launch), so `--jq '.results.already_running'`
always answers. --replace is unaffected: it still stops the prior instance and
starts a new one. Ownership is proved exactly as --replace proves it before it
may signal anything — a launch record whose PID still identifies the process it
was written for, or an owner-PID marker under ff-rdp's managed profile root. A
Firefox you started by hand, or any other listener, is a foreign owner and
still gets the port-occupied error; reporting someone else's process as
`results.pid` would be a lie this command has no way to back up.

When `launch` FAILS after creating its temporary profile — spawn error, Firefox
exiting immediately, the debug port never opening — it removes that profile
directory again (iter-175), so a failed launch costs no disk. A directory passed
via --profile is yours and is never removed. `launch` also reclaims temporary
profiles left by *earlier* failed launches: an unmarked one holding nothing but
`user.js` proves no Firefox ever opened it, so it goes without waiting out
FF_RDP_PROFILE_PRUNE_DAYS.

Examples:
  ff-rdp launch                          # launch with temp profile on port 6000
  ff-rdp launch --headless               # headless mode (no visible window)
  ff-rdp launch --headless               # again: exit 0, already_running: true
  ff-rdp launch --port 9222              # use a different debug port
  ff-rdp launch --launch-timeout 45      # allow 45 s for the debug port to open
  ff-rdp launch --auto-consent           # install the Consent-O-Matic extension
  ff-rdp launch --profile ~/my-prof      # reuse an existing profile
  ff-rdp launch --headless --window-size 600x800   # true viewport, >= floor
  ff-rdp launch --headless --window-size 390x844   # below floor — clamps to ~500, warns

--auto-consent installs the Consent-O-Matic extension into the profile so it
CAN dismiss cookie banners it recognizes once a page loads — but `launch`
returns before any page loads, so its
`results.auto_consent_extension_installed` field (iter-144) only reports
that the extension was installed, never that anything was actually
dismissed (a prior `auto_consent: true` field made that false claim — see
kb/iterations/iteration-142-session-hygiene.md). For a real dismiss
attestation after navigating, use `ff-rdp navigate --auto-consent` or
`ff-rdp consent accept`, both of which report `results.consent = {\"cmp\":
..., \"action\": ...}` (`action` is `\"accepted\"` only when a control was
actually clicked).

Output: {\"results\": {\"pid\": N, \"host\": \"...\", \"port\": N, \"headless\": bool, \"profile\": \"...\", \"profile_path\": \"...\", \"temp_profile\": bool, \"auto_consent_extension_installed\": bool, \"window_size\": {\"requested\": {\"width\": N, \"height\": N}, \"below_floor\": bool}|null, \"warnings\"?: [...]}, \"total\": 1, \"meta\": {\"firefox\": \"...\", \"launch_wait_secs\": N, \"replaced\"?: {\"stopped\": bool, \"pid\": N}}}"
    )]
    Launch(LaunchArgs),
    /// Install Claude Code skill files to the user or project filesystem
    #[command(
        name = "install-skill",
        long_about = "Install bundled Claude Code skill files to the filesystem.

ff-rdp ships with Claude Code skills (e.g. ff-rdp-debug) that can be installed
into ~/.claude/skills/ (--user, default) or <git-root>/.claude/skills/ (--project).

The home directory for --user is resolved from the HOME env var, then
USERPROFILE (Windows), then the OS home-directory API — so setting HOME (or
USERPROFILE on Windows) redirects the install location on every platform.

Every installed file gets a managed-by header so re-installs can detect versions
and skip unchanged files. Files without that header are never overwritten unless
--force is passed.

Examples:
  ff-rdp install-skill --claude                  # install all skills to ~/.claude/skills/
  ff-rdp install-skill --claude ff-rdp-debug     # install one skill
  ff-rdp install-skill --claude --project        # install into <git-root>/.claude/skills/
  ff-rdp install-skill --claude --dry-run        # preview what would be written
  ff-rdp install-skill --claude --list           # list skills and installed status
  ff-rdp install-skill --claude --uninstall ff-rdp-debug
  ff-rdp install-skill --claude --from-dir ./my-skill ff-rdp-debug  # install from disk

Output (install):  {\"results\": [{\"skill\": \"...\", \"path\": \"...\", \"action\": \"written|skipped|would-write\"}], \"total\": N, \"meta\": {...}}
Output (--list):   {\"results\": [{\"name\": \"...\", \"version\": \"...\", \"installed\": bool, \"installed_path\": \"...\"}], \"total\": N, \"meta\": {...}}
Output (--uninstall): {\"results\": {\"uninstalled\": bool, \"path\": \"...\"}, \"total\": 1, \"meta\": {...}}"
    )]
    InstallSkill(InstallSkillArgs),

    /// Diagnose the connection: daemon, port owner, RDP handshake, tabs, version
    #[command(long_about = "Diagnose the ff-rdp connection top-to-bottom.

Probes (in order):
  1. Daemon registry — is a daemon running and reachable?
  2. Port owner     — who is listening on --port (PID, process, uptime)?
  3. RDP handshake  — can we receive a Firefox greeting?
  4. Tabs           — how many tabs are exposed by the connected target?
  5. Firefox version — within the tested compatibility range?

Run this whenever a command fails with \"no tabs available\", a connection
timeout, or any error you don't immediately understand. Exits 0 when every
probe passes, 1 otherwise.

Output: {\"results\": [{\"name\": \"...\", \"status\": \"pass|warn|fail\", \"detail\": \"...\", \"hint\": \"...\"}], \"total\": N, \"meta\": {...}}")]
    Doctor,

    /// Inspect and clean up ff-rdp's ephemeral Firefox profile directories
    #[command(
        long_about = "Inspect and clean up the ephemeral Firefox profile directories ff-rdp creates for itself under its secure per-user profile root (see `ff-rdp launch`).

`daemon stop` and `launch` already clean these up automatically (see their --help). This command is the manual escape hatch for whatever they missed — e.g. after a crash or `kill -9` that never reached `daemon stop`.

Only directories matching the `ff-rdp-profile-<16 alphanumeric chars>` naming convention are ever listed or removed — a `--profile` directory you passed to `launch` yourself is never touched, even if it happens to live under the same root.

Examples:
  ff-rdp profiles list
  ff-rdp profiles prune                    # remove entries older than 7 days (default)
  ff-rdp profiles prune --older-than 24h
  ff-rdp profiles prune --all --dry-run    # preview removing everything
  ff-rdp profiles prune --all              # remove every managed entry now

Output (list):  {\"results\": {\"path\": \"...\", \"count\": N, \"total_size_bytes\": N, \"oldest_mtime\": \"...\"|null}, \"total\": 1, \"meta\": {...}}
Output (prune): {\"results\": {\"path\": \"...\", \"would_remove\": [...], \"removed\": [...], \"dry_run\": bool}, \"total\": N, \"meta\": {...}}"
    )]
    Profiles {
        #[command(subcommand)]
        profiles_command: ProfilesCommand,
    },

    /// Execute a script file (JSON or YAML)
    #[command(long_about = "Execute a script file (JSON or YAML).

Each step is dispatched in-process and emits one NDJSON line to stdout:
  {\"step\": N, \"verb\": \"...\", \"ok\": true, \"results\": {...}, \"elapsed_ms\": N}

A final summary line is emitted:
  {\"summary\": true, \"ok\": true, \"total\": N, \"failed\": 0, \"total_elapsed_ms\": N}

Examples:
  ff-rdp run login.json
  ff-rdp run login.yaml --vars email=user@example.com --vars password=secret
  ff-rdp run login.json --dry-run
  ff-rdp run login.json --continue-on-failure
  ff-rdp run login.json --record session.json

NETWORK ASSERTIONS (iter-181): when a script contains an `assert_network` (or a
`run:` step that might), `run` opens one extra connection before the first step
and holds a `network-event` subscription on it for the whole playbook.  A
request fired by step N is therefore still visible to an `assert_network` at
step N+1, and a step `timeout` bounds only how long it waits for a request
still in flight.  If that subscription cannot be armed, `run` says so on stderr
and each assertion falls back to per-step arming, which can miss a request that
completed before the step started; the step's `diagnostics.subscription` then
reads `step` rather than `playbook`.  Under the daemon nothing changes — it
already holds a standing subscription.")]
    Run(RunArgs),

    /// Record browser commands to a replayable script
    #[command(long_about = "Record browser commands to a replayable script.

Subcommands:
  record start <output.json>   Start recording to the given file
  record stop                  Stop the active recording and print the file path
  record status                Show whether a recording is active

Examples:
  ff-rdp record start session.json
  ff-rdp navigate https://example.com
  ff-rdp click \"button[type=submit]\"
  ff-rdp record stop")]
    Record {
        #[command(subcommand)]
        record_command: RecordCommand,
    },

    /// Crawl a site and produce a page-map index (JSON) for use by scripts
    #[command(long_about = "Crawl a site from a base URL and emit a page-map index.

The page-map is a pre-computed site index that lets an agent skip the
\"discovery\" turns (what's on this page? what forms are here?) by reading
a single JSON file before starting a script run.

The crawl reuses the current daemon tab's session cookies so logged-in
areas are crawled automatically. For CI or headless flows, supply
--login-script to authenticate first.

`ff-rdp index --check` re-crawls and reports drifted selectors/routes
against an existing map — useful in CI to detect UI changes.

Examples:
  ff-rdp index                                     # crawl current tab origin
  ff-rdp index https://example.com --depth 3
  ff-rdp index --out map.json --max-pages 100
  ff-rdp index --login-script login.json
  ff-rdp index --check --page-map .ffrdp/page-map.json --report drift.json
  ff-rdp index --format yaml --out map.yaml

Output: writes page-map JSON/YAML to --out (default: .ffrdp/page-map.json)
        and emits a summary to stdout:
        {\"results\": {\"pages\": N, \"forms\": N, \"api_routes\": N, \"out\": \"...\"}}")]
    Index(IndexArgs),

    /// Detect and accept a known cookie-consent / CMP overlay
    #[command(
        long_about = "Detect and accept a known cookie-consent-management-platform (CMP) overlay.

Complements `ff-rdp launch --auto-consent` (which installs the Consent-O-Matic
extension): Consent-O-Matic does not reliably record consent for the
Sourcepoint CMP in headless mode (dogfooding-session-62 finding 1). This
command is the CLI-native fallback — it first tries a table of known
same-origin (non-iframe) CMPs (e.g. BBC's own `#bbccookies-continue-button`,
iter-144), then enumerates the tab's frame targets (including cross-origin
iframes) and recognises a known iframe-hosted CMP by matching a frame's URL,
clicking that frame's \"accept all\" control directly.

Subcommands:
  consent accept   Detect a known CMP on the current tab and accept it

Output: {\"results\": {\"cmp\": \"sourcepoint\"|\"bbc\"|null, \"action\": \"accepted\"|null, \"status\": \"accepted\"|\"detected_not_actioned\"|\"no_cmp_detected\"}, \"total\": 1, \"meta\": {...}}
All three keys are always present — cmp/action are null/null when no known CMP was
found on the page, and `status` (iter-160) names which of the three outcomes it was.
`consent accept` exits 1 for the two non-accepting outcomes; see `consent accept --help`.

See also: `ff-rdp navigate --auto-consent` to run this automatically after navigating."
    )]
    Consent {
        #[command(subcommand)]
        consent_command: ConsentCommand,
    },

    /// Generate shell completions
    #[command(long_about = "Generate a shell completion script for ff-rdp.

Supported shells: bash, zsh, fish, elvish, powershell.

The script is written to stdout — no JSON envelope, just the raw completion
source for the requested shell. Pipe it to your shell's completion loader or
save it to a file in the appropriate completions directory.

Examples:
  eval \"$(ff-rdp completions zsh)\"                 # load into the current zsh session
  ff-rdp completions bash > /etc/bash_completion.d/ff-rdp
  ff-rdp completions fish > ~/.config/fish/completions/ff-rdp.fish

The deb/rpm packages already install bash/zsh/fish completions system-wide —
this command is mainly useful for shells outside those three, or for
generating a fresh script after upgrading ff-rdp.")]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(clap::Args)]
pub struct NavigateArgs {
    /// The URL to navigate to (positional, not a flag)
    pub url: String,
    /// Also capture network requests made during navigation
    #[arg(long)]
    pub with_network: bool,
    /// Total time limit for network event collection in milliseconds (--with-network only).
    /// Collection runs for this duration then returns all captured events.
    #[arg(long, default_value_t = 10000)]
    pub network_timeout: u64,
    /// After navigating, wait for this text to appear in the page's visible content. Runs after the navigation load event completes.
    #[arg(long, conflicts_with = "wait_selector")]
    pub wait_text: Option<String>,
    /// After navigating, wait for this CSS selector to match an element in the DOM. Runs after the navigation load event completes.
    #[arg(long, conflicts_with = "wait_text")]
    pub wait_selector: Option<String>,
    /// Timeout for the --wait-text/--wait-selector condition in milliseconds. If the condition is not met within this time, the command fails with an error showing the elapsed time.
    #[arg(long, default_value_t = 5000)]
    pub wait_timeout: u64,
    /// Skip waiting for the new document to commit; return immediately after the navigate request is acknowledged (pre-61g fire-and-forget behaviour).
    #[arg(long)]
    pub no_wait: bool,
    /// Readiness level to wait for before returning: `loading` (dom-loading), `interactive` (dom-interactive), or `complete` (dom-complete, default). Ignored when `--with-network` is set: that mode uses the network-drain settle as its commit signal.
    #[arg(long, value_name = "LEVEL", default_value = "complete", value_enum)]
    pub wait: crate::commands::navigate::WaitLevel,
    /// After the document commits, additionally wait for a predicate. Accepts selector:<css>, text:<substr>, url:<regex>, or gone:<css>.
    /// Uses the --timeout budget. On failure surfaces a descriptive error.
    #[arg(long, value_name = "PREDICATE")]
    pub wait_for: Vec<String>,
    /// Strategy for waiting for navigation readiness.
    /// `both` (default): wait on document-event resources while also probing
    ///         `document.readyState` in-loop, returning as soon as either
    ///         signals complete — so a page that finished loading is not held
    ///         up by a `dom-complete` event that never fires (e.g. FF152); a
    ///         readystate poll runs as a final fallback if events time out.
    /// `events`: wait for document-event resources (dom-complete) only.
    /// `readystate`: poll `document.readyState == "complete"` until timeout.
    #[arg(long, value_name = "STRATEGY", default_value = "both", value_enum)]
    pub wait_strategy: crate::commands::navigate::WaitStrategy,
    /// After the document commits, detect and accept a known cookie-consent
    /// overlay (see `ff-rdp consent accept`). Best-effort: a detection
    /// failure is reported as a warning, not a navigate failure. Adds
    /// `results.consent = {"cmp": ..., "action": ...}` (both keys always
    /// present).
    ///
    /// Combines with --with-network (iter-159): the consent click happens while
    /// the network capture is still open, and a short follow-up drain collects
    /// the requests the dismissal unblocks. Before iter-159 the two flags were
    /// mutually exclusive, so a consent-walled site — the only kind where you
    /// need both — forced a choice between them.
    #[arg(long)]
    pub auto_consent: bool,
    /// After the action completes, embed the resulting page under
    /// `results.page`: `headings`, `landmarks`, and `interactive` elements —
    /// each with a `ref` you can pass straight to `click --ref` / `type --ref`
    /// (daemon mode; see `meta.page_refs_registered`).
    ///
    /// Ordering: the page is collected LAST — after the command's own
    /// readiness wait, after `--wait-for`/`--settle` where those apply, and
    /// after waiting for `document.readyState == "complete"` (bounded by
    /// --timeout). So it describes the document this command produced, not the
    /// one it started from. `meta.page_ready` is false if that wait timed out.
    ///
    /// Same view and same JSON shape as `ff-rdp a11y summary`, capped at 50
    /// interactive elements.
    #[arg(long)]
    pub with_page: bool,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("eval_source").required(true).multiple(false).args(["script", "file", "stdin"])))]
pub struct EvalArgs {
    /// JavaScript expression to evaluate (positional)
    pub script: Option<String>,
    /// Read JavaScript source from a file
    #[arg(long, value_name = "PATH")]
    pub file: Option<String>,
    /// Read JavaScript source from stdin until EOF
    #[arg(long)]
    pub stdin: bool,
    /// JSON.stringify() the result to get actual values instead of actor grips
    /// (accepts multi-statement scripts, same as bare eval)
    #[arg(long)]
    pub stringify: bool,
    /// Share one scope across calls: send a plain synchronous script to the
    /// tab's global lexical environment instead of wrapping it per call, so
    /// `const`/`let`/`class` persist into the next `eval` (iter-165 — this
    /// was a no-op from iter-93 to iter-164). Does not affect --stringify or
    /// `await` scripts, whose wrap is a syntactic necessity.
    #[arg(long)]
    pub no_isolate: bool,
    /// Evaluate inside a specific frame/iframe actor (iter-77 S3).
    ///
    /// Pass the frame actor ID — e.g. obtained from a `watcher`
    /// `target-available-form` event with `targetType=frame`.  Wires the
    /// spec-declared `frameActor` field of `evaluateJSAsync`
    /// (devtools/shared/specs/webconsole.js:149-164).
    #[arg(long, value_name = "ACTOR")]
    pub frame: Option<String>,
    /// Pre-bind `$0` to a DOM node actor before evaluating (iter-77 S3).
    ///
    /// Maps to `selectedNodeActor` in the `evaluateJSAsync` request.
    #[arg(long, value_name = "ACTOR")]
    pub node: Option<String>,
    /// Scope the eval to a specific inner-window ID (iter-77 S3).
    ///
    /// Maps to `innerWindowID` in the `evaluateJSAsync` request.
    #[arg(long, value_name = "ID")]
    pub inner_window: Option<u64>,
    /// When the result is a JSON-encoded string for an object or array, parse it
    /// on the client and replace `results` with the structured value.  Pairs
    /// naturally with `--stringify` and with scripts that already return
    /// `JSON.stringify(...)`.  Non-JSON strings are left unchanged.
    #[arg(long)]
    pub unwrap: bool,
}

// ---------------------------------------------------------------------------
// --query / --query-regex (iter-211 Theme A)
// ---------------------------------------------------------------------------

/// Compile a `--query-regex` value, so an invalid pattern is a clap usage
/// error (exit 2) rather than a runtime failure after a browser round-trip.
fn parse_query_regex(raw: &str) -> Result<Regex, String> {
    Regex::new(raw).map_err(|e| format!("invalid regular expression: {e}"))
}

/// The `--query` / `--query-regex` pair, flattened into every read command
/// that supports filtering (`page-text`, `snapshot`, `a11y summary`, `dom`).
///
/// One struct rather than four copies so the flag names, the mutual
/// exclusion, and the help text cannot drift apart between commands — a
/// recipe written against one command's `--query` works on all of them.
#[derive(clap::Args, Clone, Debug, Default)]
#[command(group(ArgGroup::new("query_filter").required(false).multiple(false).args(["query", "query_regex"])))]
pub struct QueryArgs {
    /// Return only the parts of the output containing TEXT
    /// (case-insensitive substring). `meta.matches` reports how many hits
    /// there were, so a caller can tell "no matches" from "filtered down to
    /// nothing by another flag".
    #[arg(long, value_name = "TEXT")]
    pub query: Option<String>,
    /// Like --query, but PATTERN is a regular expression (Rust `regex`
    /// syntax). An invalid pattern is a usage error (exit 2).
    #[arg(long, value_name = "PATTERN", value_parser = parse_query_regex)]
    pub query_regex: Option<Regex>,
}

#[derive(clap::Args)]
pub struct PageTextArgs {
    #[command(flatten)]
    pub query: QueryArgs,
    /// Lines of context to keep either side of each --query match (default 2).
    #[arg(long, value_name = "N", default_value_t = 2, requires = "query_filter")]
    pub context: usize,
    /// Maximum characters of page text to return (default 8000).
    ///
    /// `page-text` was the only read command with no size cap, which is why
    /// agents piped it through `head -100` and lost the answer further down
    /// the page (iter-211 Theme B). `meta.total_chars` always reports the
    /// full length and `meta.truncated` says whether anything was cut.
    /// `0` is rejected — an unreachable cap is a bug, not a request.
    #[arg(long, value_name = "N", default_value_t = 8000)]
    pub max_chars: usize,
    /// Return the whole page text, ignoring --max-chars.
    #[arg(long, conflicts_with = "max_chars")]
    pub full: bool,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("dom_target").required(false).multiple(false).args(["selector", "ref_id"])))]
pub struct DomArgs {
    #[command(subcommand)]
    pub dom_command: Option<DomCommand>,

    /// CSS selector to match elements
    #[arg(group = "dom_target")]
    pub selector: Option<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "dom_target")]
    pub ref_id: Option<String>,
    /// Output outer HTML (default)
    #[arg(long, group = "output_mode")]
    pub outer_html: bool,
    /// Output inner HTML
    #[arg(long, group = "output_mode")]
    pub inner_html: bool,
    /// Output text content only
    #[arg(long, group = "output_mode")]
    pub text: bool,
    /// Output element attributes as JSON objects
    #[arg(long, group = "output_mode")]
    pub attrs: bool,
    /// Output both text content and attributes per element
    #[arg(long, group = "output_mode")]
    pub text_attrs: bool,
    /// Return only the count of matching elements
    #[arg(long, group = "output_mode")]
    pub count: bool,
    /// Return just the first match as a single value (or null) instead of an array.
    /// Provided for callers who want the legacy pre-iter-61i single-element shape.
    /// Mutually exclusive with --count.
    #[arg(long, conflicts_with = "count")]
    pub first: bool,
    /// Attach computed CSS values for each match (comma-separated property list).
    /// Each result element gets an extra `style` field with the named getComputedStyle
    /// values, e.g. `--include-style color,display`. Capped by `--include-style-limit`.
    #[arg(long, value_name = "PROPS")]
    pub include_style: Option<String>,
    /// Cap the number of matches that receive computed styles when
    /// `--include-style` is set. Default 50. Elements beyond the cap omit the
    /// `style` field and the response sets `meta.style_truncated: true`.
    #[arg(
        long,
        value_name = "N",
        default_value_t = 50,
        requires = "include_style"
    )]
    pub include_style_limit: usize,
    /// Keep only the matched elements whose accessible name / text matches
    /// (iter-211 Theme A). A selector-only call is unchanged.
    #[command(flatten)]
    pub query: QueryArgs,
}

#[derive(clap::Args)]
pub struct ConsoleArgs {
    /// Filter by log level (error, warn, info, log, debug)
    #[arg(long)]
    pub level: Option<String>,
    /// Filter by message content (regex pattern)
    #[arg(long)]
    pub pattern: Option<String>,
    /// Stream console messages in real time (connection closed or Ctrl-C to stop)
    #[arg(long)]
    pub follow: bool,
}

#[derive(clap::Args)]
pub struct NetworkArgs {
    /// Filter by URL pattern (substring match)
    #[arg(long)]
    pub filter: Option<String>,
    /// Filter by HTTP method (GET, POST, etc.)
    #[arg(long)]
    pub method: Option<String>,
    /// Stream network events in real time (Ctrl-C to stop)
    #[arg(long)]
    pub follow: bool,
    /// Include request and response headers in --detail output.
    /// Headers are fetched per-entry from the NetworkEventActor (watcher source
    /// only). When the source is performance-api, a per-entry note is emitted
    /// explaining why headers are missing; use --with-network to engage the
    /// watcher and make headers available.
    #[arg(long)]
    pub headers: bool,
    /// Attach per-request TLS/certificate detail (protocolVersion, cipherSuite,
    /// cert summary, hsts, weaknessReasons) to each captured request. HTTPS
    /// requests get a `security` object; plain-HTTP requests get `security:
    /// null` and contribute to a top-level `insecure_requests` count. Like
    /// --headers this is a watcher-source-only, per-entry pull (performance-api
    /// source has no security info); implies detail output.
    #[arg(long)]
    pub security: bool,
    /// Scope the result to a specific navigation window (daemon mode only).
    /// -1 = current navigation (default), -2 = one back, 'all' = full cumulative buffer.
    /// Positive integers are treated as 1-based indices from the oldest boundary.
    ///
    /// `allow_hyphen_values` lets the negative forms be passed as either
    /// `--since -1` or `--since=-1` without clap mistaking `-1` for a flag.
    #[arg(long, value_name = "NAV_INDEX_OR_ALL", allow_hyphen_values = true)]
    pub since: Option<String>,
    /// Which capture source produces the rows.
    ///
    /// `watcher` (the default) reads the RDP resource watcher, the only source
    /// that carries `method`, `status`, `content_type` and `transfer_size`.
    /// `performance-api` evaluates `performance.getEntriesByType('resource')`
    /// in the page instead — fewer fields, but it can see requests that
    /// finished before ff-rdp connected.
    ///
    /// There is no automatic substitution: an empty watcher buffer is reported
    /// as zero watcher rows, never silently swapped for a different dataset
    /// with different fields (iter-159). `auto` is accepted as a deprecated
    /// alias of `watcher`.
    #[arg(long, value_enum, default_value_t = NetworkSource::Watcher)]
    pub source: NetworkSource,
}

/// Capture source for `ff-rdp network` (iter-137 Theme C, narrowed in iter-159).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum NetworkSource {
    /// The watcher/daemon resource buffer. Reports zero rows rather than
    /// silently substituting a different dataset.
    ///
    /// `auto` is a deprecated alias: until iter-159 it meant "watcher if
    /// non-empty, else performance-api", and that silent substitution is what
    /// hid a daemon watcher that had been delivering nothing since iter-137.
    #[value(alias = "auto")]
    Watcher,
    /// Only `performance.getEntriesByType('resource')`, evaluated in the page.
    /// Identical in daemon and direct mode; no headers or security detail.
    #[value(name = "performance-api")]
    PerformanceApi,
}

#[derive(clap::Args)]
pub struct PerfArgs {
    #[command(subcommand)]
    pub perf_command: Option<PerfCommand>,

    /// Performance entry type to query (resource, navigation, paint, lcp, cls, longtask)
    #[arg(long = "type", default_value = "resource")]
    pub entry_type: String,

    /// Filter by URL substring (resource/navigation types)
    #[arg(long)]
    pub filter: Option<String>,

    /// Group results by a field (e.g., "domain" for resource entries)
    #[arg(long)]
    pub group_by: Option<String>,
}

#[derive(clap::Args)]
pub struct ScreenshotArgs {
    /// Output file path
    #[arg(long, short, conflicts_with = "base64")]
    pub output: Option<String>,
    /// Return the screenshot as base64 PNG data in JSON output instead of saving to a file
    #[arg(long, conflicts_with = "output")]
    pub base64: bool,
    /// Capture the entire scrollable page (document.scrollingElement.scrollHeight)
    #[arg(long, conflicts_with = "viewport_height")]
    pub full_page: bool,
    /// Capture at this explicit height (pixels) instead of the viewport height
    #[arg(long, value_name = "PX", conflicts_with = "full_page")]
    pub viewport_height: Option<u32>,
    /// Restrict output to paths under this directory (rejects path traversal)
    #[arg(long, value_name = "DIR")]
    pub output_root: Option<std::path::PathBuf>,
    /// Attempt to receive the screenshot via a bulk-frame streaming path.
    ///
    /// When set, the command sends the capture request and then tries to
    /// read the response as a bulk binary frame via
    /// `Transport::recv_bulk_with_handler` (no full base64 allocation in
    /// memory).  If Firefox responds with a JSON frame (the current
    /// behaviour for all Firefox versions), the command falls back to the
    /// standard base64 path transparently.
    #[arg(long)]
    pub bulk: bool,
    /// Batch-capture a screenshot at this exact `WxH` pixel size via a
    /// one-shot `firefox --headless --window-size --screenshot` subprocess
    /// — the only path to a TRUE sub-500px mobile raster (bypasses the live
    /// viewport floor `launch --window-size` hits below ~500px). Runs in a
    /// fresh scratch profile separate from the live RDP session/daemon: the
    /// current tab's URL is re-navigated from scratch, so cookies/localStorage/
    /// session state from the live tab are NOT carried over. No density knob:
    /// `layout.css.devPixelsPerPx` was tested against this exact capture path
    /// and found to have zero effect on the output raster (Firefox 153.0.3).
    #[arg(long, value_name = "WxH", conflicts_with_all = ["full_page", "viewport_height"])]
    pub window_size: Option<String>,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("click_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
pub struct ClickArgs {
    /// CSS selector of the element to click (positional, or use --selector)
    #[arg(group = "click_target")]
    pub selector_pos: Option<String>,
    /// CSS selector of the element to click (flag form)
    #[arg(long = "selector", value_name = "SELECTOR", group = "click_target")]
    pub selector_flag: Option<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "click_target")]
    pub ref_id: Option<String>,
    /// After clicking, wait for a network request whose URL contains this pattern.
    /// Returns the matched request record in the output.
    #[arg(long, value_name = "PATTERN")]
    pub wait_for_network: Option<String>,
    /// Timeout in milliseconds for --wait-for-network (default: global --timeout)
    #[arg(long, value_name = "MS", requires = "wait_for_network")]
    pub network_timeout: Option<u64>,
    /// Skip auto-wait and click immediately (reverts to pre-iter-59 fire-and-forget)
    #[arg(long)]
    pub no_wait: bool,
    /// Event dispatch mode: pointer (default), legacy (mouse events only), click-only
    #[arg(long, default_value = "pointer", value_name = "MODE")]
    pub dispatch: String,
    /// After clicking, wait for this condition. Repeatable. Forms: selector:<css>, text:<substr>, url:<regex>, gone:<css>
    #[arg(long, value_name = "PREDICATE", action = clap::ArgAction::Append)]
    pub wait_for: Vec<String>,
    /// Timeout in milliseconds for --wait-for predicates (default: same as --timeout)
    #[arg(long, value_name = "MS")]
    pub wait_for_timeout: Option<u64>,
    /// After clicking, wait for network and DOM to idle (no XHR/fetch for 500ms, no DOM mutations for 200ms)
    #[arg(long)]
    pub settle: bool,
    /// Click inside the frame whose URL contains this substring, skipping the
    /// top-level attempt and the frame scan (e.g. --frame sourcepoint)
    #[arg(long, value_name = "URL_SUBSTRING")]
    pub frame: Option<String>,
    /// When the selector matches more than one element, click the first
    /// *visible* one instead of blindly taking DOM-order index 0. Mutually
    /// exclusive with --index.
    #[arg(long, conflicts_with = "index")]
    pub visible: bool,
    /// When the selector matches more than one element, click the Nth match
    /// (0-based), regardless of visibility. Mutually exclusive with --visible.
    #[arg(long, value_name = "N", conflicts_with = "visible")]
    pub index: Option<usize>,
    /// After the action completes, embed the resulting page under
    /// `results.page`: `headings`, `landmarks`, and `interactive` elements —
    /// each with a `ref` you can pass straight to `click --ref` / `type --ref`
    /// (daemon mode; see `meta.page_refs_registered`).
    ///
    /// Ordering: the page is collected LAST — after the command's own
    /// readiness wait, after `--wait-for`/`--settle` where those apply, and
    /// after waiting for `document.readyState == "complete"` (bounded by
    /// --timeout). So it describes the document this command produced, not the
    /// one it started from. `meta.page_ready` is false if that wait timed out.
    ///
    /// Same view and same JSON shape as `ff-rdp a11y summary`, capped at 50
    /// interactive elements.
    #[arg(long)]
    pub with_page: bool,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("type_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
pub struct TypeArgs {
    /// CSS selector of the input element (positional, or use --selector)
    #[arg(group = "type_target")]
    pub selector_pos: Option<String>,
    /// Text to type into the element (positional, or use --text)
    pub text_pos: Option<String>,
    /// CSS selector of the input element (flag form)
    #[arg(long = "selector", value_name = "SELECTOR", group = "type_target")]
    pub selector_flag: Option<String>,
    /// Text to type into the element (flag form)
    #[arg(long = "text", value_name = "TEXT")]
    pub text_flag: Option<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "type_target")]
    pub ref_id: Option<String>,
    /// Clear the element's current value before typing
    #[arg(long)]
    pub clear: bool,
    /// Skip auto-wait and type immediately (reverts to pre-iter-59 fire-and-forget)
    #[arg(long)]
    pub no_wait: bool,
    /// After typing, wait for this condition. Repeatable. Forms: selector:<css>, text:<substr>, url:<regex>, gone:<css>
    #[arg(long, value_name = "PREDICATE", action = clap::ArgAction::Append)]
    pub wait_for: Vec<String>,
    /// Timeout in milliseconds for --wait-for predicates (default: same as --timeout)
    #[arg(long, value_name = "MS")]
    pub wait_for_timeout: Option<u64>,
    /// After typing, wait for network and DOM to idle
    #[arg(long)]
    pub settle: bool,
    /// When the selector matches more than one element, type into the first
    /// *visible* one instead of blindly taking DOM-order index 0. Mutually
    /// exclusive with --index.
    #[arg(long, conflicts_with = "index")]
    pub visible: bool,
    /// When the selector matches more than one element, type into the Nth
    /// match (0-based), regardless of visibility. Mutually exclusive with --visible.
    #[arg(long, value_name = "N", conflicts_with = "visible")]
    pub index: Option<usize>,
    /// After typing, press Enter on the element and — if that did not navigate
    /// and the element is inside a `<form>` — call `form.requestSubmit()`.
    ///
    /// The two-step shape is deliberate: the synthetic Enter is
    /// `isTrusted: false`, and Firefox does not perform its own implicit form
    /// submission for an untrusted key press, so on most forms Enter alone does
    /// nothing. Pages that DO handle Enter in script would be submitted twice
    /// by an unconditional `requestSubmit()`, so ff-rdp watches for a
    /// navigation in between and only falls back when none happened.
    ///
    /// Adds `submitted` (did anything submit) and `navigated` (did the URL
    /// change) to `results`, plus `method`: `enter`, `request_submit`,
    /// `no_form`, or `enter_prevented`.
    #[arg(long)]
    pub submit: bool,
    /// After the action completes, embed the resulting page under
    /// `results.page`: `headings`, `landmarks`, and `interactive` elements —
    /// each with a `ref` you can pass straight to `click --ref` / `type --ref`
    /// (daemon mode; see `meta.page_refs_registered`).
    ///
    /// Ordering: the page is collected LAST — after the command's own
    /// readiness wait, after `--wait-for`/`--settle` where those apply, and
    /// after waiting for `document.readyState == "complete"` (bounded by
    /// --timeout). So it describes the document this command produced, not the
    /// one it started from. `meta.page_ready` is false if that wait timed out.
    ///
    /// Same view and same JSON shape as `ff-rdp a11y summary`, capped at 50
    /// interactive elements.
    #[arg(long)]
    pub with_page: bool,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("condition").required(true).multiple(false)))]
pub struct WaitArgs {
    /// Wait until an element matching this CSS selector exists in the DOM
    #[arg(long, group = "condition")]
    pub selector: Option<String>,
    /// Wait until this text appears anywhere on the page
    #[arg(long, group = "condition")]
    pub text: Option<String>,
    /// Wait until this JavaScript expression returns a truthy value
    #[arg(long, group = "condition")]
    pub eval: Option<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "condition")]
    pub ref_id: Option<String>,
    /// Plain sleep for this many milliseconds — no condition, no Firefox
    /// connection, just a delay. For when you need to pace commands rather
    /// than wait for a specific page state (use --selector/--text/--eval/--ref
    /// instead whenever a real condition exists — a fixed sleep is always a
    /// guess). The legacy spelling `--time` is also accepted as a hidden
    /// alias (iter-142: this was the flag dogfooders reached for first).
    #[arg(
        long = "sleep-ms",
        alias = "time",
        value_name = "MS",
        group = "condition"
    )]
    pub sleep_ms: Option<u64>,
    /// Timeout in milliseconds before giving up (canonical flag — use this one).
    /// The legacy spelling `--wait-timeout` is also accepted as a hidden alias.
    /// Not used by --sleep-ms, which always runs for exactly its own duration.
    #[arg(long = "timeout-ms", alias = "wait-timeout", default_value_t = 5000)]
    pub wait_timeout: u64,
}

#[derive(clap::Args)]
pub struct CookiesArgs {
    /// Filter by cookie name (exact match)
    #[arg(long)]
    pub name: Option<String>,
    /// Also evaluate `document.cookie` and merge any entries not already
    /// present in the StorageActor reply (marked with `source: "document.cookie"`).
    /// Useful for cookies that lack a `Domain=` attribute and are not surfaced
    /// by `getStoreObjects`.
    ///
    /// This is enabled by default. Pass `--storage-only` to disable.
    #[arg(
        long,
        hide = true,
        default_value_t = false,
        conflicts_with = "storage_only"
    )]
    pub include_document_cookie: bool,
    /// Return only cookies from the StorageActor (skip `document.cookie` evaluation).
    /// Use this when you need the raw StorageActor view, e.g. to debug httpOnly cookies.
    #[arg(long)]
    pub storage_only: bool,
}

#[derive(clap::Args)]
pub struct StorageArgs {
    /// Storage type: "local" (or "localStorage") / "session" (or "sessionStorage")
    pub storage_type: String,
    /// Get a specific key only
    #[arg(long)]
    pub key: Option<String>,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("a11y_target").required(false).multiple(false).args(["selector", "ref_id"])))]
pub struct A11yArgs {
    #[command(subcommand)]
    pub a11y_command: Option<A11yCommand>,

    /// Maximum tree depth to traverse (default: 6)
    #[arg(long, default_value_t = 6)]
    pub depth: u32,
    /// Maximum total characters of text content to include (default: 50000)
    #[arg(long, default_value_t = 50000)]
    pub max_chars: u32,
    /// CSS selector to root the tree at a specific element
    #[arg(long, group = "a11y_target")]
    pub selector: Option<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "a11y_target")]
    pub ref_id: Option<String>,
    /// Only show interactive elements (buttons, links, inputs, etc.)
    #[arg(long)]
    pub interactive: bool,
    /// Surface only nodes that fail a basic WCAG audit (e.g. `<img>` without
    /// alt, form controls without an accessible name). Returns a flat array
    /// of violation records `{role, name?, selector, violation, severity}`
    /// instead of the full accessibility tree; empty when nothing critical.
    #[arg(long, conflicts_with = "interactive")]
    pub critical: bool,
    /// Opt in to the native platform accessibility tree: enables Firefox's
    /// accessibility service (if not already running), walks the real
    /// platform tree (roles like "document"/"paragraph" instead of the
    /// JS-derived "generic"), then restores the service to its previous
    /// state if this command was the one that turned it on. This is a
    /// browser-global, process-wide change while it runs — never the
    /// default (DEC-027) — and cannot be combined with `--selector`/`--ref`,
    /// which always use the JS-derived path. Failure to enable surfaces as
    /// an explicit error, never a silent fallback.
    #[arg(long, conflicts_with_all = ["selector", "ref_id"])]
    pub native: bool,
}

#[derive(clap::Args)]
pub struct ReloadArgs {
    /// Block until network is idle after reload
    #[arg(long, conflicts_with = "no_wait")]
    pub wait_idle: bool,
    /// Milliseconds of network inactivity that counts as idle (--wait-idle only)
    #[arg(long, default_value_t = 500, requires = "wait_idle")]
    pub idle_ms: u64,
    /// Maximum total milliseconds to wait for idle (--wait-idle only)
    #[arg(long, default_value_t = 10000, requires = "wait_idle")]
    pub reload_timeout: u64,
    /// Hard reload — bypass the HTTP cache (sends Firefox's `options.force`,
    /// equivalent to Cmd-Shift-R in the browser UI). Default is a soft reload.
    #[arg(long)]
    pub hard: bool,
    /// Dispatch the reload and return immediately without waiting for it to
    /// commit (iter-138 Theme E) — the same escape hatch `navigate` already
    /// has. Conflicts with --wait-idle (a different kind of wait).
    #[arg(long, conflicts_with = "wait_idle")]
    pub no_wait: bool,
    /// After the action completes, embed the resulting page under
    /// `results.page`: `headings`, `landmarks`, and `interactive` elements —
    /// each with a `ref` you can pass straight to `click --ref` / `type --ref`
    /// (daemon mode; see `meta.page_refs_registered`).
    ///
    /// Ordering: the page is collected LAST — after the command's own
    /// readiness wait, after `--wait-for`/`--settle` where those apply, and
    /// after waiting for `document.readyState == "complete"` (bounded by
    /// --timeout). So it describes the document this command produced, not the
    /// one it started from. `meta.page_ready` is false if that wait timed out.
    ///
    /// Same view and same JSON shape as `ff-rdp a11y summary`, capped at 50
    /// interactive elements.
    #[arg(long)]
    pub with_page: bool,
}

/// Shared args for `back`/`forward` (iter-138 Theme E).
#[derive(clap::Args)]
pub struct BackForwardArgs {
    /// Dispatch the navigation and return immediately without waiting for it
    /// to commit — the same escape hatch `navigate` already has.
    #[arg(long)]
    pub no_wait: bool,
    /// After the action completes, embed the resulting page under
    /// `results.page`: `headings`, `landmarks`, and `interactive` elements —
    /// each with a `ref` you can pass straight to `click --ref` / `type --ref`
    /// (daemon mode; see `meta.page_refs_registered`).
    ///
    /// Ordering: the page is collected LAST — after the command's own
    /// readiness wait, after `--wait-for`/`--settle` where those apply, and
    /// after waiting for `document.readyState == "complete"` (bounded by
    /// --timeout). So it describes the document this command produced, not the
    /// one it started from. `meta.page_ready` is false if that wait timed out.
    ///
    /// Same view and same JSON shape as `ff-rdp a11y summary`, capped at 50
    /// interactive elements.
    #[arg(long)]
    pub with_page: bool,
}

#[derive(clap::Args)]
pub struct InspectArgs {
    /// The actor ID of the object grip to inspect
    pub actor_id: String,
    /// Recursion depth for nested objects (default: 1)
    #[arg(long, default_value_t = 1)]
    pub depth: u32,
}

#[derive(clap::Args)]
pub struct SourcesArgs {
    /// Filter sources by URL substring
    #[arg(long)]
    pub filter: Option<String>,
    /// Filter sources by URL regex pattern
    #[arg(long)]
    pub pattern: Option<String>,
}

#[derive(clap::Args)]
pub struct SnapshotArgs {
    /// Maximum tree depth to traverse (default: 6). Alias: --max-depth.
    ///
    /// Every node marked `interactive: true` carries a `ref` handle usable with
    /// `click --ref` / `type --ref` (iter-210). Refs live in the daemon, so
    /// they appear only on the daemon route; `meta.refs_registered` says
    /// whether the ones in this output are usable, and a navigation clears
    /// them. For a much smaller orientation view — headings, landmarks and
    /// interactive elements only, also ref-carrying — use `a11y summary`.
    #[arg(long, default_value_t = 6)]
    pub depth: u32,
    /// Maximum tree depth to traverse (alias for --depth, matches `dom tree --max-depth` / CDP convention).
    /// Mutually exclusive with --depth. Must be ≥ 1.
    #[arg(long, value_name = "N", conflicts_with = "depth")]
    pub max_depth: Option<u32>,
    /// Maximum size, in bytes of serialized JSON, of the whole output tree
    /// (tags, attributes, and structure — not just leaf text content;
    /// default: 50000). `meta.truncated` reports whether anything was cut.
    #[arg(long, default_value_t = 50000)]
    pub max_chars: u32,
    /// Keep only the nodes whose text or attribute values match, plus their
    /// ancestors; everything else is pruned (iter-211 Theme A). The root
    /// stays `html`, so the path to each hit is still visible.
    #[command(flatten)]
    pub query: QueryArgs,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("geo_target").required(true).multiple(false).args(["selectors", "ref_id"])))]
pub struct GeometryArgs {
    /// One or more CSS selectors to query
    #[arg(group = "geo_target")]
    pub selectors: Vec<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "geo_target")]
    pub ref_id: Option<String>,
    /// Include hidden elements (zero-size, display:none, visibility:hidden, opacity:0).
    /// By default these are excluded.
    #[arg(long)]
    pub include_hidden: bool,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("resp_target").required(true).multiple(false).args(["selectors", "ref_id"])))]
pub struct ResponsiveArgs {
    /// One or more CSS selectors to query at each breakpoint
    #[arg(group = "resp_target")]
    pub selectors: Vec<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "resp_target")]
    pub ref_id: Option<String>,
    /// Comma-separated viewport widths in pixels
    #[arg(long, value_delimiter = ',', default_value = "320,768,1024,1440")]
    pub widths: Vec<u32>,
    /// Include hidden elements (zero-size, display:none, visibility:hidden, opacity:0).
    /// By default these are excluded.
    #[arg(long)]
    pub include_hidden: bool,
    /// Exit non-zero when the media-query self-check detects that the page's
    /// media queries did not flip to the requested width (layout-only
    /// emulation). Each breakpoint always carries a `media_query_check`
    /// object regardless; --strict turns any mismatch into a failure.
    #[arg(long)]
    pub strict: bool,
}

/// `prefers-color-scheme` simulation value for `emulate --color-scheme`.
///
/// Maps to the target-configuration actor's `colorSchemeSimulation` field:
/// `none` restores the system default.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ColorScheme {
    Light,
    Dark,
    None,
}

/// A generic on/off toggle used by several `emulate` flags
/// (`--print`, `--touch`, `--js`, `--offline`, `--cache`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

/// Arguments for `emulate` — one option per target-configuration field.
///
/// Every flag is optional; a call applies only the fields the user names.
/// `--reset` restores every field to its documented default and must be used
/// on its own (enforced at runtime, not by clap, so the error is descriptive).
#[derive(clap::Args)]
pub struct EmulateArgs {
    /// Override navigator.userAgent and the User-Agent request header
    #[arg(long, value_name = "STRING")]
    pub user_agent: Option<String>,
    /// Simulate prefers-color-scheme (none = system default)
    #[arg(long, value_enum, value_name = "SCHEME")]
    pub color_scheme: Option<ColorScheme>,
    /// Override window.devicePixelRatio (positive number, e.g. 2 for retina)
    #[arg(long, value_name = "FLOAT")]
    pub dppx: Option<f64>,
    /// Toggle @media print simulation
    #[arg(long, value_enum, value_name = "ON_OFF")]
    pub print: Option<OnOff>,
    /// Toggle touch-event simulation
    #[arg(long, value_enum, value_name = "ON_OFF")]
    pub touch: Option<OnOff>,
    /// Enable/disable JavaScript (server reloads the document on change)
    #[arg(long, value_enum, value_name = "ON_OFF")]
    pub js: Option<OnOff>,
    /// Take the tab offline (navigator.onLine === false, network requests fail)
    #[arg(long, value_enum, value_name = "ON_OFF")]
    pub offline: Option<OnOff>,
    /// Toggle the HTTP cache ('off' disables it — maps to cacheDisabled=true)
    #[arg(long, value_enum, value_name = "ON_OFF")]
    pub cache: Option<OnOff>,
    /// Restore every emulation field to its default (use on its own)
    #[arg(long)]
    pub reset: bool,
}

/// Network-throttling profile for `throttle` (positional).
///
/// Maps to the network-parent actor's `setNetworkThrottling` options.
/// `off` clears any active throttling (Firefox restores full speed).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ThrottleProfileArg {
    /// Slow 3G: ~400 kbit/s, 400 ms round-trip latency.
    #[value(name = "slow-3g")]
    Slow3g,
    /// Fast 3G: ~1.6 Mbit/s, 150 ms round-trip latency.
    #[value(name = "fast-3g")]
    Fast3g,
    /// Clear throttling and restore full-speed network behaviour.
    Off,
    /// Report the profile last applied via the daemon (iter-131 Theme D).
    /// Read-only: does not touch throttling/blocking. Firefox's
    /// network-parent actor has no getter, so this recalls client-side
    /// bookkeeping rather than querying the browser — see `throttle --help`.
    Status,
}

/// Arguments for `throttle` — network throttling and URL blocking.
///
/// The positional PROFILE sets a throttling tier (or `off`); `--block` replaces
/// the URL block-list. At least one must be supplied. The envelope echoes the
/// active profile and block-list so scripts can confirm what was applied.
#[derive(clap::Args)]
pub struct ThrottleArgs {
    /// Throttling profile: slow-3g, fast-3g, off (clears throttling), or
    /// status (read-only: reports the profile last applied via the daemon)
    #[arg(value_enum, value_name = "PROFILE")]
    pub profile: Option<ThrottleProfileArg>,
    /// Block requests whose URL matches PATTERN (repeatable; substring/glob
    /// match). Pass `--block` with no value list, or an empty `--block ''`,
    /// to clear the block-list.
    #[arg(long, value_name = "PATTERN", action = clap::ArgAction::Append)]
    pub block: Vec<String>,
    /// Clear the URL block-list (equivalent to `--block` with no patterns)
    #[arg(long, conflicts_with = "block")]
    pub unblock: bool,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("computed_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
pub struct ComputedArgs {
    /// CSS selector to match elements (positional, or use --selector)
    #[arg(group = "computed_target")]
    pub selector_pos: Option<String>,
    /// CSS selector to match elements (flag form)
    #[arg(long = "selector", value_name = "SELECTOR", group = "computed_target")]
    pub selector_flag: Option<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "computed_target")]
    pub ref_id: Option<String>,
    /// Return only specific property values (repeatable: --prop color --prop font-size).
    /// Also accepts CSS custom properties like --prop=--bg-color.
    /// Comma-separated lists are also accepted: --prop color,font-size,--bg-color
    #[arg(long, value_name = "NAME", action = clap::ArgAction::Append)]
    pub prop: Vec<String>,
    /// Include every resolved property, not just non-default values
    #[arg(long, conflicts_with = "prop")]
    pub all: bool,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("styles_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
pub struct StylesArgs {
    /// CSS selector to match the element (positional, or use --selector)
    #[arg(group = "styles_target")]
    pub selector_pos: Option<String>,
    /// CSS selector to match the element (flag form)
    #[arg(long = "selector", value_name = "SELECTOR", group = "styles_target")]
    pub selector_flag: Option<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "styles_target")]
    pub ref_id: Option<String>,
    /// Show applied CSS rules with source locations instead of computed styles
    #[arg(long, group = "style_mode")]
    pub applied: bool,
    /// Show box model layout (margin/border/padding/content) instead of computed styles
    #[arg(long, group = "style_mode")]
    pub layout: bool,
    /// Comma-separated list of CSS property names to include (computed mode only)
    #[arg(long, value_delimiter = ',', conflicts_with_all = ["applied", "layout"])]
    pub properties: Option<Vec<String>>,
    /// When the selector matches more than one element, inspect the first
    /// *visible* one instead of blindly taking DOM-order index 0. Mutually
    /// exclusive with --index.
    #[arg(long, conflicts_with = "index")]
    pub visible: bool,
    /// When the selector matches more than one element, inspect the Nth match
    /// (0-based), regardless of visibility. Mutually exclusive with --visible.
    #[arg(long, value_name = "N", conflicts_with = "visible")]
    pub index: Option<usize>,
}

#[derive(clap::Args)]
#[command(group(ArgGroup::new("cascade_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
pub struct CascadeArgs {
    /// CSS selector to match the element (positional, or use --selector)
    #[arg(group = "cascade_target")]
    pub selector_pos: Option<String>,
    /// CSS selector to match the element (flag form)
    #[arg(long = "selector", value_name = "SELECTOR", group = "cascade_target")]
    pub selector_flag: Option<String>,
    /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
    #[arg(long = "ref", value_name = "REF_ID", group = "cascade_target")]
    pub ref_id: Option<String>,
    /// CSS property to explain (e.g. `--prop display`).  Defaults to all
    /// properties declared on the element.
    #[arg(long, value_name = "NAME", conflicts_with = "all")]
    pub prop: Option<String>,
    /// Explain every property declared on the element (the default).
    #[arg(long)]
    pub all: bool,
    /// Dump the raw PageStyle `getApplied` reply to stderr before parsing.
    /// Use to diagnose field-name drift between ff-rdp and Firefox.
    #[arg(long)]
    pub debug_raw: bool,
}

#[derive(clap::Args)]
pub struct LaunchArgs {
    /// Run Firefox in headless mode
    #[arg(long)]
    pub headless: bool,
    /// Path to a Firefox profile directory
    #[arg(long, conflicts_with = "temp_profile")]
    pub profile: Option<String>,
    /// Create a temporary profile for a clean session
    #[arg(long, conflicts_with = "profile")]
    pub temp_profile: bool,
    /// Override the debug server port (defaults to --port value)
    #[arg(long)]
    pub debug_port: Option<u16>,
    /// Install the Consent-O-Matic extension so it can auto-dismiss cookie
    /// consent banners once a page loads. Reported back as
    /// `results.auto_consent_extension_installed` — `launch` returns
    /// before any page loads, so it cannot itself attest that a banner was
    /// dismissed; use `navigate --auto-consent` or `consent accept` for
    /// that (see their `results.consent` field).
    #[arg(long)]
    pub auto_consent: bool,
    /// Set the initial window size as `WxH` pixels (forwarded to Firefox as
    /// `-width`/`-height`). True live-viewport emulation (real innerWidth,
    /// real media queries) only above the ~500px floor documented in
    /// `--help`; for a true sub-500px raster use `screenshot --window-size`
    /// instead.
    #[arg(long, value_name = "WxH")]
    pub window_size: Option<String>,
    /// If the debug port is already occupied, stop the prior Firefox instance
    /// gracefully (SIGTERM → SIGKILL after 2 s) and then launch a fresh one.
    /// Alias: --force.
    #[arg(long)]
    pub replace: bool,
    /// Alias for --replace (stop the prior instance and relaunch).
    #[arg(long, hide = true)]
    pub force: bool,
    /// Seconds to wait for Firefox to open its debug port after spawning
    /// (default 30, or `FF_RDP_LAUNCH_TIMEOUT_SECS`). This is NOT the global
    /// `--timeout`, which is a per-socket-operation deadline. Reported back as
    /// `meta.launch_wait_secs`.
    #[arg(long, value_name = "SECS")]
    pub launch_timeout: Option<u64>,
}

#[derive(clap::Args)]
pub struct RunArgs {
    /// Path to the script file (.json or .yaml)
    pub script: std::path::PathBuf,
    /// Ad-hoc variable overrides (format: KEY=VALUE)
    #[arg(long = "vars", value_name = "KEY=VALUE", action = clap::ArgAction::Append)]
    pub vars: Vec<String>,
    /// Load variables from a dotenv-style file (values go to {{vars.X}}, not the process env)
    #[arg(long = "vars-file", value_name = "PATH")]
    pub vars_file: Option<std::path::PathBuf>,
    /// Deprecated alias for --vars-file (will be removed in a future release)
    #[arg(long = "env-file", value_name = "PATH", hide = true)]
    pub env_file: Option<std::path::PathBuf>,
    /// Continue running steps after a failure (default: stop on first failure)
    #[arg(long = "continue-on-failure")]
    pub continue_on_failure: bool,
    /// Parse and validate the script; resolve variables; print steps without executing
    #[arg(long = "dry-run")]
    pub dry_run: bool,
    /// Show secret values in step output (default: redact fields matching *password*, *token*, *secret*)
    #[arg(long = "show-secrets")]
    pub show_secrets: bool,
    /// Record executed steps to this file
    #[arg(long = "record", value_name = "OUTPUT")]
    pub record: Option<std::path::PathBuf>,
    /// Fail the run if recording a step fails (default: log to stderr and continue)
    #[arg(long = "record-strict")]
    pub record_strict: bool,
    /// Force a specific input format (json|yaml), overriding file extension detection
    #[arg(long = "script-format", value_name = "FORMAT")]
    pub script_format: Option<String>,
    /// Page-map file for resolving page_map:/field:/api_route: targets.
    /// Falls back to .ffrdp/page-map.json when this flag is not set and that file exists.
    #[arg(long = "page-map", value_name = "PATH")]
    pub page_map: Option<std::path::PathBuf>,
    /// Comma-separated list of env var names that {{env.X}} references may resolve.
    /// HOME/USER/LANG/LC_ALL/TZ are always allowed. Names matching the
    /// secret-name pattern (*password*, *passwd*, *pwd*, *token*, *secret*,
    /// *key*) are refused unconditionally — even an explicit entry here will
    /// not unlock them. Rename the variable or pass the value via --vars.
    #[arg(
        long = "allow-env",
        value_name = "NAMES",
        value_delimiter = ',',
        num_args = 1..,
        action = clap::ArgAction::Append
    )]
    pub allow_env: Vec<String>,
    /// Allow sub-script `run:` paths that are absolute or escape the
    /// top-level script's directory. Only enable when you author every
    /// file in the include chain.
    #[arg(long = "allow-unsafe-script-paths")]
    pub allow_unsafe_script_paths: bool,
}

#[derive(clap::Args)]
pub struct IndexArgs {
    /// Base URL to crawl (defaults to the current daemon tab's origin)
    pub base_url: Option<String>,

    /// Output path for the page-map file
    #[arg(long, default_value = ".ffrdp/page-map.json")]
    pub out: std::path::PathBuf,

    /// Maximum crawl depth from the base URL
    #[arg(long, default_value_t = 2)]
    pub depth: u32,

    /// Maximum number of pages to crawl
    #[arg(long = "max-pages", default_value_t = 50)]
    pub max_pages: usize,

    /// Only crawl URLs matching this regex
    #[arg(long)]
    pub include: Option<String>,

    /// Skip URLs matching this regex
    #[arg(long)]
    pub exclude: Option<String>,

    /// Output format: json (default) or yaml
    #[arg(long, default_value = "json")]
    pub format: String,

    /// Also crawl cross-origin links (default: same-origin only)
    #[arg(long)]
    pub cross_origin: bool,

    /// Ignore robots.txt (useful for internal admin tools)
    #[arg(long)]
    pub ignore_robots: bool,

    /// Load Netscape-format cookie jar before crawling
    #[arg(long, value_name = "PATH")]
    pub cookies_from: Option<std::path::PathBuf>,

    /// Inject Authorization: Bearer <token> on each navigate
    #[arg(long, value_name = "TOKEN")]
    pub bearer: Option<String>,

    /// Run this iter-61 script before crawling (for authentication)
    #[arg(long, value_name = "PATH")]
    pub login_script: Option<std::path::PathBuf>,

    /// Check mode: re-crawl and report drifted selectors vs. an existing map
    #[arg(long, conflicts_with_all = ["out", "format"])]
    pub check: bool,

    /// Existing page-map to check against (--check mode only)
    #[arg(long, value_name = "PATH", requires = "check")]
    pub page_map: Option<std::path::PathBuf>,

    /// Write drift report to this file (--check mode only, default: stdout)
    #[arg(long, value_name = "PATH", requires = "check")]
    pub report: Option<std::path::PathBuf>,

    /// Restrict output to paths under this directory (rejects path traversal)
    #[arg(long, value_name = "DIR")]
    pub output_root: Option<std::path::PathBuf>,
}

/// Subcommands for `ff-rdp consent`.
#[derive(Subcommand)]
pub enum ConsentCommand {
    /// Detect a known CMP on the current tab and click its "accept all" control
    #[command(
        long_about = "Detect a known cookie-consent-management-platform (CMP) overlay on the current tab and accept it.

Exit code (iter-160): 0 only when a banner was actually accepted. The command
used to exit 0 for all three outcomes, so a page whose banner was still up and
still swallowing clicks was indistinguishable from a dismissed one.

  status \"accepted\"               exit 0
  status \"detected_not_actioned\"  exit 1, error_type \"consent_not_actioned\"
  status \"no_cmp_detected\"        exit 1, error_type \"consent_no_cmp\"

One JSON document either way. On exit 0 that is the usual results envelope; on
exit 1 it is the error envelope, which carries `cmp`, `action` and `status`
alongside `error`/`error_type` so nothing is lost. The command deliberately does
NOT print a results envelope and then fail — two JSON documents on stdout is the
double-envelope bug iter-153 removed from `launch --replace`.

Output: {\"results\": {\"cmp\": \"sourcepoint\"|null, \"action\": \"accepted\"|null, \"status\": \"accepted\"|\"detected_not_actioned\"|\"no_cmp_detected\"}, \"total\": 1, \"meta\": {...}}"
    )]
    Accept {
        /// Exit 0 instead of 1 when no known CMP was found on the page.
        ///
        /// Exists for callers that run `consent accept` speculatively — a
        /// script that dismisses a banner if there is one and carries on if
        /// there is not should not have to swallow the exit code of every
        /// other failure to do that. It opts in ONLY to the
        /// "no_cmp_detected" outcome: a CMP that was found and could not be
        /// actioned still exits 1, because the caller asked for the banner to
        /// go away and it is still there.
        #[arg(long)]
        allow_no_cmp: bool,
    },
}

/// Subcommands for `ff-rdp record`.
#[derive(Subcommand)]
pub enum RecordCommand {
    /// Start a recording session
    Start {
        /// Output file path for the recorded script
        output: std::path::PathBuf,
        /// Human-readable name embedded in the script
        #[arg(long)]
        name: Option<String>,
    },
    /// Stop the active recording session and print the file path
    Stop,
    /// Show whether a recording is active
    Status,
}

#[derive(Subcommand)]
pub enum PerfCommand {
    /// Compute Core Web Vitals summary (LCP, CLS, TBT, FCP, TTFB)
    Vitals,
    /// Aggregate resource summary: sizes, request counts by type, slowest resources, domain breakdown
    Summary,
    /// Full page performance audit: vitals, navigation timing, resource breakdown, DOM stats
    ///
    /// LCP: Firefox doesn't implement the Chromium LCP PerformanceObserver entry. ff-rdp
    /// reports a best-effort approximation (largest visible image). For canonical LCP,
    /// use Lighthouse against Chromium.
    Audit,
    /// Compare performance across multiple URLs: navigate each, collect vitals + timing
    Compare {
        /// URLs to compare
        #[arg(required = true, num_args = 2..)]
        urls: Vec<String>,
        /// Labels for each URL (in order); defaults to the URL itself
        #[arg(long, value_delimiter = ',')]
        label: Option<Vec<String>>,
    },
}

#[derive(Subcommand)]
pub enum A11yCommand {
    /// Check WCAG color contrast ratios for text elements
    #[command(long_about = "Check WCAG color contrast ratios for text elements.

`total` counts the results returned: every checked element by default, or just
the AA failures under --fail-only (pre-limit — a --limit truncates `results` but
`total` still reports the full count, alongside `truncated: true`). The separate
`sampled` field reports how many elements were examined, so total == sampled
without --fail-only. `meta.summary` carries aa_pass/aa_fail/capped detail.

`capped` and `source` sit at the TOP level next to `sampled` (iter-160), not
only inside `meta`. `capped: true` means the in-page pass stopped at its
1000-element ceiling, so `total: 0` means \"none of the sampled elements
failed\" — not \"this page has no contrast failures\". A capped sample that
returns no failures also emits a hint naming the element count, because the
qualifier must travel with the clean bill of health rather than sitting two
levels down in a block --format text does not print.

Backward-compat note: before iter-127, `total` under --fail-only reported the
sampled element count rather than the failure count; that count now lives in
`sampled`.

Output: {\"results\": [{\"selector\": \"...\", \"ratio\": N, \"aa_normal\": bool, ...}], \"total\": N, \"sampled\": M, \"capped\": bool, \"source\": \"js-fallback\", \"meta\": {\"summary\": {...}}}")]
    Contrast {
        /// CSS selector to limit checking (default: all text elements)
        #[arg(long)]
        selector: Option<String>,
        /// Only show elements that fail AA contrast requirements
        /// (then `total` counts failures; `sampled` counts elements checked)
        #[arg(long)]
        fail_only: bool,
    },
    /// Flat summary: landmarks, headings, and interactive elements for quick page orientation
    #[command(
        long_about = "Flat page summary: landmarks, headings, and interactive elements.

The cheapest way to orient on a page — a few hundred tokens on an article,
against tens of kilobytes for `snapshot`'s DOM tree.

Since iter-210 every `interactive` entry carries a `ref` you can pass straight
to `click --ref` / `type --ref`, so this command is a complete answer to \"what
can I do on this page\" — no `dom <selector>` round-trip, and no guessing a
selector in order to get a handle. Refs are stored by the daemon, so they exist
only on the daemon route (the default); `meta.refs_registered` says whether the
ones in this output are usable, and `meta.source` names how the view was
produced. A navigation clears them.

`--limit N` caps the interactive list (default 50); `--all` lifts the cap.
`interactive_total` and `interactive_truncated` appear when the cap bit.

The same view is what `--with-page` embeds under `results.page` on
navigate/click/type/reload/back/forward/scroll — identical keys, so a recipe
written against one works on the other.

Output: {\"results\": {\"landmarks\": [...], \"headings\": [...], \"interactive\": [{\"role\": \"link\", \"name\": \"...\", \"href\": \"...\", \"ref\": \"e3\"}]}, \"total\": 1, \"meta\": {\"refs_registered\": bool, \"source\": \"js-fallback\", ...}}"
    )]
    Summary {
        /// Keep only the headings/landmarks/interactive entries whose text or
        /// name matches (iter-211 Theme A). Survivors keep their `ref`.
        #[command(flatten)]
        query: QueryArgs,
    },
}

/// Block-alignment values accepted by `scroll to --block`.
///
/// The CSS spec only defines `start`, `center`, `end`, `nearest`, so we map
/// the user-friendly aliases `top` → `start` and `bottom` → `end`.
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ScrollBlock {
    Top,
    Start,
    Center,
    Bottom,
    End,
    Nearest,
}

impl ScrollBlock {
    /// Return the CSSOM spec value for `scrollIntoView({block})`.
    pub fn as_spec(self) -> &'static str {
        match self {
            ScrollBlock::Top | ScrollBlock::Start => "start",
            ScrollBlock::Center => "center",
            ScrollBlock::Bottom | ScrollBlock::End => "end",
            ScrollBlock::Nearest => "nearest",
        }
    }
}

#[derive(Subcommand)]
pub enum ScrollCommand {
    /// Scroll an element into the viewport using scrollIntoView
    #[command(long_about = "Scroll an element into the viewport.

Auto-waits for the element to exist and be visible before scrolling. Use --no-wait to skip.

Output: {\"results\": {\"scrolled\": true, \"selector\": \"...\", \"viewport\": {...}, \"target\": {...}, \"atEnd\": bool}, \"total\": 1, \"meta\": {...}}")]
    #[command(group(ArgGroup::new("scroll_to_target").required(true).multiple(false).args(["selector", "ref_id"])))]
    To {
        /// CSS selector of the element to scroll into view
        #[arg(group = "scroll_to_target")]
        selector: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "scroll_to_target")]
        ref_id: Option<String>,
        /// Block alignment [default: top]. Aliases: top=start, bottom=end
        #[arg(long, value_enum, default_value_t = ScrollBlock::Top)]
        block: ScrollBlock,
        /// Use smooth scrolling behavior (default is instant)
        #[arg(long)]
        smooth: bool,
        /// Skip auto-wait and scroll immediately (reverts to pre-iter-59 fire-and-forget)
        #[arg(long)]
        no_wait: bool,
        /// After scrolling, wait for this condition. Repeatable. Forms: selector:<css>, text:<substr>, url:<regex>, gone:<css>
        #[arg(long, value_name = "PREDICATE", action = clap::ArgAction::Append)]
        wait_for: Vec<String>,
        /// Timeout in milliseconds for --wait-for predicates (default: same as --timeout)
        #[arg(long, value_name = "MS")]
        wait_for_timeout: Option<u64>,
        /// After scrolling, wait for network and DOM to idle
        #[arg(long)]
        settle: bool,
        /// After scrolling, embed the resulting page under `results.page`
        /// (`headings`, `landmarks`, `interactive` with `click --ref` handles)
        /// — the same view `ff-rdp a11y summary` prints. Collected last, so
        /// content the scroll lazily rendered is included.
        #[arg(long)]
        with_page: bool,
    },
    /// Scroll the viewport by a number of pixels or by a page
    #[command(
        long_about = "Scroll the viewport by pixels or by a full page.
  --page-down and --page-up scroll by 85% of the viewport height.
  --page-down and --page-up are mutually exclusive with --dy and with each other.
  Negative values for --dy/--dx are accepted (use 'scroll by --dy -500' or '--dy=-500').
Output: {\"results\": {\"scrolled\": true, \"viewport\": {...}, \"scrollHeight\": N, \"atEnd\": bool}, \"total\": 1, \"meta\": {...}}",
        allow_negative_numbers = true
    )]
    By {
        /// Horizontal scroll delta in pixels
        #[arg(long, default_value_t = 0)]
        dx: i64,
        /// Vertical scroll delta in pixels (mutually exclusive with --page-down/--page-up)
        #[arg(long, conflicts_with_all = ["page_down", "page_up"])]
        dy: Option<i64>,
        /// Scroll down by 85% of the viewport height (mutually exclusive with --dy/--page-up)
        #[arg(long, conflicts_with_all = ["dy", "page_up"])]
        page_down: bool,
        /// Scroll up by 85% of the viewport height (mutually exclusive with --dy/--page-down)
        #[arg(long, conflicts_with_all = ["dy", "page_down"])]
        page_up: bool,
        /// Use smooth scrolling behavior
        #[arg(long)]
        smooth: bool,
        /// After scrolling, embed the resulting page under `results.page`
        /// (`headings`, `landmarks`, `interactive` with `click --ref` handles)
        /// — the same view `ff-rdp a11y summary` prints. Collected last, so
        /// content the scroll lazily rendered is included.
        #[arg(long)]
        with_page: bool,
    },
    /// Scroll to the very top of the page (equivalent to scroll by --dy -99999999)
    #[command(long_about = "Scroll to the very top of the page.
  Uses window.scrollTo(0, 0) for an instant jump to the top.
  `warning` (iter-129) is always present — null normally, or a string naming the
  locking element (e.g. \"<html> (class=\\\"sp-message-open\\\") has overflow:hidden\")
  when the scroll position didn't move AND <html>/<body> has overflow:hidden —
  the CMP/modal-overlay case where a silent atEnd:true would otherwise mask
  the real cause (dogfooding-session-62).
Output: {\"results\": {\"scrolled\": true, \"viewport\": {...}, \"scrollHeight\": N, \"atEnd\": bool, \"warning\": null}, \"total\": 1, \"meta\": {...}}")]
    Top {
        /// After scrolling, embed the resulting page under `results.page`
        /// (`headings`, `landmarks`, `interactive` with `click --ref` handles)
        /// — the same view `ff-rdp a11y summary` prints. Collected last, so
        /// content the scroll lazily rendered is included.
        #[arg(long)]
        with_page: bool,
    },
    /// Scroll to the very bottom of the page (equivalent to scroll by --dy 99999999)
    #[command(long_about = "Scroll to the very bottom of the page.
  Uses window.scrollTo(0, document.documentElement.scrollHeight) for an instant jump to the bottom.
  `warning` (iter-129) is always present — null normally, or a string naming the
  locking element (e.g. \"<html> (class=\\\"sp-message-open\\\") has overflow:hidden\")
  when the scroll position didn't move AND <html>/<body> has overflow:hidden —
  the CMP/modal-overlay case where a silent atEnd:true would otherwise mask
  the real cause (dogfooding-session-62).
Output: {\"results\": {\"scrolled\": true, \"viewport\": {...}, \"scrollHeight\": N, \"atEnd\": bool, \"warning\": null}, \"total\": 1, \"meta\": {...}}")]
    Bottom {
        /// After scrolling, embed the resulting page under `results.page`
        /// (`headings`, `landmarks`, `interactive` with `click --ref` handles)
        /// — the same view `ff-rdp a11y summary` prints. Collected last, so
        /// content the scroll lazily rendered is included.
        #[arg(long)]
        with_page: bool,
    },
    /// Scroll an overflow container element directly
    #[command(
        long_about = "Scroll an overflow container element (scrollTop/scrollLeft).
  --to-end scrolls to the bottom; --to-start scrolls to the top.
Output: {\"results\": {\"scrolled\": true, \"selector\": \"...\", \"before\": {...}, \"after\": {...}, \"scrollHeight\": N, \"clientHeight\": N, \"atEnd\": bool}, \"total\": 1, \"meta\": {...}}"
    )]
    Container {
        /// CSS selector of the overflow container
        selector: String,
        /// Horizontal scroll delta in pixels
        #[arg(long, default_value_t = 0)]
        dx: i64,
        /// Vertical scroll delta in pixels
        #[arg(long, default_value_t = 0)]
        dy: i64,
        /// Scroll to the end (bottom/right) of the container (ignores --dx/--dy)
        #[arg(long, conflicts_with_all = ["to_start", "dx", "dy"])]
        to_end: bool,
        /// Scroll to the start (top/left) of the container (ignores --dx/--dy)
        #[arg(long, conflicts_with_all = ["to_end", "dx", "dy"])]
        to_start: bool,
        /// After scrolling, embed the resulting page under `results.page`
        /// (`headings`, `landmarks`, `interactive` with `click --ref` handles)
        /// — the same view `ff-rdp a11y summary` prints. Collected last, so
        /// content the scroll lazily rendered is included.
        #[arg(long)]
        with_page: bool,
    },
    /// Scroll until an element is visible in the viewport (polls up to --timeout)
    #[command(long_about = "Scroll until an element is visible in the viewport.
  Polls every 200ms, scrolling by 80% of the viewport height each step.
Output: {\"results\": {\"found\": true, \"selector\": \"...\", \"elapsed_ms\": N, \"scrolls\": N, \"viewport\": {...}, \"target\": {...}}, \"total\": 1, \"meta\": {...}}")]
    Until {
        /// CSS selector of the element to scroll to
        selector: String,
        /// Scroll direction: up or down [default: down]
        #[arg(long, default_value = "down")]
        direction: String,
        /// Timeout in milliseconds before giving up [default: 10000]
        #[arg(long, default_value_t = 10000)]
        timeout: u64,
        /// After scrolling, embed the resulting page under `results.page`
        /// (`headings`, `landmarks`, `interactive` with `click --ref` handles)
        /// — the same view `ff-rdp a11y summary` prints. Collected last, so
        /// content the scroll lazily rendered is included.
        #[arg(long)]
        with_page: bool,
    },
    /// Find text on the page and scroll to it using TreeWalker
    #[command(
        long_about = "Find a text string on the page and scroll its container element into view.
  Uses TreeWalker + NodeFilter.SHOW_TEXT to find the first matching text node (case-sensitive).
Output: {\"results\": {\"scrolled\": true, \"text\": \"...\", \"viewport\": {...}, \"target\": {\"tag\": \"...\", \"rect\": {...}}}, \"total\": 1, \"meta\": {...}}"
    )]
    Text {
        /// Text to search for (case-sensitive substring match)
        text: String,
        /// After scrolling, embed the resulting page under `results.page`
        /// (`headings`, `landmarks`, `interactive` with `click --ref` handles)
        /// — the same view `ff-rdp a11y summary` prints. Collected last, so
        /// content the scroll lazily rendered is included.
        #[arg(long)]
        with_page: bool,
    },
}

#[derive(Subcommand)]
pub enum DaemonCommand {
    /// Print daemon status as JSON
    #[command(long_about = "Print the current daemon status as JSON.

If no daemon is running, reports running=false.

Output: {\"results\": {\"running\": bool, \"pid\": N, \"port\": N, \"uptime_seconds\": N, \"connections\": N, \"buffer_sizes\": {...}}, \"total\": 1, \"meta\": {...}}")]
    Status,
    /// Gracefully stop the running daemon
    #[command(long_about = "Gracefully stop the running daemon.

Sends a shutdown RPC to the daemon. Falls back to SIGTERM if the RPC does
not succeed within 2 seconds. Cleans up the daemon's per-port registry file
(daemon.<port>.json) on success.

When Firefox was started via `launch`, stopping it also removes its
temporary profile directory (never a directory passed via --profile).

Output: {\"results\": {\"stopped\": bool, \"pid\": N, \"port\": N, \"profile_removed\": bool, \"profile_removed_path\": \"...\"|null}, \"total\": 1, \"meta\": {...}}")]
    Stop,
}

#[derive(Subcommand)]
pub enum ProfilesCommand {
    /// Report profile-root path, managed-entry count, total size, and oldest mtime
    #[command(
        long_about = "Report the profile root path, managed-entry count, total on-disk size, and oldest mtime.

Only entries matching the `ff-rdp-profile-<16 alphanumeric chars>` naming convention are counted.

Output: {\"results\": {\"path\": \"...\", \"count\": N, \"total_size_bytes\": N, \"oldest_mtime\": \"...\"|null}, \"total\": 1, \"meta\": {...}}"
    )]
    List,
    /// Remove stale managed profile directories
    #[command(
        long_about = "Remove managed `ff-rdp-profile-*` directories under the profile root.

By default only removes stale entries (default --older-than 7d). An entry is stale when both
the directory mtime AND its newest top-level file mtime are at least --older-than old — a
profile a running Firefox is still writing into is not selected.
Any age-gated prune (i.e. not --all) also honours a positive liveness guard: a profile whose
owner Firefox process is still alive (recorded in an `.ff-rdp-owner-pid` marker at launch) is
never removed, regardless of age.
Pass --all to remove every managed entry regardless of age (mutually exclusive with
--older-than). --all bypasses the age gate but NOT quietly: a profile whose owner Firefox is
still alive is still removed (--all is the explicit escape hatch), but each such removal is
logged as a warning and its basename is listed under `removed_live` in the output. Do not run
--all while a Firefox launched by ff-rdp is still using one of these profiles.
Pass --dry-run to preview without touching disk: `would_remove` is populated and `removed` stays
empty, and every listed directory still exists afterwards. On a real run it's the other way round:
`removed` is populated and `would_remove` stays empty.

This subcommand is a pure age query and stays one: it never reclaims anything early. The two
'provably abandoned regardless of age' rules — a marker naming a dead owner (iter-142), and an
unmarked directory holding nothing but `user.js`, i.e. a launch that died before Firefox ever
opened it (iter-175) — belong to the automatic sweep `ff-rdp launch` runs, so `ff-rdp launch`
is what reclaims those. Use --all here if you want everything gone now.

Duration grammar for --older-than: <N>d, <N>h, <N>m, <N>s, or a bare number of seconds
(e.g. 7d, 24h, 30m, 45s, 3600). Individual removal failures (permission error, a directory
vanishing mid-scan) are logged and skipped, not fatal to the rest of the batch.

Examples:
  ff-rdp profiles prune --dry-run
  ff-rdp profiles prune --older-than 24h
  ff-rdp profiles prune --all

Output: {\"results\": {\"path\": \"...\", \"would_remove\": [...], \"removed\": [...], \"removed_live\": [...], \"dry_run\": bool}, \"total\": N, \"meta\": {...}}"
    )]
    Prune {
        /// Only remove entries whose mtime is at least this old. Accepts <N>d, <N>h, <N>m, <N>s,
        /// or a bare number of seconds. Mutually exclusive with --all.
        #[arg(
            long,
            default_value = "7d",
            value_name = "DURATION",
            conflicts_with = "all"
        )]
        older_than: String,
        /// Remove every managed profile directory regardless of age. Mutually exclusive with --older-than.
        #[arg(long, conflicts_with = "older_than")]
        all: bool,
        /// Preview what would be removed without touching disk.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum DomCommand {
    /// DOM statistics: node count, document size, inline scripts, images without lazy loading
    Stats,
    /// Dump structured DOM subtree via native WalkerActor (not eval)
    Tree {
        /// CSS selector to root the tree at (defaults to document element)
        selector: Option<String>,
        /// Maximum tree depth to traverse (default: 6)
        #[arg(long, default_value_t = 6)]
        depth: u32,
        /// Maximum total characters of text content to include (default: 50000)
        #[arg(long, default_value_t = 50000)]
        max_chars: u32,
    },
}

// ---------------------------------------------------------------------------
// install-skill args
// ---------------------------------------------------------------------------

/// Target scope for skill installation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SkillScope {
    /// Install to $HOME/.claude/skills/ (default)
    User,
    /// Install to <git-root>/.claude/skills/
    Project,
}

impl SkillScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

#[derive(clap::Args, Debug)]
pub struct InstallSkillArgs {
    /// Target the Claude Code agent runtime (required; forward-compat flag)
    #[arg(long)]
    pub claude: bool,

    /// Install to $HOME/.claude/skills/ (default)
    #[arg(long, conflicts_with = "project")]
    pub user: bool,

    /// Install to <git-root>/.claude/skills/
    #[arg(long, conflicts_with = "user")]
    pub project: bool,

    /// Overwrite unmanaged files and bypass git-repo check for --project
    #[arg(long)]
    pub force: bool,

    /// Preview files that would be written without touching disk
    #[arg(long)]
    pub dry_run: bool,

    /// Read skill source from a directory on disk instead of the embedded data
    #[arg(long, value_name = "PATH")]
    pub from_dir: Option<std::path::PathBuf>,

    /// List registered skills and their installation status, then exit
    #[arg(long, conflicts_with_all = ["uninstall", "dry_run"])]
    pub list: bool,

    /// Remove an installed skill by name
    #[arg(long, value_name = "NAME", conflicts_with_all = ["list", "dry_run"])]
    pub uninstall: Option<String>,

    /// Skill name to install; if omitted, all registered skills are installed
    pub skill_name: Option<String>,
}

impl InstallSkillArgs {
    /// Resolve the effective scope (user unless --project was explicitly passed).
    pub fn effective_scope(&self) -> SkillScope {
        if self.project {
            SkillScope::Project
        } else {
            SkillScope::User
        }
    }
}

#[cfg(test)]
mod help_stack_tests {
    //! iter-99: guard the `cookies --help` (and every other subcommand's `--help`)
    //! stack-overflow regression.
    //!
    //! Background: on Windows the process main thread has a 1 MiB stack (vs 8 MiB
    //! on Linux/macOS).  clap's derive builds every struct-variant subcommand's
    //! argument list inside one monolithic `Command::augment_subcommands` stack
    //! frame; with ~40 subcommands and ~150 inline args that single frame grew
    //! past 1 MiB, so `ff-rdp cookies --help` exited `0xC00000FD`
    //! (STATUS_STACK_OVERFLOW) on windows-latest CI while staying green on the
    //! 8 MiB Linux/macOS runners.
    //!
    //! iter-99 moved each arg-heavy variant's fields into a dedicated
    //! `#[derive(clap::Args)]` struct (`Navigate(NavigateArgs)` …), so each
    //! subcommand's args build in their own frame.  These tests pin the fix by
    //! rendering help inside a deliberately small (1 MiB) thread stack, mirroring
    //! the Windows main-thread limit so the latent overflow is reproducible on
    //! every platform.

    use super::Cli;
    use clap::CommandFactory;

    /// Size of the guard thread's stack: 1 MiB, matching the Windows main-thread
    /// stack that overflowed pre-fix.  Rendering `--help` must complete within it.
    const SMALL_STACK: usize = 1 << 20;

    /// Render one subcommand's long help into a byte buffer.  Returns the number
    /// of bytes written so the closure has an observable result (and so the
    /// optimizer can't elide the work).
    fn render_subcommand_help(name: &str) -> usize {
        let mut cmd = Cli::command();
        // `build()` finalizes the whole tree exactly as `try_parse_from` does
        // before rendering help — this is the code path that overflowed.
        cmd.build();
        let sub = cmd
            .find_subcommand_mut(name)
            .unwrap_or_else(|| panic!("subcommand {name:?} not found"));
        let mut buf: Vec<u8> = Vec::new();
        sub.write_long_help(&mut buf)
            .expect("write_long_help must not fail");
        buf.len()
    }

    /// Run `f` on a thread whose stack is capped at [`SMALL_STACK`].  Panics with
    /// a clear message if the thread overflowed / panicked.
    fn in_small_stack<F, T>(what: &str, f: F) -> T
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        std::thread::Builder::new()
            .stack_size(SMALL_STACK)
            .spawn(f)
            .expect("spawn small-stack thread")
            .join()
            .unwrap_or_else(|_| {
                panic!("{what} overflowed a {SMALL_STACK}-byte stack (STATUS_STACK_OVERFLOW regression)")
            })
    }

    /// iter-99 AC / pre-fix repro: `cookies --help` must render inside a 1 MiB
    /// stack.  Pre-fix this overflowed on every platform (the Windows-only CI
    /// failure made reproducible everywhere); post-fix it completes.
    #[test]
    fn pre_fix_repro_cookies_help_stack_depth() {
        let bytes = in_small_stack("cookies --help", || render_subcommand_help("cookies"));
        assert!(
            bytes > 0,
            "cookies --help rendered no output — help text missing?"
        );
    }

    /// iter-99 AC: every subcommand's `--help` must render inside the same 1 MiB
    /// stack, so no other command hides the same latent oversized-frame bug.
    #[test]
    fn unit_all_subcommand_helps_render_in_small_stack() {
        // Collect the externally-visible subcommand names from the built tree so
        // this test automatically covers subcommands added in future iterations.
        let names: Vec<String> = {
            let mut cmd = Cli::command();
            cmd.build();
            cmd.get_subcommands()
                .map(|s| s.get_name().to_owned())
                .collect()
        };
        assert!(
            names.iter().any(|n| n == "cookies"),
            "expected the cookies subcommand to be present; got {names:?}"
        );
        for name in names {
            let owned = name.clone();
            let bytes = in_small_stack(&format!("{owned} --help"), move || {
                render_subcommand_help(&owned)
            });
            assert!(bytes > 0, "subcommand {name:?} --help rendered no output");
        }
    }
}
