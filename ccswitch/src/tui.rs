use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, HighlightSpacing, Paragraph, Row, Table},
    Frame, Terminal,
};
use std::io::stdout;

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
// Tab enum
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

    const ALL: [Tab; 4] = [Tab::Providers, Tab::Keys, Tab::Usage, Tab::Health];
}

// ---------------------------------------------------------------------------
// Input modes
// ---------------------------------------------------------------------------
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
    AddProvider {
        name: String,
        display_name: String,
        base_url: String,
        model: String,
        auth_header: String,
        timeout_ms: String,
        requires_disable_traffic: bool,
        focused_field: usize,
    },
    EditProvider {
        provider_id: i64,
        name: String,
        display_name: String,
        base_url: String,
        model: String,
        auth_header: String,
        timeout_ms: String,
        requires_disable_traffic: bool,
        focused_field: usize,
    },
    ConfirmDeleteProvider {
        provider_id: i64,
        provider_name: String,
    },
}

// ---------------------------------------------------------------------------
// Keys tab pane focus
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeysPanelFocus {
    Providers,
    Keys,
}

// ---------------------------------------------------------------------------
// Click target tracking
// ---------------------------------------------------------------------------
#[derive(Default)]
struct ClickTargets {
    tab_rects: Vec<(Tab, Rect)>,
    provider_rows: Vec<(usize, Rect)>,
    key_rows: Vec<(usize, Rect)>,
    buttons: Vec<(String, Rect)>,
    popup_area: Option<Rect>,
    form_fields: Vec<(usize, Rect)>,
    keys_provider_rows: Vec<(usize, Rect)>,
    confirm_buttons: Vec<(String, Rect)>,
}

impl ClickTargets {
    fn clear(&mut self) {
        self.tab_rects.clear();
        self.provider_rows.clear();
        self.key_rows.clear();
        self.buttons.clear();
        self.popup_area = None;
        self.form_fields.clear();
        self.keys_provider_rows.clear();
        self.confirm_buttons.clear();
    }

    fn hit_button(&self, pos: Position) -> Option<&str> {
        for (name, rect) in &self.buttons {
            if rect.contains(pos) {
                return Some(name);
            }
        }
        None
    }

    fn hit_tab(&self, pos: Position) -> Option<Tab> {
        for (tab, rect) in &self.tab_rects {
            if rect.contains(pos) {
                return Some(*tab);
            }
        }
        None
    }

    fn hit_provider_row(&self, pos: Position) -> Option<usize> {
        for (idx, rect) in &self.provider_rows {
            if rect.contains(pos) {
                return Some(*idx);
            }
        }
        None
    }

    fn hit_key_row(&self, pos: Position) -> Option<usize> {
        for (idx, rect) in &self.key_rows {
            if rect.contains(pos) {
                return Some(*idx);
            }
        }
        None
    }

    fn hit_keys_provider_row(&self, pos: Position) -> Option<usize> {
        for (idx, rect) in &self.keys_provider_rows {
            if rect.contains(pos) {
                return Some(*idx);
            }
        }
        None
    }

    fn hit_form_field(&self, pos: Position) -> Option<usize> {
        for (idx, rect) in &self.form_fields {
            if rect.contains(pos) {
                return Some(*idx);
            }
        }
        None
    }

