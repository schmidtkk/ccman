mod cli;
mod tui;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing::{info, debug};

use ccswitch_core::health_check::HealthCheckService;
use ccswitch_core::provider::ProviderService;
use ccswitch_core::settings::SettingsManager;
use ccswitch_core::usage_tracker::UsageTracker;
use ccswitch_db::migrations::run_migrations_from_dir;
use ccswitch_db::pool::DatabasePool;
use ccswitch_db::repositories::{
    ApiKeyRepository, ProviderRepository,
    SqliteApiKeyRepository, SqliteProviderRepository, SqliteSettingsRepository,
    SqliteUsageLogRepository, SqlitePricingRepository, SqliteHealthCheckRepository,
};

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let cli = cli::Cli::parse();

    // Initialize database and services
    let (provider_service, settings_manager, usage_repo, pricing_repo, health_repo, pool) = initialize()?;

    match cli.command {
        cli::Commands::List => {
            let providers = provider_service.list_providers()?;
            println!("Available providers:");
            for p in providers {
                println!("  {:12} - {}", p.name, p.display_name);
                if let Some(model) = &p.model {
                    println!("    model: {}", model);
                }
                println!("    endpoint: {}", p.base_url);
            }
        }
        cli::Commands::Use { provider } => {
            // Normalize provider aliases (e.g., "cc" -> "claude")
            let provider = match provider.as_str() {
                "cc" => "claude".to_string(),
                "zz" => "zhongzhuan".to_string(),
                _ => provider,
            };

            let env_vars = provider_service.switch_to_provider(&provider)?;

            if provider == "claude" {
                settings_manager.clear_env_vars()?;
                println!("Switched to native Claude");

                // Print unset commands for shell integration
                println!("\n# Run these commands in your shell:");
                for key in &ccswitch_core::provider::CCSWITCH_ENV_KEYS {
                    println!("unset {}", key);
                }
            } else {
                settings_manager.write_env_vars(&env_vars)?;
                println!("Switched to {}", provider);

                // Print export commands for shell integration
                println!("\n# Run these commands in your shell:");
                for (key, value) in env_vars.to_export_pairs() {
                    println!("export {}={}", key, shell_escape(&value));
                }
            }
        }
        cli::Commands::Status => {
            let active = provider_service.get_active_provider()?;

            match active {
                Some((provider, key)) => {
                    println!("Current provider: {} ({})", provider.display_name, provider.name);
                    if let Some(model) = &provider.model {
                        println!("Model: {}", model);
                    }
                    println!("Base URL: {}", provider.base_url);
                    println!("Key: {} ({} chars)",
                        key.key_label.as_deref().unwrap_or("unnamed"),
                        key.key_value.len()
                    );
                    if provider.timeout_ms > 0 {
                        println!("Timeout: {}ms", provider.timeout_ms);
                    }
                }
                None => {
                    println!("Current provider: Native Claude (no provider active)");
                }
            }

            // Show settings.json .env block
            if let Ok(Some(env)) = settings_manager.read_current_env() {
                println!("\nClaude settings.json .env:");
                println!("{}", serde_json::to_string_pretty(&env)?);
            }
        }
        cli::Commands::Key { command } => {
            match command {
                cli::KeyCommands::Add { provider, key, label, priority } => {
                    handle_key_add(&provider_service, &provider, key, label, priority)?;
                }
                cli::KeyCommands::List { provider } => {
                    handle_key_list(&provider_service, provider.as_deref())?;
                }
                cli::KeyCommands::Remove { id } => {
                    handle_key_remove(&provider_service, id)?;
                }
            }
        }
        cli::Commands::Usage { command } => {
            let tracker = UsageTracker::new(usage_repo, pricing_repo);
            match command {
                cli::UsageCommands::Today => {
                    handle_usage_today(&tracker)?;
                }
                cli::UsageCommands::Month => {
                    handle_usage_month(&tracker)?;
                }
                cli::UsageCommands::Total { start, end, by_provider } => {
                    handle_usage_total(&tracker, start, end, by_provider)?;
                }
                cli::UsageCommands::Logs { limit } => {
                    handle_usage_logs(&tracker, limit)?;
                }
                cli::UsageCommands::Log { provider, model, prompt, completion, request_id } => {
                    handle_usage_log(&provider_service, &tracker, &provider, &model, prompt, completion, request_id)?;
                }
            }
        }
        cli::Commands::Health { command } => {
            let service = HealthCheckService::new(
                SqliteProviderRepository::new(pool.clone()),
                SqliteApiKeyRepository::new(pool.clone()),
                health_repo,
            );
            match command {
                cli::HealthCommands::Check { provider } => {
                    handle_health_check(&service, provider)?;
                }
                cli::HealthCommands::Status => {
                    handle_health_status(&service)?;
                }
            }
        }
        cli::Commands::Tui => {
            tui::run(pool)?;
        }
    }

    Ok(())
}

