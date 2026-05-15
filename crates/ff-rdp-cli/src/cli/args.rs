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
    ff-rdp geometry <SEL>... [--include-hidden]
    ff-rdp responsive <SEL>... [--widths W1,W2,...]

  Accessibility:
    ff-rdp a11y [--depth N] [--selector SEL] [--interactive]
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

#[derive(Parser)]
#[command(
    name = "ff-rdp",
    about = "Firefox Remote Debugging Protocol CLI\n\nQuick start:  ff-rdp launch          # start Firefox with debugging enabled\n              ff-rdp navigate <URL>   # open a page",
    long_about = "Firefox Remote Debugging Protocol CLI

Quick start:
  ff-rdp launch                   Launch a new Firefox instance with remote debugging
  ff-rdp launch --headless        Launch headless (no visible window)
  ff-rdp navigate https://example.com

'ff-rdp launch' starts a separate Firefox process that won't interfere with
any already-running Firefox windows — it uses a temporary profile and
the -no-remote flag automatically.",
    after_help = "Tip: Run 'ff-rdp launch' first to start Firefox with remote debugging.\n     It won't affect any existing Firefox windows — safe to run alongside\n     your normal browser.",
    after_long_help = AFTER_LONG_HELP,
    version
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

    /// Operation timeout in milliseconds
    #[arg(long, default_value_t = 5000, global = true)]
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

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Returns `true` when internal debug messages should be printed to stderr.
    ///
    /// Enabled by `--verbose` or by having `RUST_LOG` set (the latter implies
    /// that the caller already opted into structured logging output).
    pub fn is_verbose(&self) -> bool {
        self.verbose || std::env::var("RUST_LOG").is_ok()
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

The URL is a positional argument (not a flag). There is no --url option.

Examples:
  ff-rdp navigate https://example.com
  ff-rdp navigate https://example.com --with-network
  ff-rdp navigate https://example.com --wait-text \"Welcome\"

Output: {\"results\": {\"url\": \"...\", \"title\": \"...\"}, \"total\": 1, \"meta\": {...}}")]
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

By default the script is wrapped in an isolated IIFE so `const`/`let`
declarations don't leak across calls (Firefox's console actor shares scope
across evaluations otherwise, so two consecutive `eval 'const x = 1; x'`
calls would error with \"redeclaration of const x\"). Single expressions
like `1 + 1` still return their value.

Pass --no-isolate to opt out and share scope across calls — useful for
incrementally building up helpers in an interactive debugging session.

Output: {\"results\": <value>, \"total\": 1, \"meta\": {...}}

When the result is a non-primitive (object, array), Firefox returns actor grip
metadata (actor IDs, class names) instead of the actual values. Use --stringify
to wrap the expression in JSON.stringify() and get the real data back.")]
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
        /// Share scope across eval calls (skip the default IIFE isolation wrapping).
        /// Useful when incrementally building up helpers across an interactive session.
        #[arg(long)]
        no_isolate: bool,
    },
    /// Extract visible page text (document.body.innerText)
    #[command(long_about = "Extract visible page text (document.body.innerText).

Output: {\"results\": \"<page text as a plain string>\", \"total\": 1, \"meta\": {...}}")]
    PageText,
    /// Query DOM elements by CSS selector
    #[command(long_about = "Query DOM elements by CSS selector.

Default output (ARIA-tree JSON): {\"results\": {\"ref\":\"e1\",\"role\":\"heading\",\"name\":\"...\",\"level\":1,\"tag\":\"h1\",\"attrs\":{...}}, \"total\": N}
Each element has: ref (stable ID), role (ARIA semantic role), name (accessible name), tag, attrs (actionable only), state, level (headings).
Use --format html for the legacy raw HTML string output.
With --count: {\"results\": {\"count\": N}, \"total\": 1, \"meta\": {...}}")]
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
        /// Headers are fetched per-entry from the NetworkEventActor; not available
        /// for performance-api fallback entries (source=performance-api).
        #[arg(long)]
        headers: bool,
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
        /// Timeout in milliseconds before giving up
        #[arg(long, default_value_t = 5000)]
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
    },
    /// Reload the page
    #[command(long_about = "Reload the page.

With --wait-idle, the command blocks after reload until network activity has been
idle for --idle-ms (default 500) or the --reload-timeout expires (default 10000).

Examples:
  ff-rdp reload
  ff-rdp reload --wait-idle
  ff-rdp reload --wait-idle --idle-ms 1000 --reload-timeout 30000

Output (plain):    {\"results\": {\"action\": \"reload\"}, \"total\": 1, \"meta\": {...}}
Output (wait-idle): {\"results\": {\"reloaded\": true, \"idle_at_ms\": N, \"requests_observed\": M}, \"total\": 1, \"meta\": {...}}")]
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
        /// Maximum tree depth to traverse (default: 6)
        #[arg(long, default_value_t = 6)]
        depth: u32,
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
        /// Return only a single property value (e.g. \"color\", \"display\")
        #[arg(long, value_name = "NAME")]
        prop: Option<String>,
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
}

#[derive(Subcommand)]
pub enum PerfCommand {
    /// Compute Core Web Vitals summary (LCP, CLS, TBT, FCP, TTFB)
    Vitals,
    /// Aggregate resource summary: sizes, request counts by type, slowest resources, domain breakdown
    Summary,
    /// Full page performance audit: vitals, navigation timing, resource breakdown, DOM stats
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
