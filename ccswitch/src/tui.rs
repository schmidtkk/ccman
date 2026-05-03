use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Table, Tabs,
    },
    Frame, Terminal,
};
use ccswitch_core::health_check::{HealthCheckService, KeyHealthStatus};
use ccswitch_core::provider::ProviderService;
use ccswitch_core::settings::SettingsManager;
use ccswitch_core::usage_tracker::UsageTracker;
use ccswitch_db::models::{ApiKey, Provider, UsageLog};
use ccswitch_db::pool::DatabasePool;
use ccswitch_db::repositories::{
    DailyStat, ProviderStat, SqliteApiKeyRepository, SqliteHealthCheckRepository,
    SqlitePricingRepository, SqliteProviderRepository, SqliteSettingsRepository,
    SqliteUsageLogRepository,
};

// ---------------------------------------------------------------------------
// App State
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Providers,
    Keys,
    Usage,
    Health,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Providers => "Providers",
            Tab::Keys => "Keys",
            Tab::Usage => "Usage",
            Tab::Health => "Health",
        }
    }

    fn next(self) -> Self {
        match self {
            Tab::Providers => Tab::Keys,
            Tab::Keys => Tab::Usage,
            Tab::Usage => Tab::Health,
            Tab::Health => Tab::Providers,
        }
    }

    fn prev(self) -> Self {
        match self {
            Tab::Providers => Tab::Health,
            Tab::Keys => Tab::Providers,
            Tab::Usage => Tab::Keys,
            Tab::Health => Tab::Usage,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    AddKey {
        key_value: String,
        key_label: String,
        key_priority: String,
        focused_field: usize,
    },
    ConfirmDelete {
        key_id: i64,
        key_label: String,
    },
}

pub struct App {
    tab: Tab,
    providers: Vec<Provider>,
    active_provider_id: Option<i64>,
    selected_provider: usize,

    keys: Vec<ApiKey>,
    selected_key: usize,

    usage_logs: Vec<UsageLog>,
    usage_daily: Vec<DailyStat>,
    usage_provider: Vec<ProviderStat>,

    health_status: Vec<KeyHealthStatus>,

    popup_message: Option<String>,
    should_quit: bool,
    input_mode: InputMode,

    // Services
    provider_service: ProviderService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteSettingsRepository,
    >,
    settings_manager: SettingsManager,
    usage_tracker: UsageTracker<SqliteUsageLogRepository, SqlitePricingRepository>,
    health_service: HealthCheckService<
        SqliteProviderRepository,
        SqliteApiKeyRepository,
        SqliteHealthCheckRepository,
    >,
}

impl App {
    pub fn new(pool: DatabasePool) -> Result<Self> {
        let provider_repo = SqliteProviderRepository::new(pool.clone());
        let api_key_repo = SqliteApiKeyRepository::new(pool.clone());
        let settings_repo = SqliteSettingsRepository::new(pool.clone());
        let usage_repo = SqliteUsageLogRepository::new(pool.clone());
        let pricing_repo = SqlitePricingRepository::new(pool.clone());
        let health_repo = SqliteHealthCheckRepository::new(pool.clone());

        let provider_service = ProviderService::new(provider_repo, api_key_repo, settings_repo);
        let settings_manager = SettingsManager::new()?;
        let usage_tracker = UsageTracker::new(usage_repo, pricing_repo);
        let health_service = HealthCheckService::new(
            SqliteProviderRepository::new(pool.clone()),
            SqliteApiKeyRepository::new(pool.clone()),
            health_repo,
        );

        let providers = provider_service.list_providers()?;
        let active_provider_id = provider_service.get_active_provider()?.map(|(p, _)| p.id);

        let selected_provider = providers
            .iter()
            .position(|p| Some(p.id) == active_provider_id)
            .unwrap_or(0);

        Ok(Self {
            tab: Tab::Providers,
            providers,
            active_provider_id,
            selected_provider,
            keys: Vec::new(),
            selected_key: 0,
            usage_logs: Vec::new(),
            usage_daily: Vec::new(),
            usage_provider: Vec::new(),
            health_status: Vec::new(),
            popup_message: None,
            should_quit: false,
            input_mode: InputMode::Normal,
            provider_service,
            settings_manager,
            usage_tracker,
            health_service,
        })
    }

    fn refresh_tab_data(&mut self) -> Result<()> {
        match self.tab {
            Tab::Providers => {
                self.providers = self.provider_service.list_providers()?;
                self.active_provider_id = self.provider_service.get_active_provider()?.map(|(p, _)| p.id);
            }
            Tab::Keys => {
                if let Some(provider) = self.providers.get(self.selected_provider) {
                    self.keys = self.provider_service.list_keys(&provider.name)?;
                    if self.selected_key >= self.keys.len() && !self.keys.is_empty() {
                        self.selected_key = self.keys.len() - 1;
                    }
                }
            }
            Tab::Usage => {
                self.usage_logs = self.usage_tracker.recent_logs(50)?;
                self.usage_daily = self.usage_tracker.month_stats()?;
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                self.usage_provider = self.usage_tracker.provider_stats(&today, &today)?;
            }
            Tab::Health => {
                self.health_status = self.health_service.latest_status()?;
            }
        }
        Ok(())
    }

    fn switch_provider(&mut self) -> Result<()> {
        if let Some(provider) = self.providers.get(self.selected_provider) {
            let env_vars = self.provider_service.switch_to_provider(&provider.name)?;

            if provider.name == "claude" {
                self.settings_manager.clear_env_vars()?;
                self.popup_message = Some(format!("Switched to native Claude"));
            } else {
                self.settings_manager.write_env_vars(&env_vars)?;
                self.popup_message = Some(format!("Switched to {}", provider.display_name));
            }
            self.active_provider_id = Some(provider.id);
        }
        Ok(())
    }

    fn run_health_check(&mut self) -> Result<()> {
        if let Some(provider) = self.providers.get(self.selected_provider) {
            let _ = self.health_service.check_provider(&provider.name)?;
            self.popup_message = Some(format!("Health check completed for {}", provider.display_name));
        }
        Ok(())
    }

    fn submit_add_key(&mut self, key_value: String, key_label: String, key_priority: String) -> Result<()> {
        if let Some(provider) = self.providers.get(self.selected_provider) {
            let label = if key_label.is_empty() { None } else { Some(key_label) };
            let priority = key_priority.parse::<i32>().unwrap_or(0);
            let id = self.provider_service.add_key(&provider.name, key_value, label, Some(priority))?;
            self.popup_message = Some(format!("Added key id={} for {}", id, provider.display_name));
            self.refresh_tab_data()?;
        }
        Ok(())
    }

    fn delete_selected_key(&mut self, key_id: i64) -> Result<()> {
        self.provider_service.remove_key(key_id)?;
        self.popup_message = Some(format!("Removed key {}", key_id));
        self.refresh_tab_data()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------
fn handle_events(app: &mut App) -> Result<()> {
    if let Event::Key(key) = event::read()? {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        match &app.input_mode {
            InputMode::Normal => handle_normal_key(app, key.code)?,
            InputMode::AddKey { .. } => {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::AddKey { key_value, key_label, key_priority, focused_field } = mode {
                    app.input_mode = handle_add_key_input(app, key.code, key_value, key_label, key_priority, focused_field)?;
                }
            }
            InputMode::ConfirmDelete { .. } => {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::ConfirmDelete { key_id, key_label } = mode {
                    app.input_mode = handle_confirm_delete_input(app, key.code, key_id, key_label)?;
                }
            }
        }
    }
    Ok(())
}

fn handle_normal_key(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.should_quit = true;
        }
        KeyCode::Tab => {
            app.tab = app.tab.next();
            let _ = app.refresh_tab_data();
        }
        KeyCode::BackTab => {
            app.tab = app.tab.prev();
            let _ = app.refresh_tab_data();
        }
        KeyCode::Down | KeyCode::Char('j') => match app.tab {
            Tab::Providers => {
                if app.selected_provider + 1 < app.providers.len() {
                    app.selected_provider += 1;
                }
            }
            Tab::Keys => {
                if app.selected_key + 1 < app.keys.len() {
                    app.selected_key += 1;
                }
            }
            _ => {}
        },
        KeyCode::Up | KeyCode::Char('k') => match app.tab {
            Tab::Providers => {
                if app.selected_provider > 0 {
                    app.selected_provider -= 1;
                }
            }
            Tab::Keys => {
                if app.selected_key > 0 {
                    app.selected_key -= 1;
                }
            }
            _ => {}
        },
        KeyCode::Enter => match app.tab {
            Tab::Providers => {
                let _ = app.switch_provider();
                let _ = app.refresh_tab_data();
            }
            Tab::Health => {
                let _ = app.run_health_check();
                let _ = app.refresh_tab_data();
            }
            _ => {}
        },
        KeyCode::Char('r') => {
            let _ = app.refresh_tab_data();
        }
        KeyCode::Char('c') => {
            app.popup_message = None;
        }
        KeyCode::Char('a') => {
            if app.tab == Tab::Keys {
                app.input_mode = InputMode::AddKey {
                    key_value: String::new(),
                    key_label: String::new(),
                    key_priority: String::new(),
                    focused_field: 0,
                };
            }
        }
        KeyCode::Char('d') => {
            if app.tab == Tab::Keys {
                if let Some(key) = app.keys.get(app.selected_key) {
                    app.input_mode = InputMode::ConfirmDelete {
                        key_id: key.id,
                        key_label: key.key_label.clone().unwrap_or_else(|| format!("id={}", key.id)),
                    };
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_add_key_input(
    app: &mut App,
    code: KeyCode,
    mut key_value: String,
    mut key_label: String,
    mut key_priority: String,
    focused_field: usize,
) -> Result<InputMode> {
    match code {
        KeyCode::Esc => Ok(InputMode::Normal),
        KeyCode::Tab => {
            let next_field = (focused_field + 1) % 3;
            Ok(InputMode::AddKey { key_value, key_label, key_priority, focused_field: next_field })
        }
        KeyCode::BackTab => {
            let next_field = (focused_field + 2) % 3;
            Ok(InputMode::AddKey { key_value, key_label, key_priority, focused_field: next_field })
        }
        KeyCode::Backspace => {
            match focused_field {
                0 => { key_value.pop(); }
                1 => { key_label.pop(); }
                2 => { key_priority.pop(); }
                _ => {}
            }
            Ok(InputMode::AddKey { key_value, key_label, key_priority, focused_field })
        }
        KeyCode::Enter => {
            if focused_field < 2 {
                Ok(InputMode::AddKey { key_value, key_label, key_priority, focused_field: focused_field + 1 })
            } else if !key_value.is_empty() {
                let _ = app.submit_add_key(key_value, key_label, key_priority);
                Ok(InputMode::Normal)
            } else {
                app.popup_message = Some("Key value is required".to_string());
                Ok(InputMode::Normal)
            }
        }
        KeyCode::Char(c) => {
            match focused_field {
                0 => key_value.push(c),
                1 => key_label.push(c),
                2 => {
                    if c.is_ascii_digit() || c == '-' {
                        key_priority.push(c);
                    }
                }
                _ => {}
            }
            Ok(InputMode::AddKey { key_value, key_label, key_priority, focused_field })
        }
        _ => Ok(InputMode::AddKey { key_value, key_label, key_priority, focused_field }),
    }
}

fn handle_confirm_delete_input(
    app: &mut App,
    code: KeyCode,
    key_id: i64,
    key_label: String,
) -> Result<InputMode> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let _ = app.delete_selected_key(key_id);
            Ok(InputMode::Normal)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Ok(InputMode::Normal),
        _ => Ok(InputMode::ConfirmDelete { key_id, key_label }),
    }
}

// ---------------------------------------------------------------------------
// UI Rendering
// ---------------------------------------------------------------------------
fn ui(frame: &mut Frame, app: &App) {
    let main_layout = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)],
    )
    .split(frame.area());

    render_header(frame, app, main_layout[0]);

    match app.tab {
        Tab::Providers => render_providers(frame, app, main_layout[1]),
        Tab::Keys => render_keys(frame, app, main_layout[1]),
        Tab::Usage => render_usage(frame, app, main_layout[1]),
        Tab::Health => render_health(frame, app, main_layout[1]),
    }

    render_footer(frame, app, main_layout[2]);

    match &app.input_mode {
        InputMode::Normal => {
            if let Some(msg) = &app.popup_message {
                render_popup(frame, msg);
            }
        }
        InputMode::AddKey { key_value, key_label, key_priority, focused_field } => {
            render_add_key_form(frame, key_value, key_label, key_priority, *focused_field);
        }
        InputMode::ConfirmDelete { key_id, key_label } => {
            render_confirm_delete(frame, *key_id, key_label);
        }
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = [Tab::Providers, Tab::Keys, Tab::Usage, Tab::Health]
        .iter()
        .map(|t| {
            let style = if *t == app.tab {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(t.title()).style(style)
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("ccswitch"))
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED))
        .divider(symbols::line::VERTICAL);

    frame.render_widget(tabs, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let help = match &app.input_mode {
        InputMode::Normal => {
            let mut spans = vec![
                Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" quit "),
                Span::styled("Tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" switch tab "),
                Span::styled("Enter", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" select/check "),
                Span::styled("r", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" refresh "),
            ];
            if app.tab == Tab::Keys {
                spans.push(Span::styled("a", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
                spans.push(Span::raw(" add "));
                spans.push(Span::styled("d", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
                spans.push(Span::raw(" delete "));
            }
            Line::from(spans)
        }
        InputMode::AddKey { .. } => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" next field "),
            Span::styled("Enter", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" submit "),
            Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel "),
        ]),
        InputMode::ConfirmDelete { .. } => Line::from(vec![
            Span::styled("y", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" confirm "),
            Span::styled("n/Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel "),
        ]),
    };
    frame.render_widget(Paragraph::new(help).alignment(Alignment::Center), area);
}

fn render_providers(frame: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    )
    .split(area);

    // Provider list
    let rows: Vec<Row> = app
        .providers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_active = Some(p.id) == app.active_provider_id;
            let is_selected = i == app.selected_provider;

            let indicator = if is_active { " ● " } else { "   " };
            let name = format!("{}{}", indicator, p.display_name);

            let style = if is_selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else if is_active {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            Row::new(vec![name, p.name.clone()]).style(style)
        })
        .collect();

    let table = Table::new(rows, [Constraint::Percentage(70), Constraint::Percentage(30)])
        .header(Row::new(vec!["Provider", "Name"]).style(Style::default().add_modifier(Modifier::BOLD)))
        .block(Block::default().borders(Borders::ALL).title("Providers (j/k to navigate, Enter to switch)"))
        .highlight_spacing(HighlightSpacing::Always);

    frame.render_widget(table, layout[0]);

    // Provider details
    let details = if let Some(p) = app.providers.get(app.selected_provider) {
        let active_text = if Some(p.id) == app.active_provider_id {
            "ACTIVE"
        } else {
            ""
        };
        let mut lines = vec![
            Line::from(vec![
                Span::styled(&p.display_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(active_text, Style::default().fg(Color::Green)),
            ]),
            Line::raw(""),
            Line::from(vec![Span::styled("Name: ", Style::default().fg(Color::Yellow)), Span::raw(&p.name)]),
            Line::from(vec![Span::styled("Base URL: ", Style::default().fg(Color::Yellow)), Span::raw(&p.base_url)]),
        ];
        if let Some(model) = &p.model {
            lines.push(Line::from(vec![Span::styled("Model: ", Style::default().fg(Color::Yellow)), Span::raw(model)]));
        }
        lines.push(Line::from(vec![Span::styled("Timeout: ", Style::default().fg(Color::Yellow)), Span::raw(format!("{}ms", p.timeout_ms))]));
        lines.push(Line::from(vec![Span::styled("Disable Traffic: ", Style::default().fg(Color::Yellow)), Span::raw(if p.requires_disable_traffic { "yes" } else { "no" })]));
        lines
    } else {
        vec![Line::raw("No provider selected")]
    };

    let detail_widget = Paragraph::new(Text::from(details))
        .block(Block::default().borders(Borders::ALL).title("Details"))
        .wrap(ratatui::widgets::Wrap { trim: true });

    frame.render_widget(detail_widget, layout[1]);
}

fn render_keys(frame: &mut Frame, app: &App, area: Rect) {
    let provider_name = app
        .providers
        .get(app.selected_provider)
        .map(|p| p.display_name.as_str())
        .unwrap_or("Unknown");

    if app.keys.is_empty() {
        let msg = Paragraph::new(format!("No keys for {}", provider_name)).block(
            Block::default().borders(Borders::ALL).title(format!("Keys — {}", provider_name)),
        );
        frame.render_widget(msg, area);
        return;
    }

    let key_rows: Vec<Row> = app
        .keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let is_selected = i == app.selected_key;
            let status = if k.is_active { "active" } else { "inactive" };
            let preview = &k.key_value[..20.min(k.key_value.len())];
            let style = if is_selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            };
            Row::new(vec![
                format!("{}", k.id),
                status.to_string(),
                format!("{}", k.priority),
                format!("{}", k.error_count),
                format!("{}...", preview),
                k.key_label.clone().unwrap_or_default(),
            ])
            .style(style)
        })
        .collect();

    let key_table = Table::new(
        key_rows,
        [
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Length(4),
            Constraint::Length(6),
            Constraint::Length(22),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["ID", "Status", "Prio", "Errs", "Preview", "Label"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title(format!("Keys — {} (j/k a/d)", provider_name)));

    frame.render_widget(key_table, area);
}

fn render_usage(frame: &mut Frame, app: &App, area: Rect) {
    let layout = Layout::new(
        Direction::Vertical,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    )
    .split(area);

    // Top: daily stats
    if app.usage_daily.is_empty() {
        frame.render_widget(
            Paragraph::new("No usage this month").block(Block::default().borders(Borders::ALL).title("Monthly Usage")),
            layout[0],
        );
    } else {
        let rows: Vec<Row> = app
            .usage_daily
            .iter()
            .map(|s| {
                Row::new(vec![
                    s.day.clone(),
                    format!("{}", s.requests),
                    format!("{}", s.total_tokens),
                    format!("${:.2}", s.cost_cents as f64 / 100.0),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(14),
                Constraint::Length(10),
            ],
        )
        .header(Row::new(vec!["Date", "Requests", "Tokens", "Cost"]).style(Style::default().add_modifier(Modifier::BOLD)))
        .block(Block::default().borders(Borders::ALL).title("Monthly Usage"));

        frame.render_widget(table, layout[0]);
    }

    // Bottom: recent logs
    if app.usage_logs.is_empty() {
        frame.render_widget(
            Paragraph::new("No usage logs").block(Block::default().borders(Borders::ALL).title("Recent Logs")),
            layout[1],
        );
    } else {
        let rows: Vec<Row> = app
            .usage_logs
            .iter()
            .take(20)
            .map(|log| {
                let ts = &log.timestamp[..19.min(log.timestamp.len())];
                let status = if log.success { "OK" } else { "ERR" };
                Row::new(vec![
                    format!("{}", log.id),
                    ts.to_string(),
                    log.model.clone(),
                    format!("{}", log.prompt_tokens),
                    format!("{}", log.completion_tokens),
                    format!("${:.2}", log.total_cost_cents as f64 / 100.0),
                    status.to_string(),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(5),
                Constraint::Length(20),
                Constraint::Length(18),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(5),
            ],
        )
        .header(
            Row::new(vec!["ID", "Time", "Model", "Prompt", "Comp", "Cost", "Status"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(Block::default().borders(Borders::ALL).title("Recent Logs"));

        frame.render_widget(table, layout[1]);
    }
}

fn render_health(frame: &mut Frame, app: &App, area: Rect) {
    if app.health_status.is_empty() {
        frame.render_widget(
            Paragraph::new("No keys stored").block(Block::default().borders(Borders::ALL).title("Health Status")),
            area,
        );
        return;
    }

    let rows: Vec<Row> = app
        .health_status
        .iter()
        .map(|s| {
            let status = match &s.latest_check {
                Some(hc) if hc.is_healthy => Span::styled("OK", Style::default().fg(Color::Green)),
                Some(_) => Span::styled("FAIL", Style::default().fg(Color::Red)),
                None => Span::raw("-"),
            };
            let last_check = match &s.latest_check {
                Some(hc) => hc.timestamp[..19.min(hc.timestamp.len())].to_string(),
                None => "never".to_string(),
            };
            let active = if s.is_active {
                Span::styled("yes", Style::default().fg(Color::Green))
            } else {
                Span::styled("no", Style::default().fg(Color::Red))
            };

            Row::new(vec![
                Cell::from(s.provider_display.clone()),
                Cell::from(s.key_label.clone().unwrap_or_default()),
                Cell::from(active),
                Cell::from(format!("{}", s.error_count)),
                Cell::from(status),
                Cell::from(last_check),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(16),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(20),
        ],
    )
    .header(
        Row::new(vec!["Provider", "Key", "Active", "Errors", "Status", "Last Check"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL).title("Health Status (Enter to check selected provider)"));

    frame.render_widget(table, area);
}

fn render_popup(frame: &mut Frame, msg: &str) {
    let area = frame.area();
    let popup_area = centered_rect(50, 20, area);

    frame.render_widget(Clear, popup_area);
    let popup = Paragraph::new(msg)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title("Info"),
        )
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(popup, popup_area);
}

fn render_add_key_form(
    frame: &mut Frame,
    key_value: &str,
    key_label: &str,
    key_priority: &str,
    focused_field: usize,
) {
    let area = frame.area();
    let popup_area = centered_rect(60, 40, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title("Add Key");

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let fields = [
        ("Key Value *", key_value, focused_field == 0),
        ("Label", key_label, focused_field == 1),
        ("Priority", key_priority, focused_field == 2),
    ];

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (label, value, is_focused) in &fields {
        let label_style = if *is_focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let input_style = if *is_focused {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{label:12} "), label_style),
            Span::styled(if value.is_empty() { " ".to_string() } else { value.to_string() }, input_style),
        ]));
        lines.push(Line::raw(""));
    }

    let paragraph = Paragraph::new(Text::from(lines));
    frame.render_widget(paragraph, inner);
}

fn render_confirm_delete(frame: &mut Frame, key_id: i64, key_label: &str) {
    let area = frame.area();
    let popup_area = centered_rect(50, 20, area);

    frame.render_widget(Clear, popup_area);

    let msg = format!("Delete key {} ({})?", key_id, key_label);
    let popup = Paragraph::new(msg)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .title("Confirm Delete"),
        )
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(popup, popup_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::new(
        Direction::Vertical,
        [
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ],
    )
    .split(r);

    Layout::new(
        Direction::Horizontal,
        [
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ],
    )
    .split(popup_layout[1])[1]
}

// ---------------------------------------------------------------------------
// Main entry
// ---------------------------------------------------------------------------
pub fn run(pool: DatabasePool) -> Result<()> {
    let mut app = App::new(pool)?;
    app.refresh_tab_data()?;

    let mut terminal = ratatui::init();
    terminal.clear()?;

    let result = run_app(&mut terminal, &mut app);

    ratatui::restore();
    result
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui(frame, app))?;
        handle_events(app)?;
    }
    Ok(())
}
