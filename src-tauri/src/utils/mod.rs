//! Utility functions and helpers for RustIDE
//!
//! This module provides common utilities used throughout the application:
//! - Logging setup and configuration
//! - File system operations and path utilities
//! - Process management for external tools
//! - File watching for real-time updates
//! - Async utilities and debouncing
//! - Configuration management

use std::path::{Path, PathBuf};
use tracing::{error, warn};

pub mod async_utils;
pub mod config;
pub mod debounce;
pub mod file_watcher;
pub mod logging;
pub mod paths;
pub mod processes;

// Re-export commonly used utilities
pub use async_utils::{retry_async, timeout_future, CancellationToken};
pub use config::{AppConfig, ConfigManager};
pub use debounce::{DebounceConfig, Debouncer};
pub use file_watcher::{FileEvent, FileWatcher, WatchError};
pub use paths::{ensure_directory, get_relative_path, normalize_path, PathExt};
pub use processes::{CommandOutput, ProcessError, ProcessManager};

/// Common result type for utility functions
pub type UtilResult<T> = Result<T, UtilError>;

/// Error types for utility operations
#[derive(Debug, thiserror::Error)]
pub enum UtilError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Path error: {message}")]
    Path { message: String },

    #[error("Config error: {message}")]
    Config { message: String },

    #[error("Process error: {0}")]
    Process(#[from] ProcessError),

    #[error("Watch error: {0}")]
    Watch(#[from] WatchError),

    #[error("Timeout error: operation timed out after {duration:?}")]
    Timeout { duration: std::time::Duration },

    #[error("Cancelled: operation was cancelled")]
    Cancelled,

    #[error("Invalid argument: {argument}")]
    InvalidArgument { argument: String },

    #[error("Not found: {item}")]
    NotFound { item: String },
}

/// Initialize all utility subsystems
pub async fn init_utils(app_handle: tauri::AppHandle) -> UtilResult<()> {
    // Initialize logging first
    logging::init_logging(&app_handle)?;

    // Initialize configuration
    config::init_config(&app_handle).await?;

    tracing::info!("Utility subsystems initialized successfully");
    Ok(())
}

/// Shutdown utility subsystems gracefully
pub async fn shutdown_utils() -> UtilResult<()> {
    // Shutdown file watchers
    file_watcher::shutdown_watchers().await?;

    // Cancel any running processes
    processes::shutdown_processes().await?;

    tracing::info!("Utility subsystems shut down gracefully");
    Ok(())
}

/// Get the application data directory
pub fn get_app_data_dir(app_handle: &tauri::AppHandle) -> UtilResult<PathBuf> {
    app_handle
        .path_resolver()
        .app_data_dir()
        .ok_or_else(|| UtilError::Path {
            message: "Could not determine app data directory".to_string(),
        })
}

/// Get the application config directory
pub fn get_app_config_dir(app_handle: &tauri::AppHandle) -> UtilResult<PathBuf> {
    app_handle
        .path_resolver()
        .app_config_dir()
        .ok_or_else(|| UtilError::Path {
            message: "Could not determine app config directory".to_string(),
        })
}

/// Get the application log directory
pub fn get_app_log_dir(app_handle: &tauri::AppHandle) -> UtilResult<PathBuf> {
    let data_dir = get_app_data_dir(app_handle)?;
    Ok(data_dir.join("logs"))
}

/// Common validation functions
pub mod validation {
    use super::*;

    /// Validate that a path exists and is accessible
    pub fn validate_path_exists<P: AsRef<Path>>(path: P) -> UtilResult<()> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(UtilError::NotFound {
                item: path.display().to_string(),
            });
        }
        Ok(())
    }

    /// Validate that a path is a file
    pub fn validate_is_file<P: AsRef<Path>>(path: P) -> UtilResult<()> {
        let path = path.as_ref();
        validate_path_exists(path)?;

        if !path.is_file() {
            return Err(UtilError::InvalidArgument {
                argument: format!("Path is not a file: {}", path.display()),
            });
        }
        Ok(())
    }

    /// Validate that a path is a directory
    pub fn validate_is_directory<P: AsRef<Path>>(path: P) -> UtilResult<()> {
        let path = path.as_ref();
        validate_path_exists(path)?;

        if !path.is_dir() {
            return Err(UtilError::InvalidArgument {
                argument: format!("Path is not a directory: {}", path.display()),
            });
        }
        Ok(())
    }

    /// Validate file extension
    pub fn validate_file_extension<P: AsRef<Path>>(
        path: P,
        expected_extensions: &[&str],
    ) -> UtilResult<()> {
        let path = path.as_ref();

        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            if expected_extensions
                .iter()
                .any(|&exp| exp.eq_ignore_ascii_case(extension))
            {
                return Ok(());
            }
        }

        Err(UtilError::InvalidArgument {
            argument: format!(
                "File must have one of these extensions: {}",
                expected_extensions.join(", ")
            ),
        })
    }
}
