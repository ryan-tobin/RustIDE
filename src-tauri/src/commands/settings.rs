use super::{CommandError, CommandResult, SuccessResponse};
use crate::core::EditorConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{command, AppHandle, Manager};
use tracing::{debug, instrument};

/// Application settings
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub editor: EditorConfig,
    pub recent_files: Vec<String>,
    pub window_state: WindowState,
    pub theme: String,
    pub auto_save: bool,
    pub auto_save_delay: u64, // milliseconds
}

/// Window state for persistence
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    pub maximized: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            editor: EditorConfig::default(),
            recent_files: Vec::new(),
            window_state: WindowState {
                width: 1200.0,
                height: 800.0,
                x: 100.0,
                y: 100.0,
                maximized: false,
            },
            theme: "dark".to_string(),
            auto_save: true,
            auto_save_delay: 2000,
        }
    }
}

/// Get application settings
#[command]
#[instrument]
pub async fn get_settings(app: AppHandle) -> CommandResult<AppSettings> {
    let config_dir =
        app.path_resolver()
            .app_config_dir()
            .ok_or_else(|| CommandError::OperationFailed {
                message: "Could not determine config directory".to_string(),
            })?;

    let settings_path = config_dir.join("settings.json");

    if settings_path.exists() {
        let content = tokio::fs::read_to_string(&settings_path).await?;
        let settings: AppSettings =
            serde_json::from_str(&content).map_err(|e| CommandError::ParseError {
                message: format!("Failed to parse settings: {}", e),
            })?;
        Ok(settings)
    } else {
        Ok(AppSettings::default())
    }
}

/// Save application settings
#[command]
#[instrument(skip(app, settings))]
pub async fn save_settings(
    app: AppHandle,
    settings: AppSettings,
) -> CommandResult<SuccessResponse> {
    let config_dir =
        app.path_resolver()
            .app_config_dir()
            .ok_or_else(|| CommandError::OperationFailed {
                message: "Could not determine config directory".to_string(),
            })?;

    // Ensure config directory exists
    tokio::fs::create_dir_all(&config_dir).await?;

    let settings_path = config_dir.join("settings.json");
    let content =
        serde_json::to_string_pretty(&settings).map_err(|e| CommandError::ParseError {
            message: format!("Failed to serialize settings: {}", e),
        })?;

    tokio::fs::write(&settings_path, content).await?;

    debug!("Saved settings to: {}", settings_path.display());
    Ok(SuccessResponse::new("Settings saved successfully"))
}

/// Add file to recent files list
#[command]
#[instrument(skip(app))]
pub async fn add_recent_file(app: AppHandle, file_path: String) -> CommandResult<SuccessResponse> {
    let mut settings = get_settings(app.clone()).await?;

    // Remove if already exists
    settings.recent_files.retain(|path| path != &file_path);

    // Add to beginning
    settings.recent_files.insert(0, file_path);

    // Keep only last 10 files
    settings.recent_files.truncate(10);

    save_settings(app, settings).await?;

    Ok(SuccessResponse::new("Added to recent files"))
}

/// Get recent files list
#[command]
#[instrument(skip(app))]
pub async fn get_recent_files(app: AppHandle) -> CommandResult<Vec<String>> {
    let settings = get_settings(app).await?;
    Ok(settings.recent_files)
}

/// Reset settings to default
#[command]
#[instrument(skip(app))]
pub async fn reset_settings(app: AppHandle) -> CommandResult<SuccessResponse> {
    let default_settings = AppSettings::default();
    save_settings(app, default_settings).await?;

    debug!("Reset settings to default");
    Ok(SuccessResponse::new("Settings reset to default"))
}

/// Export settings to file
#[command]
#[instrument(skip(app))]
pub async fn export_settings(
    app: AppHandle,
    export_path: String,
) -> CommandResult<SuccessResponse> {
    let settings = get_settings(app).await?;
    let content =
        serde_json::to_string_pretty(&settings).map_err(|e| CommandError::ParseError {
            message: format!("Failed to serialize settings: {}", e),
        })?;

    let path = PathBuf::from(export_path);
    tokio::fs::write(&path, content).await?;

    debug!("Exported settings to: {}", path.display());
    Ok(SuccessResponse::new("Settings exported successfully"))
}

/// Import settings from file
#[command]
#[instrument(skip(app))]
pub async fn import_settings(
    app: AppHandle,
    import_path: String,
) -> CommandResult<SuccessResponse> {
    let path = PathBuf::from(import_path);

    if !path.exists() {
        return Err(CommandError::FileError {
            message: "Settings file does not exist".to_string(),
        });
    }

    let content = tokio::fs::read_to_string(&path).await?;
    let settings: AppSettings =
        serde_json::from_str(&content).map_err(|e| CommandError::ParseError {
            message: format!("Failed to parse settings file: {}", e),
        })?;

    save_settings(app, settings).await?;

    debug!("Imported settings from: {}", path.display());
    Ok(SuccessResponse::new("Settings imported successfully"))
}
