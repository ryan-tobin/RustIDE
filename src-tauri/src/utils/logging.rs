use crate::utils::{get_app_log_dir, UtilError, UtilResult};
use std::path::PathBuf;
use tracing::{Level, Subscriber};
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer, Registry,
};

/// Logging configuration
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Log level for console output
    pub console_level: Level,
    /// Log level for file output
    pub file_level: Level,
    /// Whether to include spans in logs
    pub include_spans: bool,
    /// Whether to use ANSI colors in console
    pub use_colors: bool,
    /// Log file rotation strategy
    pub rotation: LogRotation,
    /// Maximum number of log files to keep
    pub max_files: usize,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            console_level: Level::INFO,
            file_level: Level::DEBUG,
            include_spans: true,
            use_colors: true,
            rotation: LogRotation::Daily,
            max_files: 7,
        }
    }
}

/// Log file rotation strategy
#[derive(Debug, Clone)]
pub enum LogRotation {
    /// Rotate log files daily
    Daily,
    /// Rotate log files hourly
    Hourly,
    /// Rotate log files when they exceed a size limit
    Size { max_size: u64 },
    /// Never rotate log files
    Never,
}

/// Initialize the logging system
pub fn init_logging(app_handle: &tauri::AppHandle) -> UtilResult<()> {
    let config = LoggingConfig::default();
    init_logging_with_config(app_handle, config)
}

/// Initialize logging with custom configuration
pub fn init_logging_with_config(
    app_handle: &tauri::AppHandle,
    config: LoggingConfig,
) -> UtilResult<()> {
    let log_dir = get_app_log_dir(app_handle)?;
    std::fs::create_dir_all(&log_dir)?;

    // Create console layer
    let console_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_span_events(if config.include_spans {
            FmtSpan::NEW | FmtSpan::CLOSE
        } else {
            FmtSpan::NONE
        })
        .with_ansi(config.use_colors)
        .with_filter(
            EnvFilter::new("")
                .add_directive(config.console_level.into())
                .add_directive("rustide=trace".parse().unwrap()),
        );

    // Create file appender based on rotation strategy
    let file_appender = match config.rotation {
        LogRotation::Daily => rolling::daily(&log_dir, "rustide.log"),
        LogRotation::Hourly => rolling::hourly(&log_dir, "rustide.log"),
        LogRotation::Never => rolling::never(&log_dir, "rustide.log"),
        LogRotation::Size { max_size: _ } => {
            // For now, use daily rotation for size-based rotation
            // TODO: Implement proper size-based rotation
            rolling::daily(&log_dir, "rustide.log")
        }
    };

    let (non_blocking_appender, _guard) = non_blocking(file_appender);

    // Create file layer
    let file_layer = fmt::layer()
        .with_writer(non_blocking_appender)
        .with_target(true)
        .with_thread_ids(true)
        .with_span_events(if config.include_spans {
            FmtSpan::NEW | FmtSpan::CLOSE
        } else {
            FmtSpan::NONE
        })
        .with_ansi(false) // No ANSI colors in files
        .with_filter(
            EnvFilter::new("")
                .add_directive(config.file_level.into())
                .add_directive("rustide=trace".parse().unwrap()),
        );

    // Initialize the subscriber
    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();

    tracing::info!("Logging initialized successfully");
    tracing::info!("Log directory: {}", log_dir.display());

    Ok(())
}

/// Log an error with context
pub fn log_error<E: std::error::Error>(error: &E, context: &str) {
    tracing::error!(
        error = %error,
        context = context,
        "Error occurred"
    );
}

/// Log a warning with context
pub fn log_warning(message: &str, context: &str) {
    tracing::warn!(message = message, context = context, "Warning");
}

/// Get current log level
pub fn get_log_level() -> Level {
    // This is a simplified implementation
    // In a real application, you might want to track this more precisely
    Level::INFO
}

/// Set log level dynamically
pub fn set_log_level(_level: Level) {
    // TODO: Implement dynamic log level changes
    tracing::warn!("Dynamic log level changes not yet implemented");
}
