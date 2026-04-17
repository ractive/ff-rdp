use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "ff-rdp",
    about = "Firefox Remote Debugging Protocol CLI",
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

    #[command(subcommand)]
    pub command: Command,
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

Output: {\"results\": <value>, \"total\": 1, \"meta\": {...}}")]
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
    },
    /// Extract visible page text (document.body.innerText)
    PageText,
    /// Query DOM elements by CSS selector
    #[command(long_about = "Query DOM elements by CSS selector.

Output: {\"results\": [\"<html_string>\", ...], \"total\": N, \"meta\": {...}}
With --count: {\"results\": {\"count\": N}, \"total\": 1, \"meta\": {...}}")]
    Dom {
        #[command(subcommand)]
        dom_command: Option<DomCommand>,

        /// CSS selector to match elements
        selector: Option<String>,
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
output metadata and lack HTTP status codes.

Recommended workflows:
  - Daemon mode (default): run `ff-rdp` without --no-daemon so the daemon
    buffers events continuously across commands.
  - Navigate with capture: use `ff-rdp navigate --with-network <url>` to
    start network monitoring before the page load begins.

The --filter and --method flags narrow results after capture; they do not
affect which requests Firefox records.

Default: 20 results, sorted by duration (slowest first).
Output (summary mode): {\"results\": {\"total_requests\": N, \"total_transfer_bytes\": N, \"by_cause_type\": {...}, \"slowest\": [...], \"timeout_reached\": false}, \"total\": N, \"meta\": {...}}
Output (--detail): {\"results\": [{\"url\": \"...\", \"method\": \"GET\", \"status\": 200, \"duration_ms\": N, ...}], \"total\": N, \"meta\": {...}}")]
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
    Click {
        /// CSS selector of the element to click
        selector: String,
    },
    /// Type text into an input element matching a CSS selector
    Type {
        /// CSS selector of the input element
        selector: String,
        /// Text to type into the element
        text: String,
        /// Clear the element's current value before typing
        #[arg(long)]
        clear: bool,
    },
    /// Wait for a condition to become true (polls every 100ms).
    /// Exactly one of --selector, --text, or --eval must be specified.
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
    Storage {
        /// Storage type: "local" (or "localStorage") / "session" (or "sessionStorage")
        storage_type: String,
        /// Get a specific key only
        #[arg(long)]
        key: Option<String>,
    },
    /// Inspect accessibility tree and check WCAG compliance
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
        #[arg(long)]
        selector: Option<String>,
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
    Back,
    /// Go forward in history
    Forward,
    /// Inspect a remote JavaScript object by its grip actor ID
    Inspect {
        /// The actor ID of the object grip to inspect
        actor_id: String,
        /// Recursion depth for nested objects (default: 1)
        #[arg(long, default_value_t = 1)]
        depth: u32,
    },
    /// List JavaScript/WASM sources loaded on the page
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
    Daemon,
    /// Show curated jq one-liners and recipes for common tasks
    Recipes,
    /// Dump complete CLI reference in compact LLM-friendly format (all commands, flags, examples)
    LlmHelp,
    /// Get element geometry: bounding rects, position, z-index, visibility, overflow,
    /// with automatic overlap detection between elements
    Geometry {
        /// One or more CSS selectors to query
        #[arg(required = true)]
        selectors: Vec<String>,
        /// Exclude invisible elements (zero-size, display:none, visibility:hidden, opacity:0)
        #[arg(long)]
        visible_only: bool,
    },
    /// Test responsive layout across viewport widths: resize to each width,
    /// collect geometry + computed styles for the given selectors, then restore
    /// the original viewport size.  Returns results keyed by breakpoint width.
    Responsive {
        /// One or more CSS selectors to query at each breakpoint
        #[arg(required = true)]
        selectors: Vec<String>,
        /// Comma-separated viewport widths in pixels
        #[arg(long, value_delimiter = ',', default_value = "320,768,1024,1440")]
        widths: Vec<u32>,
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
    Computed {
        /// CSS selector to match elements
        selector: String,
        /// Return only a single property value (e.g. \"color\", \"display\")
        #[arg(long, value_name = "NAME")]
        prop: Option<String>,
        /// Include every resolved property, not just non-default values
        #[arg(long, conflicts_with = "prop")]
        all: bool,
    },
    /// Inspect CSS styles for an element matching a CSS selector
    Styles {
        /// CSS selector to match the element
        selector: String,
        /// Show applied CSS rules with source locations instead of computed styles
        #[arg(long, group = "style_mode")]
        applied: bool,
        /// Show box model layout (margin/border/padding/content) instead of computed styles
        #[arg(long, group = "style_mode")]
        layout: bool,
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
Output: {\"results\": {\"scrolled\": true, \"selector\": \"...\", \"viewport\": {...}, \"target\": {...}, \"atEnd\": bool}, \"total\": 1, \"meta\": {...}}")]
    To {
        /// CSS selector of the element to scroll into view
        selector: String,
        /// Block alignment [default: top]. Aliases: top=start, bottom=end
        #[arg(long, value_enum, default_value_t = ScrollBlock::Top)]
        block: ScrollBlock,
        /// Use smooth scrolling behavior (default is instant)
        #[arg(long)]
        smooth: bool,
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