    fn hit_confirm_button(&self, pos: Position) -> Option<&str> {
        for (name, rect) in &self.confirm_buttons {
            if rect.contains(pos) {
                return Some(name);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// App State
// ---------------------------------------------------------------------------
pub struct App {
    tab: Tab,
    providers: Vec<Provider>,
    active_provider_id: Option<i64>,
    selected_provider: usize,

    keys: Vec<ApiKey>,
    selected_key: usize,
    keys_panel_focus: KeysPanelFocus,
    provider_key_counts: Vec<usize>,

    usage_logs: Vec<UsageLog>,
    usage_daily: Vec<DailyStat>,
    usage_provider: Vec<ProviderStat>,

    health_status: Vec<KeyHealthStatus>,

    popup_message: Option<String>,
    should_quit: bool,
    input_mode: InputMode,
    click_targets: ClickTargets,

    // Services
    provider_service:
        ProviderService<SqliteProviderRepository, SqliteApiKeyRepository, SqliteSettingsRepository>,
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
            keys_panel_focus: KeysPanelFocus::Keys,
            provider_key_counts: Vec::new(),
            usage_logs: Vec::new(),
            usage_daily: Vec::new(),
            usage_provider: Vec::new(),
            health_status: Vec::new(),
            popup_message: None,
            should_quit: false,
            input_mode: InputMode::Normal,
            click_targets: ClickTargets::default(),
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
                self.active_provider_id = self
                    .provider_service
                    .get_active_provider()?
                    .map(|(p, _)| p.id);
                if self.providers.is_empty() {
                    self.selected_provider = 0;
                } else if self.selected_provider >= self.providers.len() {
                    self.selected_provider = self.providers.len() - 1;
                }
            }
            Tab::Keys => {
                // Refresh provider list first
                self.providers = self.provider_service.list_providers()?;
                if self.providers.is_empty() {
                    self.selected_provider = 0;
                } else if self.selected_provider >= self.providers.len() {
                    self.selected_provider = self.providers.len() - 1;
                }
                self.active_provider_id = self
                    .provider_service
                    .get_active_provider()?
                    .map(|(p, _)| p.id);

                // Key counts for all providers (left pane)
                self.provider_key_counts = self
                    .providers
                    .iter()
                    .map(|p| {
                        self.provider_service
                            .list_keys(&p.name)
                            .map(|k| k.len())
                            .unwrap_or(0)
                    })
                    .collect();

                // Keys for selected provider (right pane)
                if let Some(provider) = self.providers.get(self.selected_provider) {
                    self.keys = self.provider_service.list_keys(&provider.name)?;
                    if self.keys.is_empty() {
                        self.selected_key = 0;
                    } else if self.selected_key >= self.keys.len() {
                        self.selected_key = self.keys.len() - 1;
                    }
                } else {
                    self.keys.clear();
                    self.selected_key = 0;
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
                self.popup_message = Some("Switched to native Claude".to_string());
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
            self.popup_message = Some(format!(
                "Health check completed for {}",
                provider.display_name
            ));
        }
        Ok(())
    }

    fn submit_add_key(
        &mut self,
        key_value: String,
        key_label: String,
        key_priority: String,
    ) -> Result<()> {
        if let Some(provider) = self.providers.get(self.selected_provider) {
            let label = if key_label.is_empty() {
                None
            } else {
                Some(key_label)
            };
            let priority = key_priority.parse::<i32>().unwrap_or(0);
            let id =
                self.provider_service
                    .add_key(&provider.name, key_value, label, Some(priority))?;
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

    #[allow(clippy::too_many_arguments)]
    fn submit_add_provider(
        &mut self,
        name: String,
        display_name: String,
        base_url: String,
        model: String,
        auth_header: String,
        timeout_ms: String,
        requires_disable_traffic: bool,
    ) -> Result<()> {
        let provider = ccswitch_db::models::Provider {
            id: 0,
            name: name.clone(),
            display_name: display_name.clone(),
            base_url,
            model: if model.is_empty() { None } else { Some(model) },
            auth_header,
            timeout_ms: timeout_ms.parse::<i64>().unwrap_or(60000),
            requires_disable_traffic,
            usage_endpoint: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let id = self.provider_service.add_provider(provider)?;
        self.popup_message = Some(format!("Added provider {} (id={})", display_name, id));
        self.refresh_tab_data()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn submit_edit_provider(
        &mut self,
        provider_id: i64,
        name: String,
        display_name: String,
        base_url: String,
        model: String,
        auth_header: String,
        timeout_ms: String,
        requires_disable_traffic: bool,
    ) -> Result<()> {
        let model_opt = if model.is_empty() { None } else { Some(model) };
        let timeout = timeout_ms.parse::<i64>().unwrap_or(60000);
        self.provider_service.update_provider(
            &name,
            Some(display_name.clone()),
            Some(base_url),
            model_opt,
            Some(auth_header),
            Some(timeout),
            Some(requires_disable_traffic),
        )?;
        self.popup_message = Some(format!("Updated provider {}", display_name));
        self.refresh_tab_data()?;
        // Keep selection on the edited provider
        if let Some(idx) = self.providers.iter().position(|p| p.id == provider_id) {
            self.selected_provider = idx;
        }
        Ok(())
    }

    fn delete_selected_provider(&mut self, provider_id: i64) -> Result<()> {
        if let Some(p) = self.providers.iter().find(|p| p.id == provider_id) {
            let name = p.name.clone();
            self.provider_service.remove_provider(&name)?;
            self.popup_message = Some(format!("Removed provider {}", name));
            self.refresh_tab_data()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------
fn handle_events(app: &mut App) -> Result<()> {
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => match &app.input_mode {
            InputMode::Normal => handle_normal_key(app, key.code)?,
            InputMode::AddKey { .. } => {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::AddKey {
                    key_value,
                    key_label,
                    key_priority,
                    focused_field,
                } = mode
                {
                    app.input_mode = handle_add_key_input(
                        app,
                        key.code,
                        key_value,
                        key_label,
                        key_priority,
                        focused_field,
                    )?;
                }
            }
            InputMode::ConfirmDelete { .. } => {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::ConfirmDelete { key_id, key_label } = mode {
                    app.input_mode = handle_confirm_delete_input(app, key.code, key_id, key_label)?;
                }
            }
            InputMode::AddProvider { .. } => {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::AddProvider {
                    name,
                    display_name,
                    base_url,
                    model,
                    auth_header,
                    timeout_ms,
                    requires_disable_traffic,
                    focused_field,
                } = mode
                {
                    app.input_mode = handle_add_provider_input(
                        app,
                        key.code,
                        name,
                        display_name,
                        base_url,
                        model,
                        auth_header,
                        timeout_ms,
                        requires_disable_traffic,
                        focused_field,
                    )?;
                }
            }
            InputMode::EditProvider { .. } => {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::EditProvider {
                    provider_id,
                    name,
                    display_name,
                    base_url,
                    model,
                    auth_header,
                    timeout_ms,
                    requires_disable_traffic,
                    focused_field,
                } = mode
                {
                    app.input_mode = handle_edit_provider_input(
                        app,
                        key.code,
                        provider_id,
                        name,
                        display_name,
                        base_url,
                        model,
                        auth_header,
                        timeout_ms,
                        requires_disable_traffic,
                        focused_field,
                    )?;
                }
            }
            InputMode::ConfirmDeleteProvider { .. } => {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::ConfirmDeleteProvider {
                    provider_id,
                    provider_name,
                } = mode
                {
                    app.input_mode = handle_confirm_delete_provider_input(
                        app,
                        key.code,
                        provider_id,
                        provider_name,
                    )?;
                }
            }
        },
        Event::Mouse(mouse) => handle_mouse_event(app, mouse),
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Keyboard handlers
// ---------------------------------------------------------------------------
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
            Tab::Providers if app.selected_provider + 1 < app.providers.len() => {
                app.selected_provider += 1;
            }
            Tab::Keys => match app.keys_panel_focus {
                KeysPanelFocus::Providers => {
                    if app.selected_provider + 1 < app.providers.len() {
                        app.selected_provider += 1;
                        let _ = app.refresh_tab_data();
                    }
                }
                KeysPanelFocus::Keys => {
                    if app.selected_key + 1 < app.keys.len() {
                        app.selected_key += 1;
                    }
                }
            },
            _ => {}
        },
        KeyCode::Up | KeyCode::Char('k') => match app.tab {
            Tab::Providers if app.selected_provider > 0 => {
                app.selected_provider -= 1;
            }
            Tab::Keys => match app.keys_panel_focus {
                KeysPanelFocus::Providers => {
                    if app.selected_provider > 0 {
                        app.selected_provider -= 1;
                        let _ = app.refresh_tab_data();
                    }
                }
                KeysPanelFocus::Keys => {
                    if app.selected_key > 0 {
                        app.selected_key -= 1;
                    }
                }
            },
            _ => {}
        },
        KeyCode::Left | KeyCode::Char('h') if app.tab == Tab::Keys => {
            app.keys_panel_focus = KeysPanelFocus::Providers;
        }
        KeyCode::Right | KeyCode::Char('l') if app.tab == Tab::Keys => {
            app.keys_panel_focus = KeysPanelFocus::Keys;
        }
        KeyCode::Enter => match app.tab {
            Tab::Providers => {
                let _ = app.switch_provider();
                let _ = app.refresh_tab_data();
            }
            Tab::Keys => match app.keys_panel_focus {
                KeysPanelFocus::Providers => {
                    app.keys_panel_focus = KeysPanelFocus::Keys;
                }
                KeysPanelFocus::Keys => {
                    // No action on Enter in keys list; use buttons instead
                }
            },
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
        KeyCode::Char('a') if app.tab == Tab::Keys => {
            app.input_mode = InputMode::AddKey {
                key_value: String::new(),
                key_label: String::new(),
                key_priority: String::new(),
                focused_field: 0,
            };
        }
        KeyCode::Char('d') if app.tab == Tab::Keys && !app.keys.is_empty() => {
            if let Some(key) = app.keys.get(app.selected_key) {
                app.input_mode = InputMode::ConfirmDelete {
                    key_id: key.id,
                    key_label: key
                        .key_label
                        .clone()
                        .unwrap_or_else(|| format!("id={}", key.id)),
                };
            }
        }
        KeyCode::Char('a') if app.tab == Tab::Providers => {
            app.input_mode = InputMode::AddProvider {
                name: String::new(),
                display_name: String::new(),
                base_url: String::new(),
                model: String::new(),
                auth_header: "Authorization: Bearer".to_string(),
                timeout_ms: "60000".to_string(),
                requires_disable_traffic: false,
                focused_field: 0,
            };
        }
        KeyCode::Char('e') if app.tab == Tab::Providers && !app.providers.is_empty() => {
            if let Some(p) = app.providers.get(app.selected_provider) {
                app.input_mode = InputMode::EditProvider {
                    provider_id: p.id,
                    name: p.name.clone(),
                    display_name: p.display_name.clone(),
                    base_url: p.base_url.clone(),
                    model: p.model.clone().unwrap_or_default(),
                    auth_header: p.auth_header.clone(),
                    timeout_ms: p.timeout_ms.to_string(),
                    requires_disable_traffic: p.requires_disable_traffic,
                    focused_field: 0,
                };
            }
        }
        KeyCode::Char('d') if app.tab == Tab::Providers && !app.providers.is_empty() => {
            if let Some(p) = app.providers.get(app.selected_provider) {
                app.input_mode = InputMode::ConfirmDeleteProvider {
                    provider_id: p.id,
                    provider_name: p.display_name.clone(),
                };
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
            Ok(InputMode::AddKey {
                key_value,
                key_label,
                key_priority,
                focused_field: next_field,
            })
        }
        KeyCode::BackTab => {
            let next_field = (focused_field + 2) % 3;
            Ok(InputMode::AddKey {
                key_value,
                key_label,
                key_priority,
                focused_field: next_field,
            })
        }
        KeyCode::Backspace => {
            match focused_field {
                0 => {
                    key_value.pop();
                }
                1 => {
                    key_label.pop();
                }
                2 => {
                    key_priority.pop();
                }
                _ => {}
            }
            Ok(InputMode::AddKey {
                key_value,
                key_label,
                key_priority,
                focused_field,
            })
        }
        KeyCode::Enter => {
            if focused_field < 2 {
                Ok(InputMode::AddKey {
                    key_value,
                    key_label,
                    key_priority,
                    focused_field: focused_field + 1,
                })
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
                2 if (c.is_ascii_digit() || c == '-') => {
                    key_priority.push(c);
                }
                _ => {}
            }
            Ok(InputMode::AddKey {
                key_value,
                key_label,
                key_priority,
                focused_field,
            })
        }
        _ => Ok(InputMode::AddKey {
            key_value,
            key_label,
            key_priority,
            focused_field,
        }),
    }
}

fn handle_confirm_delete_input(
    app: &mut App,
    code: KeyCode,
    key_id: i64,
    _key_label: String,
) -> Result<InputMode> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let _ = app.delete_selected_key(key_id);
            Ok(InputMode::Normal)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Ok(InputMode::Normal),
        _ => Ok(InputMode::ConfirmDelete {
            key_id,
            key_label: _key_label,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_add_provider_input(
    app: &mut App,
    code: KeyCode,
    mut name: String,
    mut display_name: String,
    mut base_url: String,
    mut model: String,
    mut auth_header: String,
    mut timeout_ms: String,
    mut requires_disable_traffic: bool,
    focused_field: usize,
) -> Result<InputMode> {
    const FIELD_COUNT: usize = 7;
    match code {
        KeyCode::Esc => Ok(InputMode::Normal),
        KeyCode::Tab => Ok(InputMode::AddProvider {
            name,
            display_name,
            base_url,
            model,
            auth_header,
            timeout_ms,
            requires_disable_traffic,
            focused_field: (focused_field + 1) % FIELD_COUNT,
        }),
        KeyCode::BackTab => Ok(InputMode::AddProvider {
            name,
            display_name,
            base_url,
            model,
            auth_header,
            timeout_ms,
            requires_disable_traffic,
            focused_field: (focused_field + FIELD_COUNT - 1) % FIELD_COUNT,
        }),
        KeyCode::Backspace => {
            match focused_field {
                0 => name.pop(),
                1 => display_name.pop(),
                2 => base_url.pop(),
                3 => model.pop(),
                4 => auth_header.pop(),
                5 => timeout_ms.pop(),
                6 => {
                    requires_disable_traffic = false;
                    None
                }
                _ => None,
            };
            Ok(InputMode::AddProvider {
                name,
                display_name,
                base_url,
                model,
                auth_header,
                timeout_ms,
                requires_disable_traffic,
                focused_field,
            })
        }
        KeyCode::Enter => {
            if focused_field < FIELD_COUNT - 1 {
                Ok(InputMode::AddProvider {
                    name,
                    display_name,
                    base_url,
                    model,
                    auth_header,
                    timeout_ms,
                    requires_disable_traffic,
                    focused_field: focused_field + 1,
                })
            } else if !name.is_empty() && !display_name.is_empty() && !base_url.is_empty() {
                let _ = app.submit_add_provider(
                    name,
                    display_name,
                    base_url,
                    model,
                    auth_header,
                    timeout_ms,
                    requires_disable_traffic,
                );
                Ok(InputMode::Normal)
            } else {
                app.popup_message =
                    Some("Name, Display Name, and Base URL are required".to_string());
                Ok(InputMode::Normal)
            }
        }
        KeyCode::Char(c) => {
            match focused_field {
                0 => name.push(c),
                1 => display_name.push(c),
                2 => base_url.push(c),
                3 => model.push(c),
                4 => auth_header.push(c),
                5 if c.is_ascii_digit() => {
                    timeout_ms.push(c);
                }
                5 => {}
                6 => {
                    requires_disable_traffic =
                        matches!(c.to_ascii_lowercase(), 'y' | 't' | '1' | ' ');
                }
                _ => {}
            }
            Ok(InputMode::AddProvider {
                name,
                display_name,
                base_url,
                model,
                auth_header,
                timeout_ms,
                requires_disable_traffic,
                focused_field,
            })
        }
        _ => Ok(InputMode::AddProvider {
            name,
            display_name,
            base_url,
            model,
            auth_header,
            timeout_ms,
            requires_disable_traffic,
            focused_field,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_edit_provider_input(
    app: &mut App,
    code: KeyCode,
    provider_id: i64,
    mut name: String,
    mut display_name: String,
    mut base_url: String,
    mut model: String,
    mut auth_header: String,
    mut timeout_ms: String,
    mut requires_disable_traffic: bool,
    focused_field: usize,
) -> Result<InputMode> {
    const FIELD_COUNT: usize = 7;
    match code {
        KeyCode::Esc => Ok(InputMode::Normal),
        KeyCode::Tab => Ok(InputMode::EditProvider {
            provider_id,
            name,
            display_name,
            base_url,
            model,
            auth_header,
            timeout_ms,
            requires_disable_traffic,
            focused_field: (focused_field + 1) % FIELD_COUNT,
        }),
        KeyCode::BackTab => Ok(InputMode::EditProvider {
            provider_id,
            name,
            display_name,
            base_url,
            model,
            auth_header,
            timeout_ms,
            requires_disable_traffic,
            focused_field: (focused_field + FIELD_COUNT - 1) % FIELD_COUNT,
        }),
        KeyCode::Backspace => {
            match focused_field {
                0 => name.pop(),
                1 => display_name.pop(),
                2 => base_url.pop(),
                3 => model.pop(),
                4 => auth_header.pop(),
                5 => timeout_ms.pop(),
                6 => {
                    requires_disable_traffic = false;
                    None
                }
                _ => None,
            };
            Ok(InputMode::EditProvider {
                provider_id,
                name,
                display_name,
                base_url,
                model,
                auth_header,
                timeout_ms,
                requires_disable_traffic,
                focused_field,
            })
        }
        KeyCode::Enter => {
            if focused_field < FIELD_COUNT - 1 {
                Ok(InputMode::EditProvider {
                    provider_id,
                    name,
                    display_name,
                    base_url,
                    model,
                    auth_header,
                    timeout_ms,
                    requires_disable_traffic,
                    focused_field: focused_field + 1,
                })
            } else if !name.is_empty() && !display_name.is_empty() && !base_url.is_empty() {
                let _ = app.submit_edit_provider(
                    provider_id,
                    name,
                    display_name,
                    base_url,
                    model,
                    auth_header,
                    timeout_ms,
                    requires_disable_traffic,
                );
                Ok(InputMode::Normal)
            } else {
                app.popup_message =
                    Some("Name, Display Name, and Base URL are required".to_string());
                Ok(InputMode::Normal)
            }
        }
        KeyCode::Char(c) => {
            match focused_field {
                0 => name.push(c),
                1 => display_name.push(c),
                2 => base_url.push(c),
                3 => model.push(c),
                4 => auth_header.push(c),
                5 if c.is_ascii_digit() => {
                    timeout_ms.push(c);
                }
                5 => {}
                6 => {
                    requires_disable_traffic =
                        matches!(c.to_ascii_lowercase(), 'y' | 't' | '1' | ' ');
                }
                _ => {}
            }
            Ok(InputMode::EditProvider {
                provider_id,
                name,
                display_name,
                base_url,
                model,
                auth_header,
                timeout_ms,
                requires_disable_traffic,
                focused_field,
            })
        }
        _ => Ok(InputMode::EditProvider {
            provider_id,
            name,
            display_name,
            base_url,
            model,
            auth_header,
            timeout_ms,
            requires_disable_traffic,
            focused_field,
        }),
    }
}

fn handle_confirm_delete_provider_input(
    app: &mut App,
    code: KeyCode,
    provider_id: i64,
    _provider_name: String,
) -> Result<InputMode> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let _ = app.delete_selected_provider(provider_id);
            Ok(InputMode::Normal)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Ok(InputMode::Normal),
        _ => Ok(InputMode::ConfirmDeleteProvider {
            provider_id,
            provider_name: _provider_name,
        }),
    }
}

// ---------------------------------------------------------------------------
// Mouse event handling
// ---------------------------------------------------------------------------
fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            handle_click(app, mouse.column, mouse.row);
        }
        MouseEventKind::ScrollUp => {
            handle_scroll(app, true);
        }
        MouseEventKind::ScrollDown => {
            handle_scroll(app, false);
        }
        _ => {}
    }
}

fn handle_click(app: &mut App, col: u16, row: u16) {
    let pos = Position::new(col, row);

    // Priority 1: dismiss popup on any click
    if app.popup_message.is_some() {
        app.popup_message = None;
        return;
    }

    // Priority 2: AddKey / AddProvider / EditProvider forms
    match &app.input_mode {
        InputMode::AddKey { .. } => {
            if let Some(field_idx) = app.click_targets.hit_form_field(pos) {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::AddKey {
                    key_value,
                    key_label,
                    key_priority,
                    ..
                } = mode
                {
                    app.input_mode = InputMode::AddKey {
                        key_value,
                        key_label,
                        key_priority,
                        focused_field: field_idx,
                    };
                }
                return;
            }
            if let Some(name) = app.click_targets.hit_button(pos) {
                match name {
                    "submit" => {
                        let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                        if let InputMode::AddKey {
                            key_value,
                            key_label,
                            key_priority,
                            ..
                        } = mode
                        {
                            if !key_value.is_empty() {
                                let _ = app.submit_add_key(key_value, key_label, key_priority);
                            } else {
                                app.popup_message = Some("Key value is required".to_string());
                            }
                        }
                    }
                    "cancel" => {
                        app.input_mode = InputMode::Normal;
                    }
                    _ => {}
                }
                return;
            }
            return;
        }
        InputMode::AddProvider { .. } => {
            if let Some(field_idx) = app.click_targets.hit_form_field(pos) {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::AddProvider {
                    name,
                    display_name,
                    base_url,
                    model,
                    auth_header,
                    timeout_ms,
                    requires_disable_traffic,
                    ..
                } = mode
                {
                    app.input_mode = InputMode::AddProvider {
                        name,
                        display_name,
                        base_url,
                        model,
                        auth_header,
                        timeout_ms,
                        requires_disable_traffic,
                        focused_field: field_idx,
                    };
                }
                return;
            }
            if let Some(name) = app.click_targets.hit_button(pos) {
                match name {
                    "submit" => {
                        let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                        if let InputMode::AddProvider {
                            name,
                            display_name,
                            base_url,
                            model,
                            auth_header,
                            timeout_ms,
                            requires_disable_traffic,
                            ..
                        } = mode
                        {
                            if !name.is_empty() && !display_name.is_empty() && !base_url.is_empty()
                            {
                                let _ = app.submit_add_provider(
                                    name,
                                    display_name,
                                    base_url,
                                    model,
                                    auth_header,
                                    timeout_ms,
                                    requires_disable_traffic,
                                );
                            } else {
                                app.popup_message = Some(
                                    "Name, Display Name, and Base URL are required".to_string(),
                                );
                            }
                        }
                    }
                    "cancel" => {
                        app.input_mode = InputMode::Normal;
                    }
                    _ => {}
                }
                return;
            }
            return;
        }
        InputMode::EditProvider { .. } => {
            if let Some(field_idx) = app.click_targets.hit_form_field(pos) {
                let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                if let InputMode::EditProvider {
                    provider_id,
                    name,
                    display_name,
                    base_url,
                    model,
                    auth_header,
                    timeout_ms,
                    requires_disable_traffic,
                    ..
                } = mode
                {
                    app.input_mode = InputMode::EditProvider {
                        provider_id,
                        name,
                        display_name,
                        base_url,
                        model,
                        auth_header,
                        timeout_ms,
                        requires_disable_traffic,
                        focused_field: field_idx,
                    };
                }
                return;
            }
            if let Some(name) = app.click_targets.hit_button(pos) {
                match name {
                    "submit" => {
                        let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                        if let InputMode::EditProvider {
                            provider_id,
                            name,
                            display_name,
                            base_url,
                            model,
                            auth_header,
                            timeout_ms,
                            requires_disable_traffic,
                            ..
                        } = mode
                        {
                            if !name.is_empty() && !display_name.is_empty() && !base_url.is_empty()
                            {
                                let _ = app.submit_edit_provider(
                                    provider_id,
                                    name,
                                    display_name,
                                    base_url,
                                    model,
                                    auth_header,
                                    timeout_ms,
                                    requires_disable_traffic,
                                );
                            } else {
                                app.popup_message = Some(
                                    "Name, Display Name, and Base URL are required".to_string(),
                                );
                            }
                        }
                    }
                    "cancel" => {
                        app.input_mode = InputMode::Normal;
                    }
                    _ => {}
                }
                return;
            }
            return;
        }
        _ => {}
    }

    // Priority 3: ConfirmDelete / ConfirmDeleteProvider dialog
    match &app.input_mode {
        InputMode::ConfirmDelete { .. } => {
            if let Some(name) = app.click_targets.hit_confirm_button(pos) {
                match name {
                    "yes" => {
                        let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                        if let InputMode::ConfirmDelete { key_id, .. } = mode {
                            let _ = app.delete_selected_key(key_id);
                        }
                    }
                    "no" => {
                        app.input_mode = InputMode::Normal;
                    }
                    _ => {}
                }
                return;
            }
            return;
        }
        InputMode::ConfirmDeleteProvider { .. } => {
            if let Some(name) = app.click_targets.hit_confirm_button(pos) {
                match name {
                    "yes" => {
                        let mode = std::mem::replace(&mut app.input_mode, InputMode::Normal);
                        if let InputMode::ConfirmDeleteProvider { provider_id, .. } = mode {
                            let _ = app.delete_selected_provider(provider_id);
                        }
                    }
                    "no" => {
                        app.input_mode = InputMode::Normal;
                    }
                    _ => {}
                }
                return;
            }
            return;
        }
        _ => {}
    }

    // Priority 4: Normal mode — tabs, buttons, content
    if let Some(tab) = app.click_targets.hit_tab(pos) {
        app.tab = tab;
        let _ = app.refresh_tab_data();
        return;
    }

    if let Some(name) = app.click_targets.hit_button(pos) {
        let name = name.to_string();
        handle_button_click(app, &name);
        return;
    }

    match app.tab {
        Tab::Providers => {
            if let Some(idx) = app.click_targets.hit_provider_row(pos) {
                app.selected_provider = idx;
            }
        }
        Tab::Keys => {
            // Check left pane provider rows first
            if let Some(idx) = app.click_targets.hit_keys_provider_row(pos) {
                app.selected_provider = idx;
                app.keys_panel_focus = KeysPanelFocus::Providers;
                let _ = app.refresh_tab_data();
                return;
            }
            // Check right pane key rows
            if let Some(idx) = app.click_targets.hit_key_row(pos) {
                app.selected_key = idx;
                app.keys_panel_focus = KeysPanelFocus::Keys;
            }
        }
        _ => {}
    }
}

fn handle_button_click(app: &mut App, name: &str) {
    match name {
        "add_key" if app.tab == Tab::Keys => {
            app.input_mode = InputMode::AddKey {
                key_value: String::new(),
                key_label: String::new(),
                key_priority: String::new(),
                focused_field: 0,
            };
        }
        "delete_key" if app.tab == Tab::Keys && !app.keys.is_empty() => {
            if let Some(key) = app.keys.get(app.selected_key) {
                app.input_mode = InputMode::ConfirmDelete {
                    key_id: key.id,
                    key_label: key
                        .key_label
                        .clone()
                        .unwrap_or_else(|| format!("id={}", key.id)),
                };
            }
        }
        "switch" if app.tab == Tab::Providers => {
            let _ = app.switch_provider();
            let _ = app.refresh_tab_data();
        }
        "add_provider" if app.tab == Tab::Providers => {
            app.input_mode = InputMode::AddProvider {
                name: String::new(),
                display_name: String::new(),
                base_url: String::new(),
                model: String::new(),
                auth_header: "Authorization: Bearer".to_string(),
                timeout_ms: "60000".to_string(),
                requires_disable_traffic: false,
                focused_field: 0,
            };
        }
        "edit_provider" if app.tab == Tab::Providers && !app.providers.is_empty() => {
            if let Some(p) = app.providers.get(app.selected_provider) {
                app.input_mode = InputMode::EditProvider {
                    provider_id: p.id,
                    name: p.name.clone(),
                    display_name: p.display_name.clone(),
                    base_url: p.base_url.clone(),
                    model: p.model.clone().unwrap_or_default(),
                    auth_header: p.auth_header.clone(),
                    timeout_ms: p.timeout_ms.to_string(),
                    requires_disable_traffic: p.requires_disable_traffic,
                    focused_field: 0,
                };
            }
        }
        "delete_provider" if app.tab == Tab::Providers && !app.providers.is_empty() => {
            if let Some(p) = app.providers.get(app.selected_provider) {
                app.input_mode = InputMode::ConfirmDeleteProvider {
                    provider_id: p.id,
                    provider_name: p.display_name.clone(),
                };
            }
        }
        "check" if app.tab == Tab::Health => {
            let _ = app.run_health_check();
            let _ = app.refresh_tab_data();
        }
        "refresh" => {
            let _ = app.refresh_tab_data();
        }
        "quit" => {
            app.should_quit = true;
        }
        _ => {}
    }
}

fn handle_scroll(app: &mut App, up: bool) {
    if app.input_mode != InputMode::Normal {
        return;
    }
    match app.tab {
        Tab::Providers => {
            if up && app.selected_provider > 0 {
                app.selected_provider -= 1;
            } else if !up && app.selected_provider + 1 < app.providers.len() {
                app.selected_provider += 1;
            }
        }
        Tab::Keys => match app.keys_panel_focus {
            KeysPanelFocus::Providers => {
                if up && app.selected_provider > 0 {
                    app.selected_provider -= 1;
                    let _ = app.refresh_tab_data();
                } else if !up && app.selected_provider + 1 < app.providers.len() {
                    app.selected_provider += 1;
                    let _ = app.refresh_tab_data();
                }
            }
            KeysPanelFocus::Keys => {
                if up && app.selected_key > 0 {
                    app.selected_key -= 1;
                } else if !up && app.selected_key + 1 < app.keys.len() {
                    app.selected_key += 1;
                }
            }
        },
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// UI Rendering
// ---------------------------------------------------------------------------
fn ui(frame: &mut Frame, app: &mut App) {
    app.click_targets.clear();

    let main_layout = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ],
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

    // Popups render on top — clone enum to release the immutable borrow on app
    let mode = app.input_mode.clone();
    match &mode {
        InputMode::Normal => {
            if let Some(msg) = app.popup_message.clone() {
                render_popup(frame, app, &msg);
            }
        }
        InputMode::AddKey {
            key_value,
            key_label,
            key_priority,
            focused_field,
        } => {
            render_add_key_form(
                frame,
                app,
                key_value,
                key_label,
                key_priority,
                *focused_field,
            );
        }
        InputMode::ConfirmDelete { key_id, key_label } => {
            render_confirm_delete(frame, app, *key_id, key_label);
        }
        InputMode::AddProvider {
            name,
            display_name,
            base_url,
            model,
            auth_header,
            timeout_ms,
            requires_disable_traffic,
            focused_field,
        } => {
            render_provider_form(
                frame,
                app,
                "Add Provider",
                name,
                display_name,
                base_url,
                model,
                auth_header,
                timeout_ms,
                *requires_disable_traffic,
                *focused_field,
            );
        }
        InputMode::EditProvider {
            provider_id: _,
            name,
            display_name,
            base_url,
            model,
            auth_header,
            timeout_ms,
            requires_disable_traffic,
            focused_field,
        } => {
            render_provider_form(
                frame,
                app,
                "Edit Provider",
                name,
                display_name,
                base_url,
                model,
                auth_header,
                timeout_ms,
                *requires_disable_traffic,
                *focused_field,
            );
        }
        InputMode::ConfirmDeleteProvider {
            provider_id,
            provider_name,
        } => {
            render_confirm_delete_provider(frame, app, *provider_id, provider_name);
        }
    }
}

fn render_header(frame: &mut Frame, app: &mut App, area: Rect) {
    // Draw border block
    let block = Block::default().borders(Borders::ALL).title("ccswitch");
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let mut x = inner.x;
    for tab in Tab::ALL {
        let title = tab.title();
        let is_active = tab == app.tab;
        let style = if is_active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Gray)
        };

        let w = title.len() as u16;
        let rect = Rect::new(x, inner.y, w, 1);
        frame.render_widget(Paragraph::new(title).style(style), rect);
        app.click_targets.tab_rects.push((tab, rect));

        // Divider
        x += w;
        if x < inner.x + inner.width {
            let div_rect = Rect::new(x, inner.y, 1, 1);
            frame.render_widget(
                Paragraph::new(symbols::line::VERTICAL).style(Style::default().fg(Color::DarkGray)),
                div_rect,
            );
            x += 1;
        }
    }
}

fn render_footer(frame: &mut Frame, app: &mut App, area: Rect) {
    let layout = Layout::new(
        Direction::Vertical,
        [Constraint::Length(1), Constraint::Length(1)],
    )
    .split(area);

    // Top line: keyboard hints
    let help = match &app.input_mode {
        InputMode::Normal => {
            let mut spans = vec![
                Span::styled(
                    "q",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" quit  "),
                Span::styled(
                    "Tab",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" switch  "),
                Span::styled(
                    "j/k",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" navigate  "),
                Span::styled(
                    "r",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" refresh"),
            ];
            if app.tab == Tab::Keys {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    "h/l",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" switch pane"));
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    "a/d",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" add/del"));
            } else if app.tab == Tab::Providers {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    "a/e/d",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" add/edit/del"));
            }
            Line::from(spans)
        }
        InputMode::AddKey { .. }
        | InputMode::AddProvider { .. }
        | InputMode::EditProvider { .. } => Line::from(vec![
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" next field  "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" submit  "),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" cancel  "),
            Span::raw("(or click fields & buttons)"),
        ]),
        InputMode::ConfirmDelete { .. } | InputMode::ConfirmDeleteProvider { .. } => {
            Line::from(vec![
                Span::styled(
                    "y",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" confirm  "),
                Span::styled(
                    "n/Esc",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" cancel  "),
                Span::raw("(or click buttons)"),
            ])
        }
    };
    frame.render_widget(Paragraph::new(help).alignment(Alignment::Center), layout[0]);

    // Bottom line: clickable buttons
    let mut button_constraints = vec![
        Constraint::Length(10), // Refresh
        Constraint::Length(8),  // Quit
        Constraint::Min(1),     // spacer
    ];
    let tab_button_count = match app.tab {
        Tab::Keys => 2,      // Add Key, Delete
        Tab::Providers => 4, // Switch, Add, Edit, Remove
        Tab::Health => 1,    // Check
        Tab::Usage => 0,
    };
    for _ in 0..tab_button_count {
        button_constraints.push(Constraint::Length(12));
    }

    let btn_layout = Layout::new(Direction::Horizontal, button_constraints).split(layout[1]);

    render_button(frame, app, "refresh", " Refresh ", btn_layout[0]);
    render_button(frame, app, "quit", " Quit ", btn_layout[1]);

    let mut btn_idx = 3;
    match app.tab {
        Tab::Keys => {
            render_button(frame, app, "add_key", " + Add Key ", btn_layout[btn_idx]);
            btn_idx += 1;
            render_button(frame, app, "delete_key", " Delete ", btn_layout[btn_idx]);
        }
        Tab::Providers => {
            render_button(frame, app, "switch", " Switch ", btn_layout[btn_idx]);
            btn_idx += 1;
            render_button(frame, app, "add_provider", " + Add ", btn_layout[btn_idx]);
            btn_idx += 1;
            render_button(frame, app, "edit_provider", " Edit ", btn_layout[btn_idx]);
            btn_idx += 1;
            render_button(
                frame,
                app,
                "delete_provider",
                " Remove ",
                btn_layout[btn_idx],
            );
        }
        Tab::Health => {
            render_button(frame, app, "check", " Check ", btn_layout[btn_idx]);
        }
        Tab::Usage => {}
    }
}

fn render_button(frame: &mut Frame, app: &mut App, action: &str, label: &str, area: Rect) {
    let style = Style::default()
        .bg(Color::DarkGray)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let paragraph = Paragraph::new(label)
        .style(style)
        .alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
    app.click_targets.buttons.push((action.to_string(), area));
}

fn render_providers(frame: &mut Frame, app: &mut App, area: Rect) {
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

    let table = Table::new(
        rows,
        [Constraint::Percentage(70), Constraint::Percentage(30)],
    )
    .header(Row::new(vec!["Provider", "Name"]).style(Style::default().add_modifier(Modifier::BOLD)))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Providers (click or j/k, Enter to switch)"),
    )
    .highlight_spacing(HighlightSpacing::Always);

    frame.render_widget(table, layout[0]);

    // Register provider row click targets (inside the block, after header)
    let inner_left = Rect::new(
        layout[0].x + 1,
        layout[0].y + 2, // border(1) + header(1)
        layout[0].width.saturating_sub(2),
        layout[0].height.saturating_sub(3),
    );
    for (i, _) in app.providers.iter().enumerate() {
        let row_y = inner_left.y + i as u16;
        if row_y < inner_left.bottom() {
            let rect = Rect::new(inner_left.x, row_y, inner_left.width, 1);
            app.click_targets.provider_rows.push((i, rect));
        }
    }

    // Provider details
    let details = if let Some(p) = app.providers.get(app.selected_provider) {
        let active_text = if Some(p.id) == app.active_provider_id {
            "ACTIVE"
        } else {
            ""
        };
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    &p.display_name,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(active_text, Style::default().fg(Color::Green)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(Color::Yellow)),
                Span::raw(&p.name),
            ]),
            Line::from(vec![
                Span::styled("Base URL: ", Style::default().fg(Color::Yellow)),
                Span::raw(&p.base_url),
            ]),
        ];
        if let Some(model) = &p.model {
            lines.push(Line::from(vec![
                Span::styled("Model: ", Style::default().fg(Color::Yellow)),
                Span::raw(model),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("Timeout: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}ms", p.timeout_ms)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Disable Traffic: ", Style::default().fg(Color::Yellow)),
            Span::raw(if p.requires_disable_traffic {
                "yes"
            } else {
                "no"
            }),
        ]));
        lines
    } else {
        vec![Line::raw("No provider selected")]
    };

    let detail_widget = Paragraph::new(Text::from(details))
        .block(Block::default().borders(Borders::ALL).title("Details"))
        .wrap(ratatui::widgets::Wrap { trim: true });

    frame.render_widget(detail_widget, layout[1]);
}