fn initialize() -> Result<(
    ProviderService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteSettingsRepository,
    >,
    SettingsManager,
    SqliteUsageLogRepository,
    SqlitePricingRepository,
    SqliteHealthCheckRepository,
    DatabasePool,
)> {
    // Determine database path
    let home = dirs::home_dir().context("Could not determine home directory")?;
    let db_path = home.join(".ccswitch").join("ccswitch.db");
    debug!("Database path: {}", db_path.display());

    // Create database pool
    let pool = DatabasePool::new(&db_path)?;

    // Run migrations
    let exe_path = std::env::current_exe()?;
    let exe_dir = exe_path.parent().context("Could not get exe directory")?;

    // Try multiple locations for migrations
    let migrations_paths = [
        exe_dir.join("../migrations"),
        exe_dir.join("../../migrations"),
        PathBuf::from("/home/weidongguo/workspace/ccswitch/migrations"),
    ];

    let mut migrations_applied = false;
    for migrations_dir in &migrations_paths {
        if migrations_dir.exists() {
            run_migrations_from_dir(&pool, migrations_dir)?;
            migrations_applied = true;
            info!("Migrations applied from: {}", migrations_dir.display());
            break;
        }
    }

    if !migrations_applied {
        anyhow::bail!("Could not find migrations directory");
    }

    // Create repositories
    let provider_repo = SqliteProviderRepository::new(pool.clone());
    let api_key_repo = SqliteApiKeyRepository::new(pool.clone());
    let settings_repo = SqliteSettingsRepository::new(pool.clone());
    let usage_repo = SqliteUsageLogRepository::new(pool.clone());
    let pricing_repo = SqlitePricingRepository::new(pool.clone());
    let health_repo = SqliteHealthCheckRepository::new(pool.clone());

    // Import existing API keys from environment variables (migration from shell script)
    import_existing_keys(&provider_repo, &api_key_repo)?;

    // Create services
    let provider_service = ProviderService::new(provider_repo, api_key_repo, settings_repo);
    let settings_manager = SettingsManager::new()?;

    Ok((provider_service, settings_manager, usage_repo, pricing_repo, health_repo, pool))
}

