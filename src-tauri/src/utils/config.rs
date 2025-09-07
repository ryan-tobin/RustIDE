use crate::utils::{get_app_config_dir, UtilError, UtilResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, instrument};

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Editor configuration
    pub editor: EditorSettings,
    /// UI configuration
    pub ui: UiSettings,
    /// Language server configuration
    pub lsp: LspSettings,
    /// File watcher configuration
    pub file_watcher: FileWatcherSettings,
    /// Performance settings
    pub performance: PerformanceSettings,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            editor: EditorSettings::default(),
            ui: UiSettings::default(),
            lsp: LspSettings::default(),
            file_watcher: FileWatcherSettings::default(),
            performance: PerformanceSettings::default(),
        }
    }
}

/// Editor-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {
    pub tab_size: usize,
    pub use_tabs: bool,
    pub auto_save: bool,
    pub auto_save_delay: u64, // milliseconds
    pub word_wrap: bool,
    pub show_line_numbers: bool,
    pub show_minimap: bool,
    pub font_family: String,
    pub font_size: f32,
    pub theme: String,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            tab_size: 4,
            use_tabs: false,
            auto_save: true,
            auto_save_delay: 2000,
            word_wrap: false,
            show_line_numbers: true,
            show_minimap: true,
            font_family: "JetBrains Mono".to_string(),
            font_size: 14.0,
            theme: "dark".to_string(),
        }
    }
}

/// UI-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSettings {
    pub sidebar_width: f64,
    pub panel_height: f64,
    pub show_sidebar: bool,
    pub show_status_bar: bool,
    pub show_menu_bar: bool,
    pub compact_mode: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            sidebar_width: 250.0,
            panel_height: 200.0,
            show_sidebar: true,
            show_status_bar: true,
            show_menu_bar: true,
            compact_mode: false,
        }
    }
}

/// Language server settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspSettings {
    pub enabled: bool,
    pub rust_analyzer_path: Option<String>,
    pub completion_enabled: bool,
    pub diagnostics_enabled: bool,
    pub hover_enabled: bool,
    pub signature_help_enabled: bool,
    pub max_completion_items: usize,
}

impl Default for LspSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            rust_analyzer_path: None, // Auto-detect
            completion_enabled: true,
            diagnostics_enabled: true,
            hover_enabled: true,
            signature_help_enabled: true,
            max_completion_items: 50,
        }
    }
}

/// File watcher settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatcherSettings {
    pub enabled: bool,
    pub debounce_delay: u64, // milliseconds
    pub watch_cargo_toml: bool,
    pub watch_gitignore: bool,
    pub ignore_patterns: Vec<String>,
}

impl Default for FileWatcherSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_delay: 500,
            watch_cargo_toml: true,
            watch_gitignore: true,
            ignore_patterns: vec![
                ".git".to_string(),
                "target".to_string(),
                "node_modules".to_string(),
                ".DS_Store".to_string(),
            ],
        }
    }
}

/// Performance-related settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSettings {
    pub syntax_cache_size: usize,
    pub max_file_size_mb: u64,
    pub background_processing: bool,
    pub lazy_loading: bool,
    pub memory_limit_mb: u64,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            syntax_cache_size: 100,
            max_file_size_mb: 50,
            background_processing: true,
            lazy_loading: true,
            memory_limit_mb: 1024,
        }
    }
}

/// Configuration manager
pub struct ConfigManager {
    config: Arc<RwLock<AppConfig>>,
    config_path: PathBuf,
}

impl ConfigManager {
    /// Create a new configuration manager
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config: Arc::new(RwLock::new(AppConfig::default())),
            config_path,
        }
    }

    /// Load configuration from file
    #[instrument(skip(self))]
    pub async fn load(&self) -> UtilResult<()> {
        if self.config_path.exists() {
            let content = tokio::fs::read_to_string(&self.config_path).await?;
            let config: AppConfig = toml::from_str(&content).map_err(|e| UtilError::Config {
                message: format!("Failed to parse config: {}", e),
            })?;

            *self.config.write().await = config;
            info!("Configuration loaded from: {}", self.config_path.display());
        } else {
            self.save().await?;
            info!(
                "Created default configuration at: {}",
                self.config_path.display()
            );
        }

        Ok(())
    }

    /// Save configuration to file
    #[instrument(skip(self))]
    pub async fn save(&self) -> UtilResult<()> {
        // Ensure config directory exists
        if let Some(parent) = self.config_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let config = self.config.read().await;
        let content = toml::to_string_pretty(&*config).map_err(|e| UtilError::Config {
            message: format!("Failed to serialize config: {}", e),
        })?;

        tokio::fs::write(&self.config_path, content).await?;
        debug!("Configuration saved to: {}", self.config_path.display());

        Ok(())
    }

    /// Get a copy of the current configuration
    pub async fn get(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    /// Update configuration
    pub async fn update<F>(&self, updater: F) -> UtilResult<()>
    where
        F: FnOnce(&mut AppConfig),
    {
        let mut config = self.config.write().await;
        updater(&mut config);
        drop(config);

        self.save().await?;
        Ok(())
    }

    /// Get editor settings
    pub async fn get_editor_settings(&self) -> EditorSettings {
        self.config.read().await.editor.clone()
    }

    /// Update editor settings
    pub async fn update_editor_settings<F>(&self, updater: F) -> UtilResult<()>
    where
        F: FnOnce(&mut EditorSettings),
    {
        self.update(|config| updater(&mut config.editor)).await
    }

    /// Get UI settings
    pub async fn get_ui_settings(&self) -> UiSettings {
        self.config.read().await.ui.clone()
    }

    /// Update UI settings
    pub async fn update_ui_settings<F>(&self, updater: F) -> UtilResult<()>
    where
        F: FnOnce(&mut UiSettings),
    {
        self.update(|config| updater(&mut config.ui)).await
    }

    /// Reset to default configuration
    pub async fn reset_to_default(&self) -> UtilResult<()> {
        *self.config.write().await = AppConfig::default();
        self.save().await?;
        info!("Configuration reset to default");
        Ok(())
    }
}

// Global configuration manager instance
static CONFIG_MANAGER: once_cell::sync::OnceCell<ConfigManager> = once_cell::sync::OnceCell::new();

/// Initialize global configuration
pub async fn init_config(app_handle: &tauri::AppHandle) -> UtilResult<()> {
    let config_dir = get_app_config_dir(app_handle)?;
    let config_path = config_dir.join("config.toml");

    let manager = ConfigManager::new(config_path);
    manager.load().await?;

    CONFIG_MANAGER.set(manager).map_err(|_| UtilError::Config {
        message: "Configuration already initialized".to_string(),
    })?;

    Ok(())
}

/// Get global configuration manager
pub fn get_config_manager() -> UtilResult<&'static ConfigManager> {
    CONFIG_MANAGER.get().ok_or_else(|| UtilError::Config {
        message: "Configuration not initialized".to_string(),
    })
}

/// Get current configuration
pub async fn get_config() -> UtilResult<AppConfig> {
    let manager = get_config_manager()?;
    Ok(manager.get().await)
}

/// Update configuration
pub async fn update_config<F>(updater: F) -> UtilResult<()>
where
    F: FnOnce(&mut AppConfig),
{
    let manager = get_config_manager()?;
    manager.update(updater).await
}
