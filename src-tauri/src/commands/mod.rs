// src-tauri/src/commands/mod.rs
//! Tauri command handlers that bridge the Rust core with the React frontend
//!
//! This module provides all the Tauri commands needed for the RustIDE frontend
//! to interact with our core text editing system. Commands are organized by
//! functionality and provide a clean API for the TypeScript frontend.

use crate::core::{Editor, EditorConfig, Position, Range, SearchOptions};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub mod editor;
pub mod file_system;
pub mod search;
pub mod settings;
pub mod syntax;

/// Global state containing all open editors
pub type EditorMap = Arc<RwLock<HashMap<Uuid, Editor>>>;

/// Result type for Tauri commands
pub type CommandResult<T> = Result<T, CommandError>;

/// Error types for Tauri commands
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "type", content = "message")]
pub enum CommandError {
    #[error("Editor not found: {id}")]
    EditorNotFound { id: String },

    #[error("File error: {message}")]
    FileError { message: String },

    #[error("IO error: {message}")]
    IoError { message: String },

    #[error("Parse error: {message}")]
    ParseError { message: String },

    #[error("Invalid parameter: {parameter}")]
    InvalidParameter { parameter: String },

    #[error("Operation failed: {message}")]
    OperationFailed { message: String },

    #[error("Internal error: {message}")]
    InternalError { message: String },
}

impl From<std::io::Error> for CommandError {
    fn from(err: std::io::Error) -> Self {
        CommandError::IoError {
            message: err.to_string(),
        }
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(err: anyhow::Error) -> Self {
        CommandError::InternalError {
            message: err.to_string(),
        }
    }
}

impl From<crate::core::EditorError> for CommandError {
    fn from(err: crate::core::EditorError) -> Self {
        match err {
            crate::core::EditorError::BufferError(e) => CommandError::InternalError {
                message: e.to_string(),
            },
            crate::core::EditorError::InvalidPosition { position } => {
                CommandError::InvalidParameter {
                    parameter: format!("position: {}", position),
                }
            }
            crate::core::EditorError::InvalidRange { range } => CommandError::InvalidParameter {
                parameter: format!("range: {:?}", range),
            },
            crate::core::EditorError::NoFile => CommandError::FileError {
                message: "No file associated with editor".to_string(),
            },
            crate::core::EditorError::IoError(e) => CommandError::IoError {
                message: e.to_string(),
            },
            crate::core::EditorError::SyntaxError(msg) => {
                CommandError::OperationFailed { message: msg }
            }
            crate::core::EditorError::SearchError(msg) => {
                CommandError::OperationFailed { message: msg }
            }
        }
    }
}

/// Initialize the editor state for the Tauri application
pub fn init_editor_state() -> EditorMap {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Common response structure for successful operations
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
}

impl SuccessResponse {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }
}
