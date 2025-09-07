// src-tauri/src/core/mod.rs
//! Core text editing system for RustIDE
//!
//! This module provides the foundational text editing capabilities including:
//! - Text buffer management with rope data structure
//! - Multi-cursor support and selection handling
//! - Syntax highlighting with Tree-sitter
//! - Editor orchestration and event handling

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

pub mod cursor;
pub mod editor;
pub mod syntax;
pub mod text_buffer;
pub mod traits;
pub mod utils;

// Re-export commonly used types
pub use cursor::{Cursor, CursorManager, Direction, MovementUnit, SelectionMode};
pub use editor::{
    Editor, EditorConfig, EditorEvent, EditorMetrics, EditorState, SearchOptions, SearchResult,
    ViewState,
};
pub use syntax::{
    SyntaxHighlighter, SyntaxTheme, ThemedSyntaxHighlighter, ThemedToken, Token, TokenType,
};
pub use text_buffer::{
    BufferChangeEvent, BufferConfig, LineEnding, Position, Range, TextBuffer, TextEdit,
};
pub use traits::EditorEventListener;

/// Errors that can occur in the core editing system
#[derive(Debug, Error)]
pub enum EditorError {
    /// Buffer-related errors
    #[error("Buffer error: {0}")]
    BufferError(#[from] anyhow::Error),

    /// Invalid position in the buffer
    #[error("Invalid position: {position}")]
    InvalidPosition { position: Position },

    /// Invalid range in the buffer
    #[error("Invalid range: {range:?}")]
    InvalidRange { range: Range },

    /// No file associated with the editor
    #[error("No file associated with editor")]
    NoFile,

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Syntax highlighting error
    #[error("Syntax error: {0}")]
    SyntaxError(String),

    /// Search operation error
    #[error("Search error: {0}")]
    SearchError(String),
}

pub type EditorResult<T> = Result<T, EditorError>;

/// Initialize the core editing system
pub fn initialize() -> Result<()> {
    // Initialize logging if not already done
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "debug");
    }

    // Initialize any global state needed by the core system
    tracing::debug!("Core editing system initialized");
    Ok(())
}

/// Core configuration for the editing system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// Maximum number of editors to keep in memory
    pub max_editors: usize,
    /// Default editor configuration
    pub default_editor_config: EditorConfig,
    /// Default buffer configuration
    pub default_buffer_config: BufferConfig,
    /// Syntax highlighting settings
    pub syntax_config: SyntaxConfig,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            max_editors: 100,
            default_editor_config: EditorConfig::default(),
            default_buffer_config: BufferConfig::default(),
            syntax_config: SyntaxConfig::default(),
        }
    }
}

/// Configuration for syntax highlighting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxConfig {
    /// Whether syntax highlighting is enabled
    pub enabled: bool,
    /// Default theme to use
    pub default_theme: String,
    /// Cache size for syntax trees
    pub cache_size: usize,
    /// Maximum file size for syntax highlighting (in bytes)
    pub max_file_size: usize,
}

impl Default for SyntaxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_theme: "dark".to_string(),
            cache_size: 100,
            max_file_size: 1024 * 1024, // 1MB
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_config_defaults() {
        let config = CoreConfig::default();
        assert_eq!(config.max_editors, 100);
        assert!(config.syntax_config.enabled);
    }

    #[test]
    fn test_editor_error_display() {
        let pos = Position::new(10, 20);
        let err = EditorError::InvalidPosition { position: pos };
        assert!(err.to_string().contains("Invalid position"));
        assert!(err.to_string().contains("10:21")); // 1-indexed display
    }

    #[test]
    fn test_initialization() {
        assert!(initialize().is_ok());
    }
}