fn render_keys(frame: &mut Frame, app: &mut App, area: Rect) {
    let layout = Layout::new(
        Direction::Horizontal,
        [Constraint::Length(22), Constraint::Min(0)],
    )
    .split(area);

    // --- Left pane: provider list with key counts ---
    let left_border_style = if app.keys_panel_focus == KeysPanelFocus::Providers {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let left_block = Block::default()
        .borders(Borders::ALL)
        .title("Providers")
        .border_style(left_border_style);
    let inner_left = left_block.inner(layout[0]);
    frame.render_widget(left_block, layout[0]);

    let mut provider_lines: Vec<Line> = Vec::new();
    for (i, p) in app.providers.iter().enumerate() {
        let is_active = Some(p.id) == app.active_provider_id;
        let is_selected = i == app.selected_provider;

        let count = app.provider_key_counts.get(i).copied().unwrap_or(0);
        let indicator = if is_active { "*" } else { " " };
        let text = format!(
            "{}{:<12}({})",
            indicator,
            truncate_str(&p.display_name, 10),
            count
        );

        let style = if is_selected {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else if is_active {
            Style::default().fg(Color::Green)
        } else {
            Style::default()
        };

        provider_lines.push(Line::from(text).style(style));

        // Register click target
        let row_y = inner_left.y + i as u16;
        if row_y < inner_left.bottom() {
            let rect = Rect::new(inner_left.x, row_y, inner_left.width, 1);
            app.click_targets.keys_provider_rows.push((i, rect));
        }
    }
    let provider_widget = Paragraph::new(Text::from(provider_lines));
    frame.render_widget(provider_widget, inner_left);

    // --- Right pane: key table ---
    let provider_name = app
        .providers
        .get(app.selected_provider)
        .map(|p| p.display_name.as_str())
        .unwrap_or("Unknown");

    let right_border_style = if app.keys_panel_focus == KeysPanelFocus::Keys {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let right_block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Keys - {}", provider_name))
        .border_style(right_border_style);
    let inner_right = right_block.inner(layout[1]);
    frame.render_widget(right_block, layout[1]);

    if app.keys.is_empty() {
        let msg = Paragraph::new("No keys for this provider\n\nClick [+ Add Key] or press 'a'")
            .alignment(Alignment::Center);
        frame.render_widget(msg, inner_right);
        return;
    }

    let key_rows: Vec<Row> = app
        .keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let is_selected = i == app.selected_key;
            let status = if k.is_active { "active" } else { "inactive" };
            let preview = if k.key_value.len() > 8 {
                format!("****{}", &k.key_value[k.key_value.len() - 4..])
            } else {
                "****".to_string()
            };
            let style = if is_selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            };
            Row::new(vec![
                format!("{}", k.id),
                status.to_string(),
                format!("{}", k.priority),
                preview,
                k.key_label.clone().unwrap_or_default(),
            ])
            .style(style)
        })
        .collect();

    let key_table = Table::new(
        key_rows,
        [
            Constraint::Length(4),
            Constraint::Length(8),
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["ID", "Status", "Prio", "Preview", "Label"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    );

    frame.render_widget(key_table, inner_right);

    // Register key row click targets (header = 1 row)
    for (i, _) in app.keys.iter().enumerate() {
        let row_y = inner_right.y + 1 + i as u16;
        if row_y < inner_right.bottom() {
            let rect = Rect::new(inner_right.x, row_y, inner_right.width, 1);
            app.click_targets.key_rows.push((i, rect));
        }
    }
}

fn render_usage(frame: &mut Frame, app: &mut App, area: Rect) {
    let layout = Layout::new(
        Direction::Vertical,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    )
    .split(area);

    if app.usage_daily.is_empty() {
        frame.render_widget(
            Paragraph::new("No usage this month").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Monthly Usage"),
            ),
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
        .header(
            Row::new(vec!["Date", "Requests", "Tokens", "Cost"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Monthly Usage"),
        );

        frame.render_widget(table, layout[0]);
    }

    if app.usage_logs.is_empty() {
        frame.render_widget(
            Paragraph::new("No usage logs")
                .block(Block::default().borders(Borders::ALL).title("Recent Logs")),
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
            Row::new(vec![
                "ID", "Time", "Model", "Prompt", "Comp", "Cost", "Status",
            ])
            .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(Block::default().borders(Borders::ALL).title("Recent Logs"));

        frame.render_widget(table, layout[1]);
    }
}

fn render_health(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.health_status.is_empty() {
        frame.render_widget(
            Paragraph::new("No keys stored").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Health Status"),
            ),
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
        Row::new(vec![
            "Provider",
            "Key",
            "Active",
            "Errors",
            "Status",
            "Last Check",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Health Status (click [Check] to run)"),
    );

    frame.render_widget(table, area);
}

fn render_popup(frame: &mut Frame, app: &mut App, msg: &str) {
    let area = frame.area();
    let popup_area = centered_rect(50, 20, area);

    frame.render_widget(Clear, popup_area);
    let popup = Paragraph::new(msg)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title("Info (click to dismiss)"),
        )
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(popup, popup_area);
    app.click_targets.popup_area = Some(popup_area);
}

fn render_add_key_form(
    frame: &mut Frame,
    app: &mut App,
    key_value: &str,
    key_label: &str,
    key_priority: &str,
    focused_field: usize,
) {
    let area = frame.area();
    let popup_area = centered_rect(70, 50, area);

    frame.render_widget(Clear, popup_area);

    let provider_name = app
        .providers
        .get(app.selected_provider)
        .map(|p| p.display_name.as_str())
        .unwrap_or("Unknown");

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!("Add Key - {}", provider_name));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Layout: 3 field rows + spacer + button row
    let fields_layout = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
        ],
    )
    .split(inner);

    let fields = [
        ("Key Value *", key_value, 0),
        ("Label", key_label, 1),
        ("Priority", key_priority, 2),
    ];

    for (label, value, idx) in &fields {
        let is_focused = *idx == focused_field;

        let border_style = if is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", label))
            .border_style(border_style);

        let input_inner = input_block.inner(fields_layout[*idx]);
        frame.render_widget(input_block, fields_layout[*idx]);

        let text_style = if is_focused {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Gray)
        };

        let display = if value.is_empty() && is_focused {
            "_"
        } else {
            value
        };

        let paragraph = Paragraph::new(display).style(text_style);
        frame.render_widget(paragraph, input_inner);

        // Register click target for the entire field area
        app.click_targets
            .form_fields
            .push((*idx, fields_layout[*idx]));
    }

    // Submit and Cancel buttons
    let btn_layout = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(12),
            Constraint::Length(4),
            Constraint::Length(12),
        ],
    )
    .split(fields_layout[4]);

    render_button(frame, app, "submit", " Submit ", btn_layout[0]);
    render_button(frame, app, "cancel", " Cancel ", btn_layout[2]);
}

