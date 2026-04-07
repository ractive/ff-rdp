use clap::{ArgGroup, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ff-rdp", about = "Firefox Remote Debugging Protocol CLI")]
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

    /// Don't use or start a daemon (connect directly to Firefox)
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

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// List open browser tabs
    Tabs,
    /// Navigate to a URL
    Navigate {
        /// The URL to navigate to
        url: String,
        /// Also capture network requests made during navigation
        #[arg(long)]
        with_network: bool,
        /// After navigating, wait for this text to appear on the page
        #[arg(long, conflicts_with = "wait_selector")]
        wait_text: Option<String>,
        /// After navigating, wait for this CSS selector to match
        #[arg(long, conflicts_with = "wait_text")]
        wait_selector: Option<String>,
        /// Timeout for wait condition in milliseconds [default: 5000]
        #[arg(long, default_value_t = 5000)]
        wait_timeout: u64,
    },
    /// Evaluate JavaScript in the target tab
    Eval {
        /// JavaScript expression to evaluate
        script: String,
    },
    /// Extract visible page text (document.body.innerText)
    PageText,
    /// Query DOM elements by CSS selector
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
    Console {
        /// Filter by log level (error, warn, info, log, debug)
        #[arg(long)]
        level: Option<String>,
        /// Filter by message content (regex pattern)
        #[arg(long)]
        pattern: Option<String>,
    },
    /// Show network requests captured by the WatcherActor.
    ///
    /// In direct mode (--no-daemon), only requests made after connection are
    /// reliably captured. Use the daemon (default) for continuous buffering
    /// that survives across commands, or use `navigate --with-network` to
    /// capture requests triggered by a navigation.
    #[command(long_about = "Show network requests captured by the WatcherActor.

In direct mode (--no-daemon), only requests made after the connection is
established are reliably captured. Requests completed before ff-rdp connects
will typically not appear.

Recommended workflows:
  - Daemon mode (default): run `ff-rdp` without --no-daemon so the daemon
    buffers events continuously across commands.
  - Navigate with capture: use `ff-rdp navigate --with-network <url>` to
    start network monitoring before the page load begins.

The --filter and --method flags narrow results after capture; they do not
affect which requests Firefox records.")]
    Network {
        /// Filter by URL pattern (substring match)
        #[arg(long)]
        filter: Option<String>,
        /// Filter by HTTP method (GET, POST, etc.)
        #[arg(long)]
        method: Option<String>,
    },
    /// Query browser Performance API entries and Core Web Vitals
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
    Screenshot {
        /// Output file path
        #[arg(long, short, conflicts_with = "base64")]
        output: Option<String>,
        /// Return the screenshot as base64 PNG data in JSON output instead of saving to a file
        #[arg(long, conflicts_with = "output")]
        base64: bool,
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
    Cookies {
        /// Filter by cookie name (exact match)
        #[arg(long)]
        name: Option<String>,
    },
    /// Read web storage (localStorage or sessionStorage)
    Storage {
        /// Storage type: "local" or "session"
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
    Reload,
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
