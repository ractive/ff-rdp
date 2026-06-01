use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

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
    ff-rdp navigate <URL> [--with-network] [--wait-text T | --wait-selector S] [--wait-timeout MS]
    ff-rdp reload [--wait-idle [--idle-ms MS] [--reload-timeout MS]]
    ff-rdp back | forward
    ff-rdp wait --selector S | --text T | --eval JS [--wait-timeout MS]

  Page content:
    ff-rdp eval <SCRIPT> | --file PATH | --stdin [--stringify] [--no-isolate]
    ff-rdp page-text
    ff-rdp dom <SEL> [--text | --attrs | --text-attrs | --inner-html | --count]
    ff-rdp dom stats
    ff-rdp dom tree [SEL] [--depth N] [--max-chars N]
    ff-rdp snapshot [--depth N] [--max-chars N]

  Interaction:
    ff-rdp click <SEL> [--dispatch pointer|legacy|click-only] [--no-wait] [--settle]
    ff-rdp click <SEL> --wait-for-network <pattern> [--network-timeout MS]
    ff-rdp click <SEL> --wait-for selector:<css> --wait-for text:<substr>
    ff-rdp type <SEL> <TEXT> [--clear] [--no-wait] [--settle] [--wait-for ...]

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

  Accessibility:
    ff-rdp a11y [--depth N] [--selector SEL] [--interactive] [--critical]
    ff-rdp a11y contrast [--selector SEL] [--fail-only]
    ff-rdp a11y summary

  Performance:
    ff-rdp perf [--type TYPE] [--filter URL] [--group-by domain]
    ff-rdp perf vitals | summary | audit
    ff-rdp perf compare <URL>... [--label L1,L2,...]

  Monitoring:
    ff-rdp console [--level LEVEL] [--pattern REGEX] [--follow]
    ff-rdp network [--filter URL] [--method M] [--follow]
    ff-rdp network --detail [--headers]    # include request+response headers per entry

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

AI AGENT TIPS:
  - Use --format text instead of JSON for 3-10x fewer tokens
  - Use eval --stringify '<expr>' to get actual values instead of actor grip metadata
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
  ff-rdp a11y contrast --fail-only
  ff-rdp a11y --interactive --jq '[.. | select(.role? == \"link\") | .name]'

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
  Truncated output adds: {\"truncated\": true, \"hint\": \"showing 20 of 84, use --all\"}
  --format json  (default) machine-readable JSON — the stable API contract
  --format text  human-readable tables and trees
  --format html  raw HTML passthrough (dom and snapshot only — pre-iter-60 shape)
  --jq can be combined with --format text: jq runs first, text rendering applies
  Use --jq to filter the envelope: --jq '.results[0]', --jq '.total'
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
/// commit date: `0.2.0 (abc123def456 2026-05-26)`.  When `+dirty` is appended
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
    #[arg(long, default_value_t = 10000, global = true)]
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

    /// Sort results by field name
    #[arg(long, global = true)]
    pub sort: Option<String>,

    /// Sort ascending (default is per-command)
    #[arg(long, global = true, conflicts_with = "desc")]
    pub asc: bool,

    /// Sort descending (default is per-command)
    #[arg(long, global = true, conflicts_with = "asc")]
    pub desc: bool,

    /// Comma-separated list of fields to include in each result entry
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

Output: {\"results\": [{\"url\": \"...\", \"title\": \"...\", \"actor\": \"...\", \"selected\": true}], \"total\": N, \"meta\": {...}}")]
    Tabs,
    /// Navigate to a URL
    #[command(long_about = "Navigate to a URL.

By default, navigate blocks until the new document is committed (URL changes and
readyState reaches 'interactive' or 'complete'), or the --timeout budget expires.
The result includes 'committed_url', 'ready_state', and 'elapsed_ms' so agents can
confirm what page actually loaded.

Use --no-wait to restore the old fire-and-forget behaviour (returns immediately after
the navigate request is acknowledged, without waiting for the document to commit).

The URL is a positional argument (not a flag). There is no --url option.

Examples:
  ff-rdp navigate https://example.com
  ff-rdp navigate https://example.com --with-network
  ff-rdp navigate https://example.com --wait-text \"Welcome\"
  ff-rdp navigate https://example.com --wait-for selector:.athing
  ff-rdp navigate https://example.com --no-wait

