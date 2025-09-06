use super::{CommandError, CommandResult, EditorMap};
use crate::core::{Position, Range, SyntaxPerformanceStats, SyntaxTheme, ThemedToken};
use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, instrument};
use uuid::Uuid;

/// Request to get syntax tokens for a range
#[derive(Debug, Deserialize)]
pub struct GetTokensRequest {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Syntax highlighting response
#[derive(Debug, Serialize)]
pub struct SyntaxTokens {
    pub tokens: Vec<ThemedToken>,
    pub version: u64,
}

/// Set syntax highlighting language
#[command]
#[instrument(skip(editors))]
pub async fn set_syntax_language(
    editors: State<'_, EditorMap>,
    editor_id: String,
    language: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor
        .syntax_highlighter_mut()
        .highlighter_mut()
        .set_language(&language)
        .map_err(|e| CommandError::OperationFailed {
            message: format!("Failed to set language: {}", e),
        })?;

    debug!("Set syntax language for editor {}: {}", id, language);
    Ok(SuccessResponse::new("Language set successfully"))
}

/// Get all syntax tokens for the editor
#[command]
#[instrument(skip(editors))]
pub async fn get_syntax_tokens(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SyntaxTokens> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let tokens = editor
        .get_all_tokens()
        .map_err(|e| CommandError::OperationFailed {
            message: format!("Failed to get tokens: {}", e),
        })?;

    let version = editor.buffer().version();

    debug!("Got {} syntax tokens for editor {}", tokens.len(), id);
    Ok(SyntaxTokens { tokens, version })
}

/// Get syntax tokens for a specific range
#[command]
#[instrument(skip(editors, request))]
pub async fn get_syntax_tokens_range(
    editors: State<'_, EditorMap>,
    editor_id: String,
    request: GetTokensRequest,
) -> CommandResult<SyntaxTokens> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let range = Range::new(
        Position::new(request.start_line, request.start_column),
        Position::new(request.end_line, request.end_column),
    );

    let tokens = editor
        .get_visible_tokens()
        .map_err(|e| CommandError::OperationFailed {
            message: format!("Failed to get tokens: {}", e),
        })?;

    let version = editor.buffer().version();

    debug!(
        "Got {} syntax tokens for range in editor {}",
        tokens.len(),
        id
    );
    Ok(SyntaxTokens { tokens, version })
}

/// Set syntax highlighting theme
#[command]
#[instrument(skip(editors, theme))]
pub async fn set_syntax_theme(
    editors: State<'_, EditorMap>,
    editor_id: String,
    theme: SyntaxTheme,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.set_theme(theme);

    debug!("Set syntax theme for editor {}", id);
    Ok(SuccessResponse::new("Theme set successfully"))
}

/// Get current syntax theme
#[command]
#[instrument(skip(editors))]
pub async fn get_syntax_theme(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SyntaxTheme> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let theme = editor.syntax_highlighter().theme().clone();

    debug!("Got syntax theme for editor {}", id);
    Ok(theme)
}

/// Get syntax highlighting performance stats
#[command]
#[instrument(skip(editors))]
pub async fn get_syntax_performance(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SyntaxPerformanceStats> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let stats = editor
        .syntax_highlighter()
        .highlighter()
        .performance_stats();

    debug!("Got syntax performance stats for editor {}", id);
    Ok(stats)
}

/// Get available syntax languages
#[command]
pub async fn get_available_languages() -> CommandResult<Vec<String>> {
    let configs = crate::core::syntax::get_language_configs();
    let languages: Vec<String> = configs.keys().map(|&s| s.to_string()).collect();

    debug!("Available syntax languages: {:?}", languages);
    Ok(languages)
}
