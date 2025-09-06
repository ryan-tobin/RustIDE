use super::{CommandError, CommandResult, EditorMap, SuccessResponse};
use crate::core::{
    Direction, EditorConfig, EditorEvent, EditorMetrics, EditorState, MovementUnit, Position,
    Range, ViewState,
};
use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, instrument};
use uuid::Uuid;

/// Request to insert text at current cursor positions
#[derive(Debug, Deserialize)]
pub struct InsertTextRequest {
    pub text: String,
}

/// Request to move cursors
#[derive(Debug, Deserialize)]
pub struct MoveCursorsRequest {
    pub direction: Direction,
    pub unit: MovementUnit,
    pub extend_selection: bool,
}

/// Request to go to a specific position
#[derive(Debug, Deserialize)]
pub struct GotoPositionRequest {
    pub line: usize,
    pub column: usize,
}

/// Cursor information for the frontend
#[derive(Debug, Serialize)]
pub struct CursorInfo {
    pub positions: Vec<Position>,
    pub selections: Vec<Range>,
    pub has_selection: bool,
    pub primary_position: Position,
}

/// Editor content response
#[derive(Debug, Serialize)]
pub struct EditorContent {
    pub text: String,
    pub version: u64,
    pub line_count: usize,
    pub char_count: usize,
}

/// Get editor state
#[command]
#[instrument(skip(editors))]
pub async fn get_editor_state(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<EditorState> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    Ok(editor.state().clone())
}

/// Get editor content
#[command]
#[instrument(skip(editors))]
pub async fn get_editor_content(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<EditorContent> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    Ok(EditorContent {
        text: editor.buffer().text(),
        version: editor.buffer().version(),
        line_count: editor.state().line_count,
        char_count: editor.state().char_count,
    })
}

/// Get text in a specific range
#[command]
#[instrument(skip(editors))]
pub async fn get_text_range(
    editors: State<'_, EditorMap>,
    editor_id: String,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
) -> CommandResult<String> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let range = Range::new(
        Position::new(start_line, start_column),
        Position::new(end_line, end_column),
    );

    let text = editor.buffer().text_in_range(&range)?;
    Ok(text)
}

/// Insert text at current cursor positions
#[command]
#[instrument(skip(editors, request))]
pub async fn insert_text(
    editors: State<'_, EditorMap>,
    editor_id: String,
    request: InsertTextRequest,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.insert_text(&request.text)?;

    debug!("Inserted text in editor {}: {:?}", id, request.text);
    Ok(SuccessResponse::new("Text inserted successfully"))
}

/// Type a single character
#[command]
#[instrument(skip(editors))]
pub async fn type_character(
    editors: State<'_, EditorMap>,
    editor_id: String,
    character: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let ch = character
        .chars()
        .next()
        .ok_or_else(|| CommandError::InvalidParameter {
            parameter: "character".to_string(),
        })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.type_char(ch)?;

    debug!("Typed character in editor {}: {}", id, ch);
    Ok(SuccessResponse::new("Character typed successfully"))
}

/// Delete selection or character at cursor
#[command]
#[instrument(skip(editors))]
pub async fn delete_selection(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.delete_selection()?;

    debug!("Deleted selection in editor {}", id);
    Ok(SuccessResponse::new("Selection deleted successfully"))
}

/// Backspace operation
#[command]
#[instrument(skip(editors))]
pub async fn backspace(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.backspace()?;

    debug!("Backspace in editor {}", id);
    Ok(SuccessResponse::new("Backspace completed successfully"))
}

/// Move cursors
#[command]
#[instrument(skip(editors, request))]
pub async fn move_cursors(
    editors: State<'_, EditorMap>,
    editor_id: String,
    request: MoveCursorsRequest,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.move_cursors(request.direction, request.unit, request.extend_selection)?;

    debug!("Moved cursors in editor {}: {:?}", id, request);
    Ok(SuccessResponse::new("Cursors moved successfully"))
}

/// Get current cursor information
#[command]
#[instrument(skip(editors))]
pub async fn get_cursor_info(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<CursorInfo> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let cursor_manager = editor.cursor_manager();

    Ok(CursorInfo {
        positions: cursor_manager.cursor_positions(),
        selections: cursor_manager.selected_ranges(),
        has_selection: cursor_manager.has_selection(),
        primary_position: cursor_manager.primary_cursor().position,
    })
}

/// Go to a specific position
#[command]
#[instrument(skip(editors, request))]
pub async fn goto_position(
    editors: State<'_, EditorMap>,
    editor_id: String,
    request: GotoPositionRequest,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let position = Position::new(request.line, request.column);
    editor.goto_position(position)?;

    debug!("Moved to position in editor {}: {:?}", id, position);
    Ok(SuccessResponse::new("Moved to position successfully"))
}