Output: {\"results\": {\"navigated\": \"...\", \"committed_url\": \"...\", \"ready_state\": \"...\", \"elapsed_ms\": N}, \"total\": 1, \"meta\": {...}}")]
    Navigate {
        /// The URL to navigate to (positional, not a flag)
        url: String,
        /// Also capture network requests made during navigation
        #[arg(long)]
        with_network: bool,
        /// Total time limit for network event collection in milliseconds (--with-network only).
        /// Collection runs for this duration then returns all captured events.
        #[arg(long, default_value_t = 10000)]
        network_timeout: u64,
        /// After navigating, wait for this text to appear in the page's visible content. Runs after the navigation load event completes.
        #[arg(long, conflicts_with = "wait_selector")]
        wait_text: Option<String>,
        /// After navigating, wait for this CSS selector to match an element in the DOM. Runs after the navigation load event completes.
        #[arg(long, conflicts_with = "wait_text")]
        wait_selector: Option<String>,
        /// Timeout for the --wait-text/--wait-selector condition in milliseconds. If the condition is not met within this time, the command fails with an error showing the elapsed time.
        #[arg(long, default_value_t = 5000)]
        wait_timeout: u64,
        /// Skip waiting for the new document to commit; return immediately after the navigate request is acknowledged (pre-61g fire-and-forget behaviour).
        #[arg(long)]
        no_wait: bool,
        /// Readiness level to wait for before returning: `loading` (dom-loading), `interactive` (dom-interactive), or `complete` (dom-complete, default). Ignored when `--with-network` is set: that mode uses the network-drain settle as its commit signal.
        #[arg(long, value_name = "LEVEL", default_value = "complete", value_enum)]
        wait: crate::commands::navigate::WaitLevel,
        /// After the document commits, additionally wait for a predicate. Accepts selector:<css>, text:<substr>, url:<regex>, or gone:<css>.
        /// Uses the --timeout budget. On failure surfaces a descriptive error.
        #[arg(long, value_name = "PREDICATE")]
        wait_for: Vec<String>,
        /// Strategy for waiting for navigation readiness.
        /// `both` (default): try events first; if they time out, fall back to
        ///         readystate poll within the remaining budget.
        /// `events`: wait for document-event resources (dom-complete).
        /// `readystate`: poll `document.readyState == "complete"` until timeout.
        #[arg(long, value_name = "STRATEGY", default_value = "both", value_enum)]
        wait_strategy: crate::commands::navigate::WaitStrategy,
    },
    /// Evaluate JavaScript in the target tab
    #[command(long_about = "Evaluate JavaScript in the target tab.

Three input modes (exactly one required):
  Positional:  ff-rdp eval 'document.title'
  From file:   ff-rdp eval --file script.js
  From stdin:  echo 'document.title' | ff-rdp eval --stdin

Prefer --file or --stdin for scripts that contain shell metacharacters,
optional chaining (?.), template literals, or multi-line statements — shell
quoting can mangle them and produce a SyntaxError at column 1.

Since iter-93, scripts are routed through Firefox's Debugger.evalInGlobal
sandbox scope (which bypasses page CSP), so each call already has its own
scope and `const`/`let` declarations never leak across calls. The
`--no-isolate` flag is kept for backwards compatibility but is now a no-op.

Output: {\"results\": <value>, \"total\": 1, \"meta\": {...}}

When the result is a non-primitive (object, array), Firefox returns actor grip
metadata (actor IDs, class names) instead of the actual values. Use --stringify
to wrap the expression in JSON.stringify() and get the real data back.

Pass --unwrap when the expression itself already returns a JSON-encoded string
(e.g. `localStorage.getItem('user')` or a server endpoint that returns text):
ff-rdp will parse it client-side and put the structured object/array into
`results`. Primitive or non-JSON strings are passed through unchanged.")]
    #[command(group(
        ArgGroup::new("eval_source")
            .required(true)
            .multiple(false)
            .args(["script", "file", "stdin"])
    ))]
    Eval {
        /// JavaScript expression to evaluate (positional)
        script: Option<String>,
        /// Read JavaScript source from a file
        #[arg(long, value_name = "PATH")]
        file: Option<String>,
        /// Read JavaScript source from stdin until EOF
        #[arg(long)]
        stdin: bool,
        /// Wrap expression in JSON.stringify() to get actual values instead of actor grips
        #[arg(long)]
        stringify: bool,
        /// No-op since iter-93. Kept for backwards compatibility — isolation
        /// is now provided by Firefox's Debugger.evalInGlobal sandbox scope.
        #[arg(long)]
        no_isolate: bool,
        /// Evaluate inside a specific frame/iframe actor (iter-77 S3).
        ///
        /// Pass the frame actor ID — e.g. obtained from a `watcher`
        /// `target-available-form` event with `targetType=frame`.  Wires the
        /// spec-declared `frameActor` field of `evaluateJSAsync`
        /// (devtools/shared/specs/webconsole.js:149-164).
        #[arg(long, value_name = "ACTOR")]
        frame: Option<String>,
        /// Pre-bind `$0` to a DOM node actor before evaluating (iter-77 S3).
        ///
        /// Maps to `selectedNodeActor` in the `evaluateJSAsync` request.
        #[arg(long, value_name = "ACTOR")]
        node: Option<String>,
        /// Scope the eval to a specific inner-window ID (iter-77 S3).
        ///
        /// Maps to `innerWindowID` in the `evaluateJSAsync` request.
        #[arg(long, value_name = "ID")]
        inner_window: Option<u64>,
        /// When the result is a JSON-encoded string for an object or array, parse it
        /// on the client and replace `results` with the structured value.  Pairs
        /// naturally with `--stringify` and with scripts that already return
        /// `JSON.stringify(...)`.  Non-JSON strings are left unchanged.
        #[arg(long)]
        unwrap: bool,
    },
    /// Extract visible page text (document.body.innerText)
    #[command(long_about = "Extract visible page text (document.body.innerText).

Output: {\"results\": \"<page text as a plain string>\", \"total\": 1, \"meta\": {...}}")]
    PageText,
    /// Query DOM elements by CSS selector
    #[command(long_about = "Query DOM elements by CSS selector.

