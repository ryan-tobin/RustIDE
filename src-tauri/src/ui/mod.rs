//! UI state management and coordination for RustIDE
//!
//! This module handles the application-wide UI state, panel management,
//! and coordination between the Rust backend and React frontend.
//!
//! Key components:
//! - Application state management with event-driven updates
//! - Panel/docking system for flexible IDE layout
//! - UI event handling and state synchronization
//! - Integration with core editor and project systems

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;
use uuid::Uuid;

pub mod app_state;
pub mod panels;

pub use app_state::*;
pub use panels::*;

/// Result type for UI operations
pub type UiResult<T> = Result<T, UiError>;

/// Errors that can occur in UI operations
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum UiError {
    #[error("Panel not found: {panel_id}")]
    PanelNotFound { panel_id: String },

    #[error("Invalid panel configuration: {reason}")]
    InvalidPanelConfig { reason: String },

    #[error("Layout operation failed: {operation}")]
    LayoutError { operation: String },

    #[error("State serialization error: {source}")]
    SerializationError { source: String },

    #[error("UI event processing error: {event_type}")]
    EventError { event_type: String },

    #[error("Theme error: {message}")]
    ThemeError { message: String },
}

impl From<serde_json::Error> for UiError {
    fn from(err: serde_json::Error) -> Self {
        UiError::SerializationError {
            source: err.to_string(),
        }
    }
}

/// Theme configuration for the IDE
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Theme {
    pub name: String,
    pub display_name: String,
    pub is_dark: bool,
    pub colors: ThemeColors,
    pub syntax_colors: SyntaxColors,
}

/// Core theme colors
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThemeColors {
    // Background colors
    pub background: String,
    pub surface: String,
    pub elevated: String,

    // Text colors
    pub text_primary: String,
    pub text_secondary: String,
    pub text_disabled: String,

    // Accent colors
    pub primary: String,
    pub secondary: String,
    pub accent: String,

    // Status colors
    pub success: String,
    pub warning: String,
    pub error: String,
    pub info: String,

    // Editor colors
    pub editor_background: String,
    pub editor_foreground: String,
    pub editor_selection: String,
    pub editor_cursor: String,
    pub editor_line_highlight: String,

    // UI element colors
    pub border: String,
    pub hover: String,
    pub active: String,
    pub focus: String,
}

/// Syntax highlighting colors
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyntaxColors {
    pub keyword: String,
    pub string: String,
    pub comment: String,
    pub number: String,
    pub function: String,
    pub variable: String,
    pub type_name: String,
    pub operator: String,
    pub punctuation: String,
    pub constant: String,
    pub macro_name: String,
    pub attribute: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark_theme()
    }
}

impl Theme {
    /// Create the default dark theme
    pub fn dark_theme() -> Self {
        Theme {
            name: "dark".to_string(),
            display_name: "Dark".to_string(),
            is_dark: true,
            colors: ThemeColors {
                background: "#1e1e1e".to_string(),
                surface: "#252526".to_string(),
                elevated: "#2d2d30".to_string(),
                text_primary: "#cccccc".to_string(),
                text_secondary: "#969696".to_string(),
                text_disabled: "#656565".to_string(),
                primary: "#007acc".to_string(),
                secondary: "#0e639c".to_string(),
                accent: "#ff6b6b".to_string(),
                success: "#4caf50".to_string(),
                warning: "#ff9800".to_string(),
                error: "#f44336".to_string(),
                info: "#2196f3".to_string(),
                editor_background: "#1e1e1e".to_string(),
                editor_foreground: "#d4d4d4".to_string(),
                editor_selection: "#264f78".to_string(),
                editor_cursor: "#ffffff".to_string(),
                editor_line_highlight: "#2a2d2e".to_string(),
                border: "#3e3e42".to_string(),
                hover: "#2a2d2e".to_string(),
                active: "#094771".to_string(),
                focus: "#007fd4".to_string(),
            },
            syntax_colors: SyntaxColors {
                keyword: "#569cd6".to_string(),
                string: "#ce9178".to_string(),
                comment: "#6a9955".to_string(),
                number: "#b5cea8".to_string(),
                function: "#dcdcaa".to_string(),
                variable: "#9cdcfe".to_string(),
                type_name: "#4ec9b0".to_string(),
                operator: "#d4d4d4".to_string(),
                punctuation: "#d4d4d4".to_string(),
                constant: "#4fc1ff".to_string(),
                macro_name: "#c586c0".to_string(),
                attribute: "#9cdcfe".to_string(),
            },
        }
    }