/// Import existing API keys from environment variables into the database.
fn import_existing_keys(
    provider_repo: &SqliteProviderRepository,
    api_key_repo: &SqliteApiKeyRepository,
) -> Result<()> {
    let env_keys = [
        ("KIMI_API_KEY", "kimi"),
        ("GLM_API_KEY", "glm"),
        ("MINIMAX_API_KEY", "minimax"),
        ("ZHONGZHUAN_API_KEY", "zhongzhuan"),
    ];

    for (env_var, provider_name) in &env_keys {
        if let Ok(key_value) = std::env::var(env_var) {
            if key_value.is_empty() {
                continue;
            }

            // Check if provider exists and has no keys
            if let Ok(Some(provider)) = provider_repo.get_by_name(provider_name) {
                let count = api_key_repo.count_by_provider(provider.id)?;
                if count == 0 {
                    let key = ccswitch_db::models::ApiKey {
                        id: 0,
                        provider_id: provider.id,
                        key_value,
                        key_label: Some(format!("Imported from {}", env_var)),
                        is_active: true,
                        priority: 0,
                        daily_limit_cents: None,
                        monthly_limit_cents: None,
                        error_count: 0,
                        last_used_at: None,
                        last_error_at: None,
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    api_key_repo.create(&key)?;
                    tracing::info!(
                        "Imported API key for {} from {}",
                        provider_name,
                        env_var
                    );
                }
            }
        }
    }

    Ok(())
}

fn handle_key_add(
    provider_service: &ProviderService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteSettingsRepository,
    >,
    provider_name: &str,
    key_value: String,
    label: Option<String>,
    priority: Option<i32>,
) -> Result<()> {
    let id = provider_service.add_key(provider_name, key_value, label, priority)?;
    println!("Added key id={} for provider {}", id, provider_name);
    Ok(())
}

fn handle_key_list(
    provider_service: &ProviderService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteSettingsRepository,
    >,
    provider_name: Option<&str>,
) -> Result<()> {
    if let Some(name) = provider_name {
        let provider = provider_service
            .get_provider(name)?
            .with_context(|| format!("Provider not found: {}", name))?;
        let keys = provider_service.list_keys(name)?;

        println!("Keys for {}:", provider.display_name);
        if keys.is_empty() {
            println!("  (none)");
        } else {
            for k in keys {
                let status = if k.is_active { "active" } else { "inactive" };
                let label = k.key_label.as_deref().unwrap_or("unnamed");
                let preview = &k.key_value[..20.min(k.key_value.len())];
                println!(
                    "  {:3} | {:8} | p={} | errs={} | {}... | {}",
                    k.id, status, k.priority, k.error_count, preview, label
                );
            }
        }
    } else {
        let all = provider_service.list_all_keys()?;
        if all.is_empty() {
            println!("No keys stored.");
            return Ok(());
        }
        for (provider, keys) in all {
            println!("{}:", provider.display_name);
            for k in keys {
                let status = if k.is_active { "active" } else { "inactive" };
                let label = k.key_label.as_deref().unwrap_or("unnamed");
                let preview = &k.key_value[..20.min(k.key_value.len())];
                println!(
                    "  {:3} | {:8} | p={} | errs={} | {}... | {}",
                    k.id, status, k.priority, k.error_count, preview, label
                );
            }
        }
    }
    Ok(())
}

fn handle_key_remove(
    provider_service: &ProviderService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteSettingsRepository,
    >,
    id: i64,
) -> Result<()> {
    provider_service.remove_key(id)?;
    println!("Removed key {}", id);
    Ok(())
}

fn handle_usage_today(
    tracker: &UsageTracker<SqliteUsageLogRepository, SqlitePricingRepository>,
) -> Result<()> {
    let stats = tracker.today_stats()?;
    if stats.is_empty() {
        println!("No usage recorded today.");
        return Ok(());
    }
    let total = &stats[0];
    println!("Today's usage ({}):", total.day);
    println!("  Requests:     {}", total.requests);
    println!("  Prompt:       {} tokens", total.prompt_tokens);
    println!("  Completion:   {} tokens", total.completion_tokens);
    println!("  Total:        {} tokens", total.total_tokens);
    println!("  Cost:         {}", format_cents(total.cost_cents));
    Ok(())
}

fn handle_usage_month(
    tracker: &UsageTracker<SqliteUsageLogRepository, SqlitePricingRepository>,
) -> Result<()> {
    let stats = tracker.month_stats()?;
    if stats.is_empty() {
        println!("No usage recorded this month.");
        return Ok(());
    }

    println!("{:<12} {:>8} {:>12} {:>12} {:>12}", "Date", "Requests", "Prompt", "Completion", "Cost");
    println!("{}", "-".repeat(60));
    let mut total_requests = 0;
    let mut total_tokens = 0;
    let mut total_cost = 0i64;

    for stat in &stats {
        println!(
            "{:<12} {:>8} {:>12} {:>12} {:>12}",
            stat.day,
            stat.requests,
            stat.prompt_tokens,
            stat.completion_tokens,
            format_cents(stat.cost_cents),
        );
        total_requests += stat.requests;
        total_tokens += stat.total_tokens;
        total_cost += stat.cost_cents;
    }
    println!("{}", "-".repeat(60));
    println!(
        "{:<12} {:>8} {:>12} {:>12} {:>12}",
        "Total",
        total_requests,
        "",
        total_tokens,
        format_cents(total_cost),
    );
    Ok(())
}

fn handle_usage_total(
    tracker: &UsageTracker<SqliteUsageLogRepository, SqlitePricingRepository>,
    start: Option<String>,
    end: Option<String>,
    by_provider: bool,
) -> Result<()> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let start = start.as_deref().unwrap_or(&today);
    let end = end.as_deref().unwrap_or(&today);

    if by_provider {
        let stats = tracker.provider_stats(start, end)?;
        if stats.is_empty() {
            println!("No usage recorded for {} to {}.", start, end);
            return Ok(());
        }
        println!("{:<16} {:>8} {:>12} {:>12}", "Provider", "Requests", "Tokens", "Cost");
        println!("{}", "-".repeat(52));
        for stat in &stats {
            println!(
                "{:<16} {:>8} {:>12} {:>12}",
                stat.provider_name,
                stat.requests,
                stat.total_tokens,
                format_cents(stat.cost_cents),
            );
        }
    } else {
        let cost = tracker.total_cost(start, end)?;
        let tokens = tracker.total_tokens(start, end)?;
        println!("Usage from {} to {}:", start, end);
        println!("  Total tokens: {}", tokens);
        println!("  Total cost:   {}", format_cents(cost));
    }
    Ok(())
}

