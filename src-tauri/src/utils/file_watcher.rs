use crate::utils::{debounce, UtilError, UtilResult};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

/// Error types for file watching
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("Watcher creation failed: {message}")]
    CreationFailed { message: String },

    #[error("Watcher not found: {id}")]
    NotFound { id: String },

    #[error("Path not found: {path}")]
    PathNotFound { path: String },

    #[error("Permission denied: {path}")]
    PermissionDenied { path: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Notify error: {0}")]
    Notify(#[from] notify::Error),
}

/// File system event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FileEvent {
    Created {
        path: PathBuf,
    },
    Modified {
        path: PathBuf,
    },
    Deleted {
        path: PathBuf,
    },
    Renamed {
        old_path: PathBuf,
        new_path: PathBuf,
    },
    MetadataChanged {
        path: PathBuf,
    },
}

/// Configuration for file watching
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Whether to watch recursively
    pub recursive: bool,
    /// Debounce delay to avoid rapid events
    pub debounce_delay: Duration,
    /// Patterns to ignore
    pub ignore_patterns: Vec<String>,
    /// Maximum depth for recursive watching
    pub max_depth: Option<usize>,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            recursive: true,
            debounce_delay: Duration::from_millis(500),
            ignore_patterns: vec![
                ".git".to_string(),
                "target".to_string(),
                "node_modules".to_string(),
                ".DS_Store".to_string(),
                "*.swp".to_string(),
                "*.tmp".to_string(),
            ],
            max_depth: Some(10),
        }
    }
}

/// Information about a file watcher
#[derive(Debug, Clone)]
pub struct WatcherInfo {
    pub id: Uuid,
    pub path: PathBuf,
    pub config: WatchConfig,
    pub created_at: std::time::Instant,
}

/// Manages file system watchers
pub struct FileWatcher {
    watchers: Arc<RwLock<HashMap<Uuid, (WatcherInfo, RecommendedWatcher)>>>,
    event_sender: mpsc::UnboundedSender<(Uuid, FileEvent)>,
}

impl FileWatcher {
    /// Create a new file watcher manager
    pub fn new() -> (Self, mpsc::UnboundedReceiver<(Uuid, FileEvent)>) {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let manager = Self {
            watchers: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
        };

        (manager, event_receiver)
    }

    /// Start watching a path
    #[instrument(skip(self, config))]
    pub async fn watch_path<P: AsRef<Path>>(
        &self,
        path: P,
        config: WatchConfig,
    ) -> UtilResult<Uuid> {
        let path = path.as_ref().to_path_buf();
        let watcher_id = Uuid::new_v4();

        if !path.exists() {
            return Err(UtilError::Watch(WatchError::PathNotFound {
                path: path.display(),
            }));
        }

        let event_sender = self.event_sender.clone();
        let ignore_patterns = config.ignore_patterns.clone();
        let debounce_delay = config.debounce_delay;

        let (tx, mut rx) = mpsc::unbounded_channel();

        let debounce_watcher_id = watcher_id;
        tokio::spawn(async move {
            let mut debouncer = crate::utils::debounce::Debouncer::new(debounce_delay);

            while let Some(event) = rx.recv().await {
                let event_key = format!("{:?}", event);

                debouncer
                    .debounce(event_key, move || {
                        let _ = event_sender.send((debounce_watcher_id, event.clone()));
                    })
                    .await;
            }
        });

        let mut watcher = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| match result {
                Ok(event) => {
                    if let Some(file_event) = convert_notify_event(event, &ignore_patterns) {
                        let _ = tx.send(file_event);
                    }
                }
                Err(error) => {
                    error!("File watcher error: {}", error);
                }
            },
            Config::default(),
        )
        .map_err(|e| WatchError::CreationFailed {
            message: e.to_string(),
        })?;

        let recursive_mode = if config.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        watcher
            .watch(&path, recursive_mode)
            .map_err(|e| match e.kind {
                notify::ErrorKind::PathNotFound => WatchError::PathNotFound {
                    path: path.display().to_string(),
                },
                notify::ErrorKind::PermissionDenied => WatchError::PermissionDenied {
                    path: path.display().to_string(),
                },
                _ => WatchError::CreationFailed {
                    message: e.to_string(),
                },
            })?;

        let watcher_info = WatcherInfo {
            id: watcher_id,
            path: path.clone(),
            config,
            created_at: std::time::Instant::now(),
        };

        self.watchers
            .write()
            .await
            .insert(watcher_id, (watcher_info, watcher));

