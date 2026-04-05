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
    },
    /// Evaluate JavaScript in the target tab
    Eval {
        /// JavaScript expression to evaluate
        script: String,
    },
    /// Get page information
    PageText,
    /// Read console messages
    Console,
    /// Capture a screenshot
    Screenshot {
        /// Output file path
        #[arg(long, short)]
        output: Option<String>,
    },
    /// Reload the page
    Reload,
    /// Go back in history
    Back,
    /// Go forward in history
    Forward,
}
