use clap::{Parser, Subcommand};

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
    /// Wait for a condition to become true (polls every 100ms)
    Wait {
        /// Wait until an element matching this CSS selector exists in the DOM
        #[arg(long)]
        selector: Option<String>,
        /// Wait until this text appears anywhere on the page
        #[arg(long)]
        text: Option<String>,
        /// Wait until this JavaScript expression returns a truthy value
        #[arg(long)]
        eval: Option<String>,
        /// Timeout in milliseconds before giving up
        #[arg(long, default_value_t = 5000)]
        wait_timeout: u64,
    },
    /// Reload the page
    Reload,
    /// Go back in history
    Back,
    /// Go forward in history
    Forward,
}
