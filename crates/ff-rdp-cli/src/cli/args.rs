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
        /// CSS selector to match elements
        selector: String,
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
    /// Show network requests
    Network {
        /// Filter by URL pattern (substring match)
        #[arg(long)]
        filter: Option<String>,
        /// Filter by HTTP method (GET, POST, etc.)
        #[arg(long)]
        method: Option<String>,
        /// Use Performance Resource Timing API for retrospective data instead of WatcherActor
        #[arg(long)]
        cached: bool,
    },
    /// Capture a screenshot
    Screenshot {
        /// Output file path
        #[arg(long, short)]
        output: Option<String>,
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