fn render_confirm_delete(frame: &mut Frame, app: &mut App, key_id: i64, key_label: &str) {
    let area = frame.area();
    let popup_area = centered_rect(50, 30, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title("Confirm Delete");

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let layout = Layout::new(
        Direction::Vertical,
        [Constraint::Length(2), Constraint::Length(3)],
    )
    .split(inner);

    let msg = Paragraph::new(format!("Delete key {} ({})?", key_id, key_label))
        .alignment(Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(msg, layout[0]);

    let btn_layout = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(10),
            Constraint::Length(4),
            Constraint::Length(10),
        ],
    )
    .split(layout[1]);

    // [Yes] button
    let yes_style = Style::default()
        .bg(Color::Red)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let yes_btn = Paragraph::new(" Yes ")
        .style(yes_style)
        .alignment(Alignment::Center);
    frame.render_widget(yes_btn, btn_layout[0]);
    app.click_targets
        .confirm_buttons
        .push(("yes".to_string(), btn_layout[0]));

    // [No] button
    let no_style = Style::default().bg(Color::DarkGray).fg(Color::White);
    let no_btn = Paragraph::new(" No ")
        .style(no_style)
        .alignment(Alignment::Center);
    frame.render_widget(no_btn, btn_layout[2]);
    app.click_targets
        .confirm_buttons
        .push(("no".to_string(), btn_layout[2]));
}