Default output (ARIA-tree JSON): {\"results\": [{\"ref\":\"e1\",\"role\":\"heading\",\"name\":\"...\",\"level\":1,\"tag\":\"h1\",\"attrs\":{...}}, ...], \"total\": N}

Since iter-61i, `results` is **always an array** regardless of match count (0 → [], 1 → [item], N → [item, ...]). Agent recipes like `--jq '.results[0]'` work uniformly.

Each element has: ref (stable ID), role (ARIA semantic role), name (accessible name), tag, attrs (actionable only), state, level (headings).
Use --format html for raw HTML strings in each array slot.
Use --first to revert to the legacy single-value shape (object/string/null, total: 0 or 1).
With --count: {\"results\": {\"count\": N}, \"total\": N, \"meta\": {...}}

See also:
  ff-rdp styles <SEL>    — declared (matched) CSS rules for an element.
  ff-rdp computed <SEL>  — resolved computed style values for an element.")]
    #[command(group(ArgGroup::new("dom_target").required(false).multiple(false).args(["selector", "ref_id"])))]
    Dom {
        #[command(subcommand)]
        dom_command: Option<DomCommand>,

        /// CSS selector to match elements
        #[arg(group = "dom_target")]
        selector: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "dom_target")]
        ref_id: Option<String>,
        /// Output outer HTML (default)
        #[arg(long, group = "output_mode")]
        outer_html: bool,
        /// Output inner HTML
        #[arg(long, group = "output_mode")]
        inner_html: bool,
        /// Output text content only
        #[arg(long, group = "output_mode")]
        text: bool,
        /// Output element attributes as JSON objects
        #[arg(long, group = "output_mode")]
        attrs: bool,
        /// Output both text content and attributes per element
        #[arg(long, group = "output_mode")]
        text_attrs: bool,
        /// Return only the count of matching elements
        #[arg(long, group = "output_mode")]
        count: bool,
        /// Return just the first match as a single value (or null) instead of an array.
        /// Provided for callers who want the legacy pre-iter-61i single-element shape.
        /// Mutually exclusive with --count.
        #[arg(long, conflicts_with = "count")]
        first: bool,
        /// Attach computed CSS values for each match (comma-separated property list).
        /// Each result element gets an extra `style` field with the named getComputedStyle
        /// values, e.g. `--include-style color,display`. Capped by `--include-style-limit`.
        #[arg(long, value_name = "PROPS")]
        include_style: Option<String>,
        /// Cap the number of matches that receive computed styles when
        /// `--include-style` is set. Default 50. Elements beyond the cap omit the
        /// `style` field and the response sets `meta.style_truncated: true`.
        #[arg(
            long,
            value_name = "N",
            default_value_t = 50,
            requires = "include_style"
        )]
        include_style_limit: usize,
    },
    /// Read console messages
    #[command(long_about = "Read console messages.

Default: 50 messages, sorted by timestamp (newest first).
Output always includes a `summary` field with totals and per-level counts so
callers can tell at a glance whether the filter caught what they expected.

Output: {\"results\": [{\"level\": \"...\", \"message\": \"...\", \"source\": \"...\", \"line\": N, \"timestamp\": N}], \"summary\": {\"total\": N, \"shown\": Z, \"by_level\": {...}, \"matched\": M}, \"total\": N, \"meta\": {...}}")]
    Console {
        /// Filter by log level (error, warn, info, log, debug)
        #[arg(long)]
        level: Option<String>,
        /// Filter by message content (regex pattern)
        #[arg(long)]
        pattern: Option<String>,
        /// Stream console messages in real time (connection closed or Ctrl-C to stop)
        #[arg(long)]
        follow: bool,
    },
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

Source precedence (daemon mode):
  1. Daemon watcher buffer (source=watcher): used when the daemon has buffered
     network events for the current navigation. This is the default path when
     the daemon is running and `navigate --with-network` was used previously.
  2. Performance API fallback (source=performance-api): used only when the
     watcher buffer is empty (no events captured for the current navigation).

Field fidelity by source:
  watcher:         method, status, content_type, duration_ms, size_bytes, transfer_size all available
  performance-api: method=null, status=null; duration_ms, transfer_size available via Resource Timing API

Default: 20 results, sorted by duration (slowest first).
Output (summary mode): {\"results\": {\"total_requests\": N, \"total_transfer_bytes\": N, \"by_cause_type\": {...}, \"slowest\": [...], \"timeout_reached\": false}, \"total\": N, \"meta\": {...}}
Output (--detail): {\"results\": [{\"url\": \"...\", \"method\": \"GET\", \"status\": 200, \"duration_ms\": N, ...}], \"total\": N, \"meta\": {...}}
Output (--detail --headers): adds {\"headers\": {\"request\": [{\"name\": \"...\", \"value\": \"...\"}], \"response\": [...]}} per entry.")]
    Network {
        /// Filter by URL pattern (substring match)
        #[arg(long)]
        filter: Option<String>,
        /// Filter by HTTP method (GET, POST, etc.)
        #[arg(long)]
        method: Option<String>,
        /// Stream network events in real time (Ctrl-C to stop)
        #[arg(long)]
        follow: bool,
        /// Include request and response headers in --detail output.
        /// Headers are fetched per-entry from the NetworkEventActor (watcher source
        /// only). When the source is performance-api, a per-entry note is emitted
        /// explaining why headers are missing; use --with-network to engage the
        /// watcher and make headers available.
        #[arg(long)]
        headers: bool,
        /// Scope the result to a specific navigation window (daemon mode only).
        /// -1 = current navigation (default), -2 = one back, 'all' = full cumulative buffer.
        /// Positive integers are treated as 1-based indices from the oldest boundary.
        #[arg(long, value_name = "NAV_INDEX_OR_ALL")]
        since: Option<String>,
    },
    /// Query browser Performance API entries and Core Web Vitals
    #[command(
        long_about = "Query browser Performance API entries and Core Web Vitals.

Default: 20 resources, sorted by duration (slowest first).
Output: {\"results\": [{\"url\": \"...\", \"duration_ms\": N, \"transfer_size\": N, ...}], \"total\": N, \"meta\": {...}}"
    )]
    Perf {
        #[command(subcommand)]
        perf_command: Option<PerfCommand>,

        /// Performance entry type to query (resource, navigation, paint, lcp, cls, longtask)
        #[arg(long = "type", default_value = "resource")]
        entry_type: String,

        /// Filter by URL substring (resource/navigation types)
        #[arg(long)]
        filter: Option<String>,

        /// Group results by a field (e.g., "domain" for resource entries)
        #[arg(long)]
        group_by: Option<String>,
    },
    /// Capture a screenshot
    #[command(long_about = "Capture a screenshot.

