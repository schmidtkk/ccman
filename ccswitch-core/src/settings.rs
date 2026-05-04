use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info};

use crate::provider::{EnvVars, CCSWITCH_ENV_KEYS};

/// Manages ~/.claude/settings.json (same as the shell script's _ccswitch_write_settings)
pub struct SettingsManager {
    path: PathBuf,
}

impl SettingsManager {
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let path = home.join(".claude").join("settings.json");

        Ok(Self { path })
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Write environment variables to ~/.claude/settings.json
    pub fn write_env_vars(&self, env_vars: &EnvVars) -> Result<()> {
        if !self.path.exists() {
            anyhow::bail!(
                "Claude Code settings file not found: {}",
                self.path.display()
            );
        }

        let content = fs::read_to_string(&self.path)?;
        let mut settings: Value = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", self.path.display()))?;

        // Get or create .env object
        let env = settings
            .as_object_mut()
            .context("settings.json root is not an object")?;

        let env_obj = env
            .entry("env")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .context("settings.json .env is not an object")?;

        // Remove all managed keys first
        for key in &CCSWITCH_ENV_KEYS {
            env_obj.remove(*key);
        }

        // Add new values
        let pairs = env_vars.to_export_pairs();
        for (key, value) in pairs {
            env_obj.insert(key, json!(value));
        }

        // Atomic write (write to temp file, then rename)
        let tmp_path = self.path.with_extension("tmp");
        let new_content = serde_json::to_string_pretty(&settings)?;
        fs::write(&tmp_path, new_content)?;
        fs::rename(&tmp_path, &self.path)?;

        debug!("Updated {}", self.path.display());
        Ok(())
    }

    /// Clear all managed environment variables (native Claude mode)
    pub fn clear_env_vars(&self) -> Result<()> {
        if !self.path.exists() {
            anyhow::bail!(
                "Claude Code settings file not found: {}",
                self.path.display()
            );
        }

        let content = fs::read_to_string(&self.path)?;
        let mut settings: Value = serde_json::from_str(&content)?;

        if let Some(env) = settings.get_mut("env").and_then(|e| e.as_object_mut()) {
            for key in &CCSWITCH_ENV_KEYS {
                env.remove(*key);
            }
        }

        // Atomic write
        let tmp_path = self.path.with_extension("tmp");
        let new_content = serde_json::to_string_pretty(&settings)?;
        fs::write(&tmp_path, new_content)?;
        fs::rename(&tmp_path, &self.path)?;

        info!("Cleared CCSwitch env vars from {}", self.path.display());
        Ok(())
    }

    /// Read current .env from settings.json (for display purposes)
    pub fn read_current_env(&self) -> Result<Option<Value>> {
        if !self.path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.path)?;
        let settings: Value = serde_json::from_str(&content)?;

        Ok(settings.get("env").cloned())
    }
}