        info!(
            "Started watching path: {} (ID: {})",
            path.display(),
            watcher_id
        );
        Ok(watcher_id)
    }

    /// Stop watching a path
    #[instrument(skip(self))]
    pub async fn stop_watching(&self, watcher_id: Uuid) -> UtilResult<()> {
        let mut watchers = self.watchers.write().await;

        if let Some((info, _watcher)) = watchers.remove(&watcher_id) {
            info!(
                "Stopped watching path: {} (ID: {})",
                info.path.display(),
                watcher_id
            );
            Ok(())
        } else {
            Err(UtilError::Watch(WatchError::NotFound {
                id: watcher_id.to_string(),
            }))
        }
    }

    /// Get information about a watcher
    pub async fn get_watcher_info(&self, watcher_id: Uuid) -> Option<WatcherInfo> {
        let watchers = self.watchers.read().await;
        watchers.get(&watcher_id).map(|(info, _)| info.clone())
    }

    /// List all active watchers
    pub async fn list_watchers(&self) -> Vec<WatcherInfo> {
        let watchers = self.watchers.read().await;
        watchers.values().map(|(info, _)| info.clone()).collect()
    }

    /// Stop all watchers
    pub async fn stop_all_watchers(&self) -> UtilResult<()> {
        let watcher_ids: Vec<Uuid> = {
            let watchers = self.watchers.read().await;
            watchers.keys().copied().collect()
        };

        for watcher_id in watcher_ids {
            if let Err(e) = self.stop_watching(watcher_id).await {
                warn!("Failed to stop watcher {}: {}", watcher_id, e);
            }
        }

        Ok(())
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        let (watcher, _) = Self::new();
        watcher
    }
}

/// Convert notify event to our FileEvent
fn convert_notify_event(event: Event, ignore_patterns: &[String]) -> Option<FileEvent> {
    if event.paths.is_empty() {
        return None;
    }

    let path = &event.paths[0];

    if should_ignore_path(path, ignore_patterns) {
        return None;
    }

    match event.kind {
        EventKind::Create(_) => Some(FileEvent::Created { path: path.clone() }),
        EventKind::Modify(_) => Some(FileEvent::Modified { path: path.clone() }),
        EventKind::Remove(_) => Some(FileEvent::Deleted { path: path.clone() }),
        EventKind::Any | EventKind::Other => {
            if path.exists() {
                Some(FileEvent::Modified { path: path.clone() })
            } else {
                Some(FileEvent::Deleted { path: path.clone() })
            }
        }
        _ => None,
    }
}

/// Check if a path should be ignored based on patterns
fn should_ignore_path(path: &Path, ignore_patterns: &[String]) -> bool {
    let path_str = path.to_string_lossy();

    for pattern in ignore_patterns {
        if pattern.contains("*") {
            if glob_match(pattern, &path_str) {
                return true;
            }
        } else if path_str.contains(pattern) {
            return true;
        }
    }

    false
}

/// Simple glob pattern matching
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern.ends_with("*") {
        let prefix = &pattern[..pattern.len() - 1];
        text.starts_with(prefix)
    } else if pattern.starts_with("*") {
        let suffix = &pattern[1..];
        text.ends_with(suffix)
    } else {
        pattern == text
    }
}

// Global file watcher
static FILE_WATCHER: once_cell::sync::OnceCell<FileWatcher> = once_cell::sync::OnceCell::new();

/// Initialize global file watcher
pub fn init_file_watcher() -> mpsc::UnboundedReceiver<(Uuid, FileEvent)> {
    let (watcher, receiver) = FileWatcher::new();

    if FILE_WATCHER.set(watcher).is_err() {
        panic!("File watcher already initialized");
    }

    receiver
}

/// Get global file watcher
pub fn get_file_watcher() -> &'static FileWatcher {
    FILE_WATCHER.get().expect("File watcher not initialized")
}

/// Shutdown all watchers
pub async fn shutdown_watchers() -> UtilResult<()> {
    if let Some(watcher) = FILE_WATCHER.get() {
        watcher.stop_all_watchers().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_file_watcher() {
        let temp_dir = TempDir::new().unwrap();
        let (watcher, mut events) = FileWatcher::new();

        // Start watching the temp directory
        let watcher_id = watcher
            .watch_path(temp_dir.path(), WatchConfig::default())
            .await
            .unwrap();

        // Create a file
        let test_file = temp_dir.path().join("test.txt");
        tokio::fs::write(&test_file, "test content").await.unwrap();

        // Wait for event
        sleep(Duration::from_millis(100)).await;

        // Should receive a created event
        if let Ok((id, event)) = tokio::time::timeout(Duration::from_secs(1), events.recv()).await {
            assert_eq!(id.unwrap(), watcher_id);
            match event.unwrap() {
                FileEvent::Created { path } => assert_eq!(path, test_file),
                _ => panic!("Expected Created event"),
            }
        }

        // Stop watching
        watcher.stop_watching(watcher_id).await.unwrap();
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("test*", "test_file.txt"));
        assert!(!glob_match("*.rs", "main.py"));
        assert!(!glob_match("test*", "other.txt"));
    }

    #[test]
    fn test_should_ignore_path() {
        let patterns = vec![".git".to_string(), "*.tmp".to_string()];

        assert!(should_ignore_path(Path::new(".git/config"), &patterns));
        assert!(should_ignore_path(Path::new("file.tmp"), &patterns));
        assert!(!should_ignore_path(Path::new("src/main.rs"), &patterns));
    }
}