By default the screenshot is captured at the current viewport size.
Use --full-page to capture the entire scrollable document (up to
document.scrollingElement.scrollHeight) or --viewport-height N for an
explicit override.

Output: {\"results\": {\"path\": \"...\", \"width\": N, \"height\": N}, \"total\": 1, \"meta\": {...}}
With --base64: {\"results\": {\"base64\": \"...\"}, \"total\": 1, \"meta\": {...}}")]
    Screenshot {
        /// Output file path
        #[arg(long, short, conflicts_with = "base64")]
        output: Option<String>,
        /// Return the screenshot as base64 PNG data in JSON output instead of saving to a file
        #[arg(long, conflicts_with = "output")]
        base64: bool,
        /// Capture the entire scrollable page (document.scrollingElement.scrollHeight)
        #[arg(long, conflicts_with = "viewport_height")]
        full_page: bool,
        /// Capture at this explicit height (pixels) instead of the viewport height
        #[arg(long, value_name = "PX", conflicts_with = "full_page")]
        viewport_height: Option<u32>,
        /// Restrict output to paths under this directory (rejects path traversal)
        #[arg(long, value_name = "DIR")]
        output_root: Option<std::path::PathBuf>,
        /// Attempt to receive the screenshot via a bulk-frame streaming path.
        ///
        /// When set, the command sends the capture request and then tries to
        /// read the response as a bulk binary frame via
        /// `Transport::recv_bulk_with_handler` (no full base64 allocation in
        /// memory).  If Firefox responds with a JSON frame (the current
        /// behaviour for all Firefox versions), the command falls back to the
        /// standard base64 path transparently.
        #[arg(long)]
        bulk: bool,
    },
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

Output: {\"results\": {\"clicked\": true, \"tag\": \"...\", \"text\": \"...\"}, \"total\": 1, \"meta\": {...}}
With --wait-for-network: adds {\"network\": {\"url\": \"...\", \"method\": \"...\", \"status\": N, ...}} to results.")]
    #[command(group(ArgGroup::new("click_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
    Click {
        /// CSS selector of the element to click (positional, or use --selector)
        #[arg(group = "click_target")]
        selector_pos: Option<String>,
        /// CSS selector of the element to click (flag form)
        #[arg(long = "selector", value_name = "SELECTOR", group = "click_target")]
        selector_flag: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "click_target")]
        ref_id: Option<String>,
        /// After clicking, wait for a network request whose URL contains this pattern.
        /// Returns the matched request record in the output.
        #[arg(long, value_name = "PATTERN")]
        wait_for_network: Option<String>,
        /// Timeout in milliseconds for --wait-for-network (default: global --timeout)
        #[arg(long, value_name = "MS", requires = "wait_for_network")]
        network_timeout: Option<u64>,
        /// Skip auto-wait and click immediately (reverts to pre-iter-59 fire-and-forget)
        #[arg(long)]
        no_wait: bool,
        /// Event dispatch mode: pointer (default), legacy (mouse events only), click-only
        #[arg(long, default_value = "pointer", value_name = "MODE")]
        dispatch: String,
        /// After clicking, wait for this condition. Repeatable. Forms: selector:<css>, text:<substr>, url:<regex>, gone:<css>
        #[arg(long, value_name = "PREDICATE", action = clap::ArgAction::Append)]
        wait_for: Vec<String>,
        /// Timeout in milliseconds for --wait-for predicates (default: same as --timeout)
        #[arg(long, value_name = "MS")]
        wait_for_timeout: Option<u64>,
        /// After clicking, wait for network and DOM to idle (no XHR/fetch for 500ms, no DOM mutations for 200ms)
        #[arg(long)]
        settle: bool,
    },
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
prototype setter so React/Vue/Svelte value trackers are invalidated, and `input`
and `change` events are dispatched after the assignment.

Output: {\"results\": {\"typed\": true, \"tag\": \"INPUT\", \"value\": \"...\"}, \"total\": 1, \"meta\": {...}}")]
    #[command(group(ArgGroup::new("type_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
    Type {
        /// CSS selector of the input element (positional, or use --selector)
        #[arg(group = "type_target")]
        selector_pos: Option<String>,
        /// Text to type into the element (positional, or use --text)
        text_pos: Option<String>,
        /// CSS selector of the input element (flag form)
        #[arg(long = "selector", value_name = "SELECTOR", group = "type_target")]
        selector_flag: Option<String>,
        /// Text to type into the element (flag form)
        #[arg(long = "text", value_name = "TEXT")]
        text_flag: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "type_target")]
        ref_id: Option<String>,
        /// Clear the element's current value before typing
        #[arg(long)]
        clear: bool,
        /// Skip auto-wait and type immediately (reverts to pre-iter-59 fire-and-forget)
        #[arg(long)]
        no_wait: bool,
        /// After typing, wait for this condition. Repeatable. Forms: selector:<css>, text:<substr>, url:<regex>, gone:<css>
        #[arg(long, value_name = "PREDICATE", action = clap::ArgAction::Append)]
        wait_for: Vec<String>,
        /// Timeout in milliseconds for --wait-for predicates (default: same as --timeout)
        #[arg(long, value_name = "MS")]
        wait_for_timeout: Option<u64>,
        /// After typing, wait for network and DOM to idle
        #[arg(long)]
        settle: bool,
    },
    /// Wait for a condition to become true (polls every 100ms).
    /// Exactly one of --selector, --text, --eval, or --ref must be specified.
    #[command(long_about = "Wait for a condition to become true (polls every 100ms).