fn handle_usage_logs(
    tracker: &UsageTracker<SqliteUsageLogRepository, SqlitePricingRepository>,
    limit: i64,
) -> Result<()> {
    let logs = tracker.recent_logs(limit)?;
    if logs.is_empty() {
        println!("No usage logs found.");
        return Ok(());
    }

    println!("{:<6} {:<12} {:<20} {:>8} {:>8} {:>8}", "ID", "Provider", "Model", "Prompt", "Comp", "Cost");
    println!("{}", "-".repeat(70));
    for log in &logs {
        let ts = &log.timestamp[..19.min(log.timestamp.len())];
        let status = if log.success { "OK" } else { "ERR" };
        println!(
            "{:<6} {:<12} {:<20} {:>8} {:>8} {:>8} [{}]",
            log.id,
            ts,
            truncate(&log.model, 20),
            log.prompt_tokens,
            log.completion_tokens,
            format_cents(log.total_cost_cents),
            status,
        );
    }
    Ok(())
}

fn handle_usage_log(
    provider_service: &ProviderService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteSettingsRepository,
    >,
    tracker: &UsageTracker<SqliteUsageLogRepository, SqlitePricingRepository>,
    provider_name: &str,
    model: &str,
    prompt: i64,
    completion: i64,
    request_id: Option<String>,
) -> Result<()> {
    let provider = provider_service
        .get_provider(provider_name)?
        .with_context(|| format!("Provider not found: {}", provider_name))?;

    let id = tracker.record_manual(
        provider.id,
        None,
        model,
        prompt,
        completion,
        request_id.as_deref(),
    )?;

    println!("Logged usage entry id={} for {} ({}/{})", id, provider_name, prompt, completion);
    Ok(())
}

fn handle_health_check(
    service: &HealthCheckService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteHealthCheckRepository,
    >,
    provider_name: Option<String>,
) -> Result<()> {
    if let Some(name) = provider_name {
        let results = service.check_provider(&name)?;
        println!("Health check for {}:", name);
        if results.is_empty() {
            println!("  No keys found.");
        } else {
            for r in results {
                let status = if r.is_healthy { "OK" } else { "FAIL" };
                let latency = r.response_time_ms.map(|ms| format!("{}ms", ms)).unwrap_or_else(|| "-".to_string());
                let label = r.key_label.as_deref().unwrap_or("unnamed");
                println!("  {:3} | {:6} | {:8} | {} | {}", r.key_id, status, latency, label, r.error_message.as_deref().unwrap_or(""));
            }
        }
    } else {
        let all = service.check_all()?;
        if all.is_empty() {
            println!("No providers with keys to check.");
            return Ok(());
        }
        for (provider, results) in all {
            println!("{}:", provider.display_name);
            for r in results {
                let status = if r.is_healthy { "OK" } else { "FAIL" };
                let latency = r.response_time_ms.map(|ms| format!("{}ms", ms)).unwrap_or_else(|| "-".to_string());
                let label = r.key_label.as_deref().unwrap_or("unnamed");
                println!("  {:3} | {:6} | {:8} | {} | {}", r.key_id, status, latency, label, r.error_message.as_deref().unwrap_or(""));
            }
        }
    }
    Ok(())
}

fn handle_health_status(
    service: &HealthCheckService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteHealthCheckRepository,
    >,
) -> Result<()> {
    let statuses = service.latest_status()?;
    if statuses.is_empty() {
        println!("No keys stored.");
        return Ok(());
    }

    println!("{:<16} {:<6} {:<8} {:<6} {:<8} {}", "Provider", "Key", "Active", "Errors", "Status", "Last Check");
    println!("{}", "-".repeat(75));
    for s in statuses {
        let key_label = s.key_label.as_deref().unwrap_or("unnamed");
        let active = if s.is_active { "yes" } else { "no" };
        let (status, last_check) = match &s.latest_check {
            Some(hc) => {
                let st = if hc.is_healthy { "OK" } else { "FAIL" };
                let ts = &hc.timestamp[..19.min(hc.timestamp.len())];
                (st, ts.to_string())
            }
            None => ("-", "never".to_string()),
        };
        println!(
            "{:<16} {:<6} {:<8} {:<6} {:<8} {}",
            truncate(&s.provider_display, 16),
            truncate(key_label, 6),
            active,
            s.error_count,
            status,
            last_check,
        );
    }
    Ok(())
}

fn format_cents(cents: i64) -> String {
    format!("${:.2}", cents as f64 / 100.0)
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Escape a string for safe use in shell export commands
fn shell_escape(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}