#[allow(clippy::too_many_arguments)]
fn render_provider_form(
    frame: &mut Frame,
    app: &mut App,
    title: &str,
    name: &str,
    display_name: &str,
    base_url: &str,
    model: &str,
    auth_header: &str,
    timeout_ms: &str,
    requires_disable_traffic: bool,
    focused_field: usize,
) {
    let area = frame.area();
    let popup_area = centered_rect(70, 60, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title);

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let fields_layout = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
        ],
    )
    .split(inner);

    let fields = [
        ("Name *", name, 0),
        ("Display Name *", display_name, 1),
        ("Base URL *", base_url, 2),
        ("Model", model, 3),
        ("Auth Header", auth_header, 4),
        ("Timeout (ms)", timeout_ms, 5),
        (
            "Disable Traffic",
            if requires_disable_traffic {
                "yes"
            } else {
                "no"
            },
            6,
        ),
    ];

    for (label, value, idx) in &fields {
        let is_focused = *idx == focused_field;

        let border_style = if is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", label))
            .border_style(border_style);

        let input_inner = input_block.inner(fields_layout[*idx]);
        frame.render_widget(input_block, fields_layout[*idx]);

        let text_style = if is_focused {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Gray)
        };

        let display = if value.is_empty() && is_focused {
            "_"
        } else {
            value
        };

        let paragraph = Paragraph::new(display).style(text_style);
        frame.render_widget(paragraph, input_inner);

        app.click_targets
            .form_fields
            .push((*idx, fields_layout[*idx]));
    }

    let btn_layout = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(12),
            Constraint::Length(4),
            Constraint::Length(12),
        ],
    )
    .split(fields_layout[8]);

    render_button(frame, app, "submit", " Submit ", btn_layout[0]);
    render_button(frame, app, "cancel", " Cancel ", btn_layout[2]);
}