Exactly one of --selector, --text, --eval, or --ref must be specified.

Use --ref <id> to wait for an element identified by its ARIA-tree ref ID
(daemon mode only). Equivalent to --selector but uses a stable ref handle.

Output: {\"results\": {\"matched\": true, \"elapsed_ms\": N, \"condition\": \"selector|text|eval\"}, \"total\": 1, \"meta\": {...}}")]
    #[command(group(ArgGroup::new("condition").required(true).multiple(false)))]
    Wait {
        /// Wait until an element matching this CSS selector exists in the DOM
        #[arg(long, group = "condition")]
        selector: Option<String>,
        /// Wait until this text appears anywhere on the page
        #[arg(long, group = "condition")]
        text: Option<String>,
        /// Wait until this JavaScript expression returns a truthy value
        #[arg(long, group = "condition")]
        eval: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "condition")]
        ref_id: Option<String>,
        /// Timeout in milliseconds before giving up (canonical flag — use this one).
        /// The legacy spelling `--wait-timeout` is also accepted as a hidden alias.
        #[arg(long = "timeout-ms", alias = "wait-timeout", default_value_t = 5000)]
        wait_timeout: u64,
    },
    /// List cookies via the Firefox StorageActor (includes httpOnly, secure, sameSite, etc.)
    #[command(
        long_about = "List cookies via the Firefox StorageActor (includes httpOnly, secure, sameSite, etc.).

Output: {\"results\": [{\"name\": \"...\", \"value\": \"...\", \"domain\": \"...\", \"path\": \"...\", \"secure\": true, \"httpOnly\": true}], \"total\": N, \"meta\": {...}}"
    )]
    Cookies {
        /// Filter by cookie name (exact match)
        #[arg(long)]
        name: Option<String>,
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
        include_document_cookie: bool,
        /// Return only cookies from the StorageActor (skip `document.cookie` evaluation).
        /// Use this when you need the raw StorageActor view, e.g. to debug httpOnly cookies.
        #[arg(long)]
        storage_only: bool,
    },
    /// Read web storage (localStorage or sessionStorage)
    #[command(long_about = "Read web storage (localStorage or sessionStorage).

Output: {\"results\": [{\"key\": \"...\", \"value\": \"...\"}], \"total\": N, \"meta\": {...}}
With --key: {\"results\": {\"key\": \"...\", \"value\": \"...\"}, \"total\": 1, \"meta\": {...}}")]
    Storage {
        /// Storage type: "local" (or "localStorage") / "session" (or "sessionStorage")
        storage_type: String,
        /// Get a specific key only
        #[arg(long)]
        key: Option<String>,
    },
    /// Inspect accessibility tree and check WCAG compliance
    #[command(long_about = "Inspect accessibility tree and check WCAG compliance.

Output: {\"results\": {\"role\": \"...\", \"name\": \"...\", \"children\": [...]}, \"total\": 1, \"meta\": {...}}
With a11y summary: {\"results\": [{\"role\": \"...\", \"name\": \"...\", \"level\": N}], \"total\": N, \"meta\": {...}}
With a11y contrast: {\"results\": [{\"selector\": \"...\", \"ratio\": N, \"passes_aa\": bool, ...}], \"total\": N, \"meta\": {...}}")]
    #[command(group(ArgGroup::new("a11y_target").required(false).multiple(false).args(["selector", "ref_id"])))]
    A11y {
        #[command(subcommand)]
        a11y_command: Option<A11yCommand>,

        /// Maximum tree depth to traverse (default: 6)
        #[arg(long, default_value_t = 6)]
        depth: u32,
        /// Maximum total characters of text content to include (default: 50000)
        #[arg(long, default_value_t = 50000)]
        max_chars: u32,
        /// CSS selector to root the tree at a specific element
        #[arg(long, group = "a11y_target")]
        selector: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "a11y_target")]
        ref_id: Option<String>,
        /// Only show interactive elements (buttons, links, inputs, etc.)
        #[arg(long)]
        interactive: bool,
        /// Surface only nodes that fail a basic WCAG audit (e.g. `<img>` without
        /// alt, form controls without an accessible name). Returns a flat array
        /// of violation records `{role, name?, selector, violation, severity}`
        /// instead of the full accessibility tree; empty when nothing critical.
        #[arg(long, conflicts_with = "interactive")]
        critical: bool,
    },
    /// Reload the page
    #[command(long_about = "Reload the page.

With --wait-idle, the command blocks after reload until network activity has been
idle for --idle-ms (default 500) or the --reload-timeout expires (default 10000).

Pass --hard for a cache-bypassing reload (Firefox `options.force`, the
protocol equivalent of Cmd-Shift-R / `LoadFlags::BYPASS_CACHE`).  Default
remains a soft reload.

Examples:
  ff-rdp reload
  ff-rdp reload --hard
  ff-rdp reload --wait-idle
  ff-rdp reload --hard --wait-idle --idle-ms 1000 --reload-timeout 30000