/// Go to a specific line (1-indexed)
#[command]
#[instrument(skip(editors))]
pub async fn goto_line(
    editors: State<'_, EditorMap>,
    editor_id: String,
    line_number: usize,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.goto_line(line_number)?;

    debug!("Moved to line {} in editor {}", line_number, id);
    Ok(SuccessResponse::new("Moved to line successfully"))
}
/// Select all text
#[command]
#[instrument(skip(editors))]
pub async fn select_all(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.select_all()?;

    debug!("Selected all text in editor {}", id);
    Ok(SuccessResponse::new("Selected all text successfully"))
}

/// Copy selected text
#[command]
#[instrument(skip(editors))]
pub async fn copy_selection(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<String> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let copied_text = editor.copy()?;

    debug!("Copied {} characters from editor {}", copied_text.len(), id);
    Ok(copied_text)
}

/// Cut selected text
#[command]
#[instrument(skip(editors))]
pub async fn cut_selection(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<String> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let cut_text = editor.cut()?;

    debug!("Cut {} characters from editor {}", cut_text.len(), id);
    Ok(cut_text)
}

/// Paste text at cursor positions
#[command]
#[instrument(skip(editors))]
pub async fn paste_text(
    editors: State<'_, EditorMap>,
    editor_id: String,
    text: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.paste(&text)?;

    debug!("Pasted {} characters to editor {}", text.len(), id);
    Ok(SuccessResponse::new("Text pasted successfully"))
}

/// Undo operation
#[command]
#[instrument(skip(editors))]
pub async fn undo(editors: State<'_, EditorMap>, editor_id: String) -> CommandResult<bool> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let undone = editor.undo()?;

    debug!("Undo operation in editor {}: {}", id, undone);
    Ok(undone)
}

/// Redo operation
#[command]
#[instrument(skip(editors))]
pub async fn redo(editors: State<'_, EditorMap>, editor_id: String) -> CommandResult<bool> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let redone = editor.redo()?;

    debug!("Redo operation in editor {}: {}", id, redone);
    Ok(redone)
}

/// Indent selected lines
#[command]
#[instrument(skip(editors))]
pub async fn indent_lines(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.indent_lines()?;

    debug!("Indented lines in editor {}", id);
    Ok(SuccessResponse::new("Lines indented successfully"))
}

/// Unindent selected lines
#[command]
#[instrument(skip(editors))]
pub async fn unindent_lines(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.unindent_lines()?;

    debug!("Unindented lines in editor {}", id);
    Ok(SuccessResponse::new("Lines unindented successfully"))
}

/// Toggle line comments
#[command]
#[instrument(skip(editors))]
pub async fn toggle_line_comment(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.toggle_line_comment()?;

    debug!("Toggled line comments in editor {}", id);
    Ok(SuccessResponse::new("Line comments toggled successfully"))
}

/// Get current line text
#[command]
#[instrument(skip(editors))]
pub async fn get_current_line(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<String> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    let line_text = editor.current_line()?;
    Ok(line_text)
}

/// Update editor configuration
#[command]
#[instrument(skip(editors, config))]
pub async fn update_editor_config(
    editors: State<'_, EditorMap>,
    editor_id: String,
    config: EditorConfig,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.set_config(config)?;

    debug!("Updated configuration for editor {}", id);
    Ok(SuccessResponse::new("Configuration updated successfully"))
}

/// Set editor read-only mode
#[command]
#[instrument(skip(editors))]
pub async fn set_readonly(
    editors: State<'_, EditorMap>,
    editor_id: String,
    readonly: bool,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.set_readonly(readonly);

    debug!("Set read-only mode for editor {}: {}", id, readonly);
    Ok(SuccessResponse::new("Read-only mode updated successfully"))
}

/// Set editor focus state
#[command]
#[instrument(skip(editors))]
pub async fn set_focus(
    editors: State<'_, EditorMap>,
    editor_id: String,
    has_focus: bool,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.set_focus(has_focus);

    debug!("Set focus for editor {}: {}", id, has_focus);
    Ok(SuccessResponse::new("Focus state updated successfully"))
}

/// Update view state (scroll position, viewport size)
#[command]
#[instrument(skip(editors, view_state))]
pub async fn update_view_state(
    editors: State<'_, EditorMap>,
    editor_id: String,
    view_state: ViewState,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.update_view_state(view_state);

    debug!("Updated view state for editor {}", id);
    Ok(SuccessResponse::new("View state updated successfully"))
}

/// Get editor performance metrics
#[command]
#[instrument(skip(editors))]
pub async fn get_editor_metrics(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<EditorMetrics> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let editors_guard = editors.read().await;
    let editor = editors_guard
        .get(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    Ok(editor.metrics().clone())
}