fn render_confirm_delete_provider(
    frame: &mut Frame,
    app: &mut App,
    provider_id: i64,
    provider_name: &str,
) {
    let area = frame.area();
    let popup_area = centered_rect(50, 30, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title("Confirm Delete");

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let layout = Layout::new(
        Direction::Vertical,
        [Constraint::Length(2), Constraint::Length(3)],
    )
    .split(inner);

    let msg = Paragraph::new(format!(
        "Delete provider {} (id={})?",
        provider_name, provider_id
    ))
    .alignment(Alignment::Center)
    .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(msg, layout[0]);

    let btn_layout = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(10),
            Constraint::Length(4),
            Constraint::Length(10),
        ],
    )
    .split(layout[1]);

    let yes_style = Style::default()
        .bg(Color::Red)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let yes_btn = Paragraph::new(" Yes ")
        .style(yes_style)
        .alignment(Alignment::Center);
    frame.render_widget(yes_btn, btn_layout[0]);
    app.click_targets
        .confirm_buttons
        .push(("yes".to_string(), btn_layout[0]));

    let no_style = Style::default().bg(Color::DarkGray).fg(Color::White);
    let no_btn = Paragraph::new(" No ")
        .style(no_style)
        .alignment(Alignment::Center);
    frame.render_widget(no_btn, btn_layout[2]);
    app.click_targets
        .confirm_buttons
        .push(("no".to_string(), btn_layout[2]));
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

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_len - 1).collect::<String>())
    }
}

// ---------------------------------------------------------------------------
// Main entry
// ---------------------------------------------------------------------------
pub fn run(pool: DatabasePool) -> Result<()> {
    let mut app = App::new(pool)?;
    app.refresh_tab_data()?;

    let mut terminal = ratatui::init();
    execute!(stdout(), crossterm::event::EnableMouseCapture)?;
    terminal.clear()?;

    let result = run_app(&mut terminal, &mut app);

    execute!(stdout(), crossterm::event::DisableMouseCapture)?;
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