Output (plain):    {\"results\": {\"action\": \"reload\"[, \"force\": true]}, \"total\": 1, \"meta\": {...}}
Output (wait-idle): {\"results\": {\"reloaded\": true, \"idle_at_ms\": N, \"requests_observed\": M[, \"force\": true]}, \"total\": 1, \"meta\": {...}}")]
    Reload {
        /// Block until network is idle after reload
        #[arg(long)]
        wait_idle: bool,
        /// Milliseconds of network inactivity that counts as idle (--wait-idle only)
        #[arg(long, default_value_t = 500, requires = "wait_idle")]
        idle_ms: u64,
        /// Maximum total milliseconds to wait for idle (--wait-idle only)
        #[arg(long, default_value_t = 10000, requires = "wait_idle")]
        reload_timeout: u64,
        /// Hard reload — bypass the HTTP cache (sends Firefox's `options.force`,
        /// equivalent to Cmd-Shift-R in the browser UI). Default is a soft reload.
        #[arg(long)]
        hard: bool,
    },
    /// Go back in history
    #[command(long_about = "Navigate back in browser history.

Output: {\"results\": {\"action\": \"back\"}, \"total\": 1, \"meta\": {...}}")]
    Back,
    /// Go forward in history
    #[command(long_about = "Navigate forward in browser history.

Output: {\"results\": {\"action\": \"forward\"}, \"total\": 1, \"meta\": {...}}")]
    Forward,
    /// Inspect a remote JavaScript object by its grip actor ID
    #[command(long_about = "Inspect a remote JavaScript object by its grip actor ID.

Actor IDs appear in eval results when the return value is a non-primitive
(e.g. {\"type\": \"object\", \"actor\": \"server1.conn0.child0/obj12\", ...}).
Use --depth to control how many levels of nested objects are resolved.

Output: {\"results\": {\"actor\": \"...\", \"prototype\": {...}, \"ownProperties\": {...}}, \"total\": 1, \"meta\": {...}}")]
    Inspect {
        /// The actor ID of the object grip to inspect
        actor_id: String,
        /// Recursion depth for nested objects (default: 1)
        #[arg(long, default_value_t = 1)]
        depth: u32,
    },
    /// List JavaScript/WASM sources loaded on the page
    #[command(long_about = "List JavaScript/WASM sources loaded on the page.

Output: {\"results\": [{\"url\": \"...\", \"actor\": \"...\", \"isBlackBoxed\": bool}], \"total\": N, \"meta\": {...}}")]
    Sources {
        /// Filter sources by URL substring
        #[arg(long)]
        filter: Option<String>,
        /// Filter sources by URL regex pattern
        #[arg(long)]
        pattern: Option<String>,
    },
    /// Dump structured page snapshot for LLM consumption: DOM tree with semantic roles,
    /// key attributes, interactive elements, and text content
    #[command(
        long_about = "Dump structured page snapshot for LLM consumption: DOM tree with semantic roles, key attributes, interactive elements, and text content.

Output: {\"results\": {\"tag\": \"HTML\", \"children\": [...], ...}, \"total\": 1, \"meta\": {...}}"
    )]
    Snapshot {
        /// Maximum tree depth to traverse (default: 6). Alias: --max-depth.
        #[arg(long, default_value_t = 6)]
        depth: u32,
        /// Maximum tree depth to traverse (alias for --depth, matches `dom tree --max-depth` / CDP convention).
        /// Mutually exclusive with --depth. Must be ≥ 1.
        #[arg(long, value_name = "N", conflicts_with = "depth")]
        max_depth: Option<u32>,
        /// Maximum total characters of text content to include (default: 50000)
        #[arg(long, default_value_t = 50000)]
        max_chars: u32,
    },
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
    #[command(group(ArgGroup::new("geo_target").required(true).multiple(false).args(["selectors", "ref_id"])))]
    Geometry {
        /// One or more CSS selectors to query
        #[arg(group = "geo_target")]
        selectors: Vec<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "geo_target")]
        ref_id: Option<String>,
        /// Include hidden elements (zero-size, display:none, visibility:hidden, opacity:0).
        /// By default these are excluded.
        #[arg(long)]
        include_hidden: bool,
    },
    /// Test responsive layout across viewport widths: resize to each width,
    /// collect geometry + computed styles for the given selectors, then restore
    /// the original viewport size.  Returns results keyed by breakpoint width.
    #[command(long_about = "Test responsive layout across viewport widths.

Resizes to each width, collects element geometry at each breakpoint, then restores the original viewport.

By default, hidden and zero-sized elements are excluded from results at each breakpoint.
Pass --include-hidden to receive those elements as well.

Output: {\"results\": {\"breakpoints\": [{\"width\": 320, \"viewport\": {\"width\": N, \"height\": N}, \"elements\": [{\"selector\": \"...\", \"rect\": {...}, \"visible\": bool}]}, ...], \"original_viewport\": {\"width\": N, \"height\": N}}, \"total\": N, \"meta\": {...}}")]
    #[command(group(ArgGroup::new("resp_target").required(true).multiple(false).args(["selectors", "ref_id"])))]
    Responsive {
        /// One or more CSS selectors to query at each breakpoint
        #[arg(group = "resp_target")]
        selectors: Vec<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "resp_target")]
        ref_id: Option<String>,
        /// Comma-separated viewport widths in pixels
        #[arg(long, value_delimiter = ',', default_value = "320,768,1024,1440")]
        widths: Vec<u32>,
        /// Include hidden elements (zero-size, display:none, visibility:hidden, opacity:0).
        /// By default these are excluded.
        #[arg(long)]
        include_hidden: bool,
    },
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
    #[command(group(ArgGroup::new("computed_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
    Computed {
        /// CSS selector to match elements (positional, or use --selector)
        #[arg(group = "computed_target")]
        selector_pos: Option<String>,
        /// CSS selector to match elements (flag form)
        #[arg(long = "selector", value_name = "SELECTOR", group = "computed_target")]
        selector_flag: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "computed_target")]
        ref_id: Option<String>,
        /// Return only specific property values (repeatable: --prop color --prop font-size).
        /// Also accepts CSS custom properties like --prop=--bg-color.
        /// Comma-separated lists are also accepted: --prop color,font-size,--bg-color
        #[arg(long, value_name = "NAME", action = clap::ArgAction::Append)]
        prop: Vec<String>,
        /// Include every resolved property, not just non-default values
        #[arg(long, conflicts_with = "prop")]
        all: bool,
    },
    /// Inspect CSS styles for an element matching a CSS selector
    #[command(
        long_about = "Inspect CSS styles for an element matching a CSS selector.