    /// Create the light theme
    pub fn light_theme() -> Self {
        Theme {
            name: "light".to_string(),
            display_name: "Light".to_string(),
            is_dark: false,
            colors: ThemeColors {
                background: "#ffffff".to_string(),
                surface: "#f3f3f3".to_string(),
                elevated: "#e8e8e8".to_string(),
                text_primary: "#333333".to_string(),
                text_secondary: "#666666".to_string(),
                text_disabled: "#999999".to_string(),
                primary: "#0078d4".to_string(),
                secondary: "#106ebe".to_string(),
                accent: "#d13438".to_string(),
                success: "#107c10".to_string(),
                warning: "#ff8c00".to_string(),
                error: "#d13438".to_string(),
                info: "#0078d4".to_string(),
                editor_background: "#ffffff".to_string(),
                editor_foreground: "#000000".to_string(),
                editor_selection: "#add6ff".to_string(),
                editor_cursor: "#000000".to_string(),
                editor_line_highlight: "#f5f5f5".to_string(),
                border: "#e1e1e1".to_string(),
                hover: "#f0f0f0".to_string(),
                active: "#c7e0f4".to_string(),
                focus: "#0078d4".to_string(),
            },
            syntax_colors: SyntaxColors {
                keyword: "#0000ff".to_string(),
                string: "#a31515".to_string(),
                comment: "#008000".to_string(),
                number: "#098658".to_string(),
                function: "#795e26".to_string(),
                variable: "#001080".to_string(),
                type_name: "#267f99".to_string(),
                operator: "#000000".to_string(),
                punctuation: "#000000".to_string(),
                constant: "#0070c1".to_string(),
                macro_name: "#af00db".to_string(),
                attribute: "#001080".to_string(),
            },
        }
    }
}

/// UI event types that can be sent to the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum UiEvent {
    /// Theme changed
    ThemeChanged {
        theme: Theme,
    },

    PanelUpdated {
        panel: PanelInfo,
    },

    /// Panel removed
    PanelRemoved {
        panel_id: String,
    },

    /// Layout changed
    LayoutChanged {
        layout: PanelLayout,
    },

    /// Application state updated
    StateUpdated {
        state: AppStateSnapshot,
    },

    /// Notification to show
    Notification {
        level: NotificationLevel,
        message: String,
        duration: Option<u64>, // milliseconds
    },

    /// Loading state changed
    LoadingChanged {
        is_loading: bool,
        message: Option<String>,
    },
}

/// Notification levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// UI preferences that can be customized
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPreferences {
    pub theme: String,
    pub font_family: String,
    pub font_size: u16,
    pub line_height: f32,
    pub show_line_numbers: bool,
    pub show_minimap: bool,
    pub word_wrap: bool,
    pub auto_save: bool,
    pub auto_save_delay: u64, // milliseconds
    pub panel_layout: Option<PanelLayout>,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            font_family: "JetBrains Mono".to_string(),
            font_size: 14,
            line_height: 1.4,
            show_line_numbers: true,
            show_minimap: true,
            word_wrap: false,
            auto_save: true,
            auto_save_delay: 1000,
            panel_layout: None,
        }
    }
}

/// Available themes in the IDE
pub fn get_available_themes() -> Vec<Theme> {
    vec![Theme::dark_theme(), Theme::light_theme()]
}

/// Get a theme by name
pub fn get_theme_by_name(name: &str) -> Option<Theme> {
    get_available_themes()
        .into_iter()
        .find(|theme| theme.name == name)
}

/// Validate theme configuration
pub fn validate_theme(theme: &Theme) -> UiResult<()> {
    if theme.name.is_empty() {
        return Err(UiError::ThemeError {
            message: "Theme name cannot be empty".to_string(),
        });
    }

    if theme.display_name.is_empty() {
        return Err(UiError::ThemeError {
            message: "Theme display name cannot be empty".to_string(),
        });
    }

    // Validate color format (should be hex colors)
    let colors = vec![
        &theme.colors.background,
        &theme.colors.surface,
        &theme.colors.text_primary,
        &theme.colors.primary,
    ];

    for color in colors {
        if !color.starts_with('#') || color.len() != 7 {
            return Err(UiError::ThemeError {
                message: format!("Invalid color format: {}", color),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_themes() {
        let dark = Theme::dark_theme();
        let light = Theme::light_theme();

        assert_eq!(dark.name, "dark");
        assert_eq!(light.name, "light");
        assert!(dark.is_dark);
        assert!(!light.is_dark);
    }

    #[test]
    fn test_theme_validation() {
        let valid_theme = Theme::dark_theme();
        assert!(validate_theme(&valid_theme).is_ok());

        let mut invalid_theme = Theme::dark_theme();
        invalid_theme.name = String::new();
        assert!(validate_theme(&invalid_theme).is_err());

        invalid_theme = Theme::dark_theme();
        invalid_theme.colors.background = "invalid".to_string();
        assert!(validate_theme(&invalid_theme).is_err());
    }

    #[test]
    fn test_get_theme_by_name() {
        assert!(get_theme_by_name("dark").is_some());
        assert!(get_theme_by_name("light").is_some());
        assert!(get_theme_by_name("nonexistent").is_none());
    }

    #[test]
    fn test_ui_preferences_default() {
        let prefs = UiPreferences::default();
        assert_eq!(prefs.theme, "dark");
        assert_eq!(prefs.font_size, 14);
        assert!(prefs.show_line_numbers);
    }
}
