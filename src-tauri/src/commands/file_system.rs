use super::{CommandError, CommandResult, EditorMap, SuccessResponse};
use crate::core::{Editor, EditorConfig};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{command, AppHandle, Manager, State};
use tracing::{info, instrument};
use uuid::Uuid;

/// File metadata for the frontend
#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: Option<u64>,
    pub is_directory: bool,
    pub extension: Option<String>,
}

/// Directory listing response
#[derive(Debug, Serialize)]
pub struct DirectoryListing {
    pub path: PathBuf,
    pub entries: Vec<FileInfo>,
    pub parent: Option<PathBuf>,
}

/// Recent file entry
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecentFile {
    pub path: PathBuf,
    pub last_opened: u64,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// Create a new editor instance
#[command]
#[instrument(skip(editors))]
pub async fn create_editor(
    editors: State<'_, EditorMap>,
    config: Option<EditorConfig>,
) -> CommandResult<String> {
    let editor = if let Some(config) = config {
        Editor::with_config(config)
    } else {
        Editor::new()
    };

    let editor_id = editor.id();
    let editor_id_str = editor_id.to_string();

    editors.write().await.insert(editor_id, editor);

    info!("Created new editor instance: {}", editor_id_str);
    Ok(editor_id_str)
}

/// Open a file in a new or exisiting editor
#[command]
#[instrument(skip(editors))]
pub async fn open_file(
    editors: State<'_, EditorMap>,
    path: String,
    editor_id: Option<String>,
) -> CommandResult<String> {
    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(CommandError::FileError {
            message: format!("File does not exist: {}", path),
        });
    }

    if file_path.is_dir() {
        return Err(CommandError::FileError {
            message: format!("Path is a directory, not a file: {}", path),
        });
    }

    let mut editors_guard = editors.write().await;

    let (editor_id, editor) = if let Some(id_str) = editor_id {
        let id = Uuid::parse_str(&id_str).map_err(|_| CommandError::InvalidParameter {
            parameter: "editor_id".to_string(),
        })?;

        let editor = editors_guard
            .get_mut(&id)
            .ok_or_else(|| CommandError::EditorNotFound { id: id_str });

        (id, editor)
    } else {
        let editor = Editor::new();
        let id = editor.id();
        editors_guard.insert(id, editor);
        let editor = editors_guard.get_mut(&id).unwrap();
        (id, editor)
    };

    editor
        .load_file(&file_path)
        .await
        .map_err(|e| CommandError::FileError {
            message: format!("Failed to load file: {}", e),
        })?;

    info!("Opened file {} in editor {}", path, editor_id);
    Ok(editor_id.to_string())
}

/// Save the current file
#[command]
#[instrument(skip(editors))]
pub async fn save_file(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&did)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor.save().await.map_err(|e| CommandError::FileError {
        message: format!("Failed to save file: {}", e),
    })?;

    info!("Saved file for editor {}", id);
    Ok(SuccessResponse::new("File saved successfully"))
}

/// Save file to a specific path
#[command]
#[instrument(skip(editors))]
pub async fn save_file_as(
    editors: State<'_, EditorMap>,
    editor_id: String,
    path: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let file_path = PathBuf::from(&path);

    let mut editors_guard = editors.write().await;
    let editor = editors_guard
        .get_mut(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    editor
        .save_as(&file_path)
        .await
        .map_err(|e| CommandError::FileError {
            message: format!("Failed to save file: {}", e),
        })?;

    info!("Saved file as {} for editor {}", path, id);
    Ok(SuccessResponse::new("File saved successfully"))
}

/// Close and editor instance
#[command]
#[instrument(skip(editors))]
pub async fn close_editor(
    editors: State<'_, EditorMap>,
    editor_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&editor_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "editor_id".to_string(),
    })?;

    let mut editors_guard = editors.write().await;
    editors_guard
        .remove(&id)
        .ok_or_else(|| CommandError::EditorNotFound { id: editor_id })?;

    info!("Closed editor {}", id);
    Ok(SuccessResponse::new("Editor closed successfully"))
}

/// List directory contents
#[command]
#[instrument]
pub async fn list_directory(path: String) -> CommandResult<DirectoryListing> {
    let dir_path = PathBuf::from(&path);

    if !dir_path.exists() {
        return Err(CommandError::FileError {
            message: format!("Directory does not exist: {}", path),
        });
    }

    if !dir_path.is_dir() {
        return Err(CommandError::FileError {
            message: format!("Path is not a directory: {}", path),
        });
    }

    let mut entries = Vec::new();
    let mut dir_entries = tokio::fs::read_dir(&dir_path).await?;

    while let Some(entry) = dir_entries.next_entry().await? {
        let entry_path = entry.path();
        let metadata = entry.metadata().await?;

        let file_info = FileInfo {
            name: entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string(),
            path: entry_path.clone(),
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
            is_directory: metadata.is_dir(),
            extension: entry_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|s| s.to_string()),
        };

        entries.push(file_info);
    }

    // Sort entries: directories first, then by name
    entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    let parent = dir_path.parent().map(|p| p.to_path_buf());

    Ok(DirectoryListing {
        path: dir_path,
        entries,
        parent,
    })
}

/// Get file information
#[command]
#[instrument]
pub async fn get_file_info(path: String) -> CommandResult<FileInfo> {
    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(CommandError::FileError {
            message: format!("File does not exist: {}", path),
        });
    }

    let metadata = tokio::fs::metadata(&file_path).await?;

    Ok(FileInfo {
        name: file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string(),
        path: file_path.clone(),
        size: metadata.len(),
        modified: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        is_directory: metadata.is_dir(),
        extension: file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_string()),
    })
}

/// Check if a file exists
#[command]
#[instrument]
pub async fn file_exists(path: String) -> CommandResult<bool> {
    let file_path = PathBuf::from(&path);
    Ok(file_path.exists())
}

/// Create a new file
#[command]
#[instrument]
pub async fn create_file(path: String, content: Option<String>) -> CommandResult<SuccessResponse> {
    let file_path = PathBuf::from(&path);

    if file_path.exists() {
        return Err(CommandError::FileError {
            message: format!("File already exists: {}", path),
        });
    }

    // Create parent directories if they don't exist
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let content = content.unwrap_or_default();
    tokio::fs::write(&file_path, content).await?;

    info!("Created file: {}", path);
    Ok(SuccessResponse::new("File created successfully"))
}

/// Delete a file
#[command]
#[instrument]
pub async fn delete_file(path: String) -> CommandResult<SuccessResponse> {
    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(CommandError::FileError {
            message: format!("File does not exist: {}", path),
        });
    }

    if file_path.is_dir() {
        tokio::fs::remove_dir_all(&file_path).await?;
    } else {
        tokio::fs::remove_file(&file_path).await?;
    }

    info!("Deleted file: {}", path);
    Ok(SuccessResponse::new("File deleted successfully"))
}

/// Get the user's home directory
#[command]
pub async fn get_home_directory() -> CommandResult<String> {
    let home = dirs::home_dir().ok_or_else(|| CommandError::OperationFailed {
        message: "Could not determine home directory".to_string(),
    })?;

    Ok(home.to_string_lossy().to_string())
}

/// Get the current working directory
#[command]
pub async fn get_current_directory() -> CommandResult<String> {
    let cwd = std::env::current_dir().map_err(|e| CommandError::IoError {
        message: format!("Could not get current directory: {}", e),
    })?;

    Ok(cwd.to_string_lossy().to_string())
}