Output (computed):  {\"results\": [{\"selector\": \"...\", \"computed\": {\"color\": \"...\", ...}}], \"total\": N, \"meta\": {...}}
Output (--applied): {\"results\": [{\"selector\": \"...\", \"rules\": [{\"selector\": \"...\", \"properties\": [...]}]}], \"total\": N, \"meta\": {...}}
Output (--layout):  {\"results\": [{\"selector\": \"...\", \"box\": {\"margin\": {...}, \"border\": {...}, \"padding\": {...}, \"content\": {...}}}], \"total\": N, \"meta\": {...}}"
    )]
    #[command(group(ArgGroup::new("styles_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
    Styles {
        /// CSS selector to match the element (positional, or use --selector)
        #[arg(group = "styles_target")]
        selector_pos: Option<String>,
        /// CSS selector to match the element (flag form)
        #[arg(long = "selector", value_name = "SELECTOR", group = "styles_target")]
        selector_flag: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "styles_target")]
        ref_id: Option<String>,
        /// Show applied CSS rules with source locations instead of computed styles
        #[arg(long, group = "style_mode")]
        applied: bool,
        /// Show box model layout (margin/border/padding/content) instead of computed styles
        #[arg(long, group = "style_mode")]
        layout: bool,
        /// Comma-separated list of CSS property names to include (computed mode only)
        #[arg(long, value_delimiter = ',', conflicts_with_all = ["applied", "layout"])]
        properties: Option<Vec<String>>,
    },
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
    #[command(group(ArgGroup::new("cascade_target").required(false).multiple(false).args(["selector_pos", "selector_flag", "ref_id"])))]
    Cascade {
        /// CSS selector to match the element (positional, or use --selector)
        #[arg(group = "cascade_target")]
        selector_pos: Option<String>,
        /// CSS selector to match the element (flag form)
        #[arg(long = "selector", value_name = "SELECTOR", group = "cascade_target")]
        selector_flag: Option<String>,
        /// ARIA-tree ref ID from a previous dom/snapshot call (daemon mode only, e.g. 'e3')
        #[arg(long = "ref", value_name = "REF_ID", group = "cascade_target")]
        ref_id: Option<String>,
        /// CSS property to explain (e.g. `--prop display`).  Defaults to all
        /// properties declared on the element.
        #[arg(long, value_name = "NAME", conflicts_with = "all")]
        prop: Option<String>,
        /// Explain every property declared on the element (the default).
        #[arg(long)]
        all: bool,
        /// Dump the raw PageStyle `getApplied` reply to stderr before parsing.
        /// Use to diagnose field-name drift between ff-rdp and Firefox.
        #[arg(long)]
        debug_raw: bool,
    },
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

Examples:
  ff-rdp launch                      # launch with temp profile on port 6000
  ff-rdp launch --headless           # headless mode (no visible window)
  ff-rdp launch --port 9222          # use a different debug port
  ff-rdp launch --auto-consent       # auto-dismiss cookie banners
  ff-rdp launch --profile ~/my-prof  # reuse an existing profile

Output: {\"results\": {\"pid\": N, \"host\": \"...\", \"port\": N, \"headless\": bool, \"profile\": \"...\", \"temp_profile\": bool, \"auto_consent\": bool}, \"total\": 1, \"meta\": {...}}"
    )]
    Launch {
        /// Run Firefox in headless mode
        #[arg(long)]
        headless: bool,
        /// Path to a Firefox profile directory
        #[arg(long, conflicts_with = "temp_profile")]
        profile: Option<String>,
        /// Create a temporary profile for a clean session
        #[arg(long, conflicts_with = "profile")]
        temp_profile: bool,
        /// Override the debug server port (defaults to --port value)
        #[arg(long)]
        debug_port: Option<u16>,
        /// Install Consent-O-Matic extension to auto-dismiss cookie consent banners
        #[arg(long)]
        auto_consent: bool,
        /// If the debug port is already occupied, stop the prior Firefox instance
        /// gracefully (SIGTERM → SIGKILL after 2 s) and then launch a fresh one.
        /// Alias: --force.
        #[arg(long)]
        replace: bool,
        /// Alias for --replace (stop the prior instance and relaunch).
        #[arg(long, hide = true)]
        force: bool,
    },
    /// Install Claude Code skill files to the user or project filesystem
    #[command(
        name = "install-skill",
        long_about = "Install bundled Claude Code skill files to the filesystem.

