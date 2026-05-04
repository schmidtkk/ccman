use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "ccswitch")]
#[command(version = "2.0.0")]
#[command(about = "Claude Code provider switcher")]
#[command(subcommand_required = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List all available providers
    List,

    /// Switch to a provider
    Use {
        /// Provider name (kimi, glm, minimax, zhongzhuan, claude)
        provider: String,
    },

    /// Show current provider status
    Status,

    /// Manage API keys
    Key {
        #[command(subcommand)]
        command: KeyCommands,
    },

    /// Usage tracking and reporting
    Usage {
        #[command(subcommand)]
        command: UsageCommands,
    },

    /// Health check for API keys
    Health {
        #[command(subcommand)]
        command: HealthCommands,
    },

    /// Manage providers
    Provider {
        #[command(subcommand)]
        command: ProviderCommands,
    },

    /// Launch interactive TUI
    Tui,
}

#[derive(Subcommand, Debug)]
pub enum KeyCommands {
    /// Add an API key for a provider
    Add {
        /// Provider name
        #[arg(short, long)]
        provider: String,

        /// The API key value
        key: String,

        /// Optional label for the key
        #[arg(short, long)]
        label: Option<String>,

        /// Priority for key rotation (lower = higher priority)
        #[arg(short, long)]
        priority: Option<i32>,
    },

    /// List API keys for a provider
    List {
        /// Provider name (optional, lists all if omitted)
        #[arg(short, long)]
        provider: Option<String>,
    },

    /// Remove an API key by ID
    Remove {
        /// Key ID
        id: i64,
    },
}

#[derive(Subcommand, Debug)]
pub enum UsageCommands {
    /// Show today's usage
    Today,

    /// Show this month's daily usage
    Month,

    /// Show total usage for a date range (defaults to today)
    Total {
        /// Start date (YYYY-MM-DD)
        #[arg(short, long)]
        start: Option<String>,

        /// End date (YYYY-MM-DD)
        #[arg(short, long)]
        end: Option<String>,

        /// Group by provider
        #[arg(short, long)]
        by_provider: bool,
    },

    /// Show recent usage logs
    Logs {
        /// Number of entries to show
        #[arg(short, long, default_value = "20")]
        limit: i64,
    },

    /// Manually log a usage entry
    Log {
        /// Provider name
        #[arg(short, long)]
        provider: String,

        /// Model name
        #[arg(short, long)]
        model: String,

        /// Prompt tokens
        #[arg(short, long)]
        prompt: i64,

        /// Completion tokens
        #[arg(short, long)]
        completion: i64,

        /// Optional request ID
        #[arg(short, long)]
        request_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum HealthCommands {
    /// Run health check on all keys or a specific provider
    Check {
        /// Provider name (optional, checks all if omitted)
        provider: Option<String>,
    },

    /// Show latest health status for all keys
    Status,
}

#[derive(Subcommand, Debug)]
pub enum ProviderCommands {
    /// Add a new provider
    Add {
        /// Provider identifier (e.g., openrouter, deepseek)
        name: String,

        /// Display name (e.g., "OpenRouter")
        display_name: String,

        /// Base URL for API requests
        base_url: String,

        /// Default model (optional)
        #[arg(short, long)]
        model: Option<String>,

        /// Auth header format (default: "Authorization: Bearer")
        #[arg(long, default_value = "Authorization: Bearer")]
        auth_header: String,

        /// Request timeout in milliseconds (default: 60000)
        #[arg(long, default_value_t = 60000)]
        timeout_ms: i64,

        /// Disable non-essential traffic for this provider
        #[arg(long, default_value_t = false)]
        requires_disable_traffic: bool,
    },

    /// List all providers
    List,

    /// Edit an existing provider
    Edit {
        /// Provider name
        name: String,

        /// New display name
        #[arg(long)]
        display_name: Option<String>,

        /// New base URL
        #[arg(long)]
        base_url: Option<String>,

        /// New model
        #[arg(long)]
        model: Option<String>,

        /// New auth header
        #[arg(long)]
        auth_header: Option<String>,

        /// New timeout in milliseconds
        #[arg(long)]
        timeout_ms: Option<i64>,

        /// Toggle disable non-essential traffic
        #[arg(long)]
        requires_disable_traffic: Option<bool>,
    },

    /// Remove a provider by name
    Remove {
        /// Provider name
        name: String,
    },
}
