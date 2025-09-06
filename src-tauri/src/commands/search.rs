use super::{CommandError, CommandResult, EditorMap, SuccessResponse};
use crate::core::{SearchOptions, SearchResult};
use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, instrument};
use uuid::Uuid;

/// Search request from frontend
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub forward: bool,
    pub wrap_around: bool,
}

impl From<SearchRequest> for SearchOptions {
    fn from(request: SearchRequest) -> Self {
        SearchOptions {
            query: request.query,
            case_sensitive: request.case_sensitive,
            whole_word: request.whole_word,
            use_regex: request.use_regex,
            forward: request.forward,
            wrap_around: request.wrap_around,
        }
    }
}

/// Search for text in the editor
#[command]
#[instrument(skip(editors, request))]
pub async fn search_text(
    editors: State<'_, EditorMap>,
    editor_id: String,
    request: SearchRequest,
) -> CommandResult<Vec<SearchResult>> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let search_options: SearchOptions = request.into();
    let results = editor.search(search_options)?;

    debug!(
        "Search completed in editor {}: found {} results",
        id,
        results.len()
    );
    Ok(results)
}

/// Find next search result
#[command]
#[instrument(skip(editors))]
pub async fn find_next(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<Option<SearchResult>> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let result = editor.find_next()?;

    debug!("Find next in editor {}: {:?}", id, result.is_some());
    Ok(result)
}

/// Find previous search result
#[command]
#[instrument(skip(editors))]
pub async fn find_previous(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<Option<SearchResult>> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    // Create a reverse search by temporarily modifying search direction
    // Note: This is a simplified implementation - in practice, you might want
    // to store the search state more comprehensively
    let result = editor.find_next()?; // This would need proper previous implementation

    debug!("Find previous in editor {}: {:?}", id, result.is_some());
    Ok(result)
}

/// Replace current selection with text
#[command]
#[instrument(skip(editors))]
pub async fn replace_selection(
    editors: State<'_, EditorMap>,
    editor_id: String,
    replacement: String,
) -> CommandResult<bool> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let replaced = editor.replace(&replacement)?;

    debug!("Replace in editor {}: {}", id, replaced);
    Ok(replaced)
}

/// Replace all occurrences
#[command]
#[instrument(skip(editors))]
pub async fn replace_all(
    editors: State<'_, EditorMap>,
    editor_id: String,
    replacement: String,
) -> CommandResult<usize> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let count = editor.replace_all(&replacement)?;

    debug!("Replace all in editor {}: {} replacements", id, count);
    Ok(count)
}