ff-rdp ships with Claude Code skills (e.g. ff-rdp-debug) that can be installed
into ~/.claude/skills/ (--user, default) or <git-root>/.claude/skills/ (--project).

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
  ff-rdp run login.json --record session.json")]
    Run {
        /// Path to the script file (.json or .yaml)
        script: std::path::PathBuf,
        /// Ad-hoc variable overrides (format: KEY=VALUE)
        #[arg(long = "vars", value_name = "KEY=VALUE", action = clap::ArgAction::Append)]
        vars: Vec<String>,
        /// Load variables from a dotenv-style file (values go to {{vars.X}}, not the process env)
        #[arg(long = "vars-file", value_name = "PATH")]
        vars_file: Option<std::path::PathBuf>,
        /// Deprecated alias for --vars-file (will be removed in a future release)
        #[arg(long = "env-file", value_name = "PATH", hide = true)]
        env_file: Option<std::path::PathBuf>,
        /// Continue running steps after a failure (default: stop on first failure)
        #[arg(long = "continue-on-failure")]
        continue_on_failure: bool,
        /// Parse and validate the script; resolve variables; print steps without executing
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Show secret values in step output (default: redact fields matching *password*, *token*, *secret*)
        #[arg(long = "show-secrets")]
        show_secrets: bool,
        /// Record executed steps to this file
        #[arg(long = "record", value_name = "OUTPUT")]
        record: Option<std::path::PathBuf>,
        /// Fail the run if recording a step fails (default: log to stderr and continue)
        #[arg(long = "record-strict")]
        record_strict: bool,
        /// Force a specific input format (json|yaml), overriding file extension detection
        #[arg(long = "script-format", value_name = "FORMAT")]
        script_format: Option<String>,
        /// Page-map file for resolving page_map:/field:/api_route: targets.
        /// Falls back to .ffrdp/page-map.json when this flag is not set and that file exists.
        #[arg(long = "page-map", value_name = "PATH")]
        page_map: Option<std::path::PathBuf>,
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
        allow_env: Vec<String>,
        /// Allow sub-script `run:` paths that are absolute or escape the
        /// top-level script's directory. Only enable when you author every
        /// file in the include chain.
        #[arg(long = "allow-unsafe-script-paths")]
        allow_unsafe_script_paths: bool,
    },

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
    Index {
        /// Base URL to crawl (defaults to the current daemon tab's origin)
        base_url: Option<String>,

        /// Output path for the page-map file
        #[arg(long, default_value = ".ffrdp/page-map.json")]
        out: std::path::PathBuf,

        /// Maximum crawl depth from the base URL
        #[arg(long, default_value_t = 2)]
        depth: u32,

        /// Maximum number of pages to crawl
        #[arg(long = "max-pages", default_value_t = 50)]
        max_pages: usize,

        /// Only crawl URLs matching this regex
        #[arg(long)]
        include: Option<String>,

        /// Skip URLs matching this regex
        #[arg(long)]
        exclude: Option<String>,

        /// Output format: json (default) or yaml
        #[arg(long, default_value = "json")]
        format: String,

        /// Also crawl cross-origin links (default: same-origin only)
        #[arg(long)]
        cross_origin: bool,

        /// Ignore robots.txt (useful for internal admin tools)
        #[arg(long)]
        ignore_robots: bool,

        /// Load Netscape-format cookie jar before crawling
        #[arg(long, value_name = "PATH")]
        cookies_from: Option<std::path::PathBuf>,

        /// Inject Authorization: Bearer <token> on each navigate
        #[arg(long, value_name = "TOKEN")]
        bearer: Option<String>,

        /// Run this iter-61 script before crawling (for authentication)
        #[arg(long, value_name = "PATH")]
        login_script: Option<std::path::PathBuf>,

        /// Check mode: re-crawl and report drifted selectors vs. an existing map
        #[arg(long, conflicts_with_all = ["out", "format"])]
        check: bool,

        /// Existing page-map to check against (--check mode only)
        #[arg(long, value_name = "PATH", requires = "check")]
        page_map: Option<std::path::PathBuf>,

        /// Write drift report to this file (--check mode only, default: stdout)
        #[arg(long, value_name = "PATH", requires = "check")]
        report: Option<std::path::PathBuf>,

        /// Restrict output to paths under this directory (rejects path traversal)
        #[arg(long, value_name = "DIR")]
        output_root: Option<std::path::PathBuf>,
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
    Contrast {
        /// CSS selector to limit checking (default: all text elements)
        #[arg(long)]
        selector: Option<String>,
        /// Only show elements that fail AA contrast requirements
        #[arg(long)]
        fail_only: bool,
    },
    /// Flat summary: landmarks, headings, and interactive elements for quick page orientation
    Summary,
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
    },
    /// Scroll to the very top of the page (equivalent to scroll by --dy -99999999)
    #[command(long_about = "Scroll to the very top of the page.
  Uses window.scrollTo(0, 0) for an instant jump to the top.
Output: {\"results\": {\"scrolled\": true, \"viewport\": {...}, \"scrollHeight\": N, \"atEnd\": bool}, \"total\": 1, \"meta\": {...}}")]
    Top,
    /// Scroll to the very bottom of the page (equivalent to scroll by --dy 99999999)
    #[command(long_about = "Scroll to the very bottom of the page.
  Uses window.scrollTo(0, document.documentElement.scrollHeight) for an instant jump to the bottom.
Output: {\"results\": {\"scrolled\": true, \"viewport\": {...}, \"scrollHeight\": N, \"atEnd\": bool}, \"total\": 1, \"meta\": {...}}")]
    Bottom,
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
not succeed within 2 seconds. Cleans up daemon.json on success.

Output: {\"results\": {\"stopped\": bool}, \"total\": 1, \"meta\": {...}}")]
    Stop,
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
