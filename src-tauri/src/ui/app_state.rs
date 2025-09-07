use crate::commands::project;
use crate::core::Editor;
use crate::project::{Project, ProjectManager};
use crate::ui::{get_theme_by_name, Theme, UiError, UiEvent, UiPreferences, UiResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

/// Global application state manager
#[derive(Debug)]
pub struct AppState {
    /// Currently active project
    pub project: Option<Arc<Project>>,

    /// Project manager for handling multiple projects
    pub project_manager: Arc<ProjectManager>,

    /// Open editor instances keyed by file path
    pub editors: HashMap<PathBuf, Arc<RwLock<Editor>>>,

    /// Currently active editor (focused tab)
    pub active_editor: Option<PathBuf>,

    /// UI preferences and settings
    pub preferences: UiPreferences,

    /// Current theme
    pub current_theme: Theme,

    /// Panel states and layout
    pub panel_states: HashMap<String, PanelState>,

    /// Application-wide loading state
    pub is_loading: bool,
    pub loading_message: Option<String>,

    /// Event sender for UI updates
    pub event_sender: mpsc::UnboundedSender<UiEvent>,

    /// Recent files history
    pub recent_files: Vec<PathBuf>,

    /// Window state
    pub window_state: WindowState,
}

/// State of an individual panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelState {
    pub id: String,
    pub title: String,
    pub is_visible: bool,
    pub is_pinned: bool,
    pub size: f32, // percentage or pixels
    pub position: PanelPosition,
    pub data: serde_json::Value, // panel-specific data
}

/// Panel positioning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PanelPosition {
    Left,
    Right,
    Bottom,
    Center,
    Floating { x: f32, y: f32 },
}

/// Window state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub is_maximized: bool,
    pub is_fullscreen: bool,
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            is_maximized: false,
            is_fullscreen: false,
            width: 1200.0,
            height: 800.0,
            x: 100.0,
            y: 100.0,
        }
    }
}

/// Snapshot of current application state for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateSnapshot {
    pub project_path: Option<PathBuf>,
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    pub preferences: UiPreferences,
    pub theme: String,
    pub panel_states: HashMap<String, PanelState>,
    pub recent_files: Vec<PathBuf>,
    pub window_state: WindowState,
}

impl AppState {
    /// Create a new application state
    pub fn new(
        project_manager: Arc<ProjectManager>,
    ) -> UiResult<(Self, mpsc::UnboundedReceiver<UiEvent>)> {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let preferences = UiPreferences::default();
        let current_theme = get_theme_by_name(&preferences.theme).unwrap_or_else(Theme::default);

        let state = Self {
            project: None,
            project_manager,
            editors: HashMap::new(),
            active_editor: None,
            preferences,
            current_theme,
            panel_states: Self::default_panel_states(),
            is_loading: false,
            loading_message: None,
            event_sender,
            recent_files: Vec::new(),
            window_state: WindowState::default(),
        };

        Ok((state, event_receiver))
    }

    /// Get default panel states for a new workspace
    fn default_panel_states() -> HashMap<String, PanelState> {
        let mut panels = HashMap::new();

        panels.insert(
            "file_explorer".to_string(),
            PanelState {
                id: "file_explorer".to_string(),
                title: "Explorer".to_string(),
                is_visible: true,
                is_pinned: true,
                size: 300.0,
                position: PanelPosition::Left,
                data: serde_json::json!({}),
            },
        );

        panels.insert(
            "terminal".to_string(),
            PanelState {
                id: "terminal".to_string(),
                title: "Terminal".to_string(),
                is_visible: true,
                is_pinned: false,
                size: 200.0,
                position: PanelPosition::Bottom,
                data: serde_json::json!({}),
            },
        );

        panels.insert(
            "problems".to_string(),
            PanelState {
                id: "problems.to".to_string(),
                title: "Problems".to_string(),
                is_visible: false,
                is_pinned: false,
                size: 150.0,
                position: PanelPosition::Bottom,
                data: serde_json::json!({}),
            },
        );

        panels
    }

    /// Set the current project
    pub async fn set_project(&mut self, project: Arc<Project>) -> UiResult<()> {
        self.close_all_editors().await?;

        self.project = Some(project.clone());

        self.panel_states = Self::default_panel_states();

        self.emit_state_updated()?;

        Ok(())
    }

    /// Open a file in the editor
    pub async fn open_file(&mut self, file_path: PathBuf) -> UiResult<()> {
        if !self.editors.contains_key(&file_path) {
            let editor = Editor::new();

            // TODO: Integrate
            let editor = Arc::new(RwLock::new(editor));
            self.editors.insert(file_path.clone(), editor);
        }

        self.active_editor = Some(file_path.clone());

        self.add_to_recent_files(file_path);

        self.emit_state_updated()?;

        Ok(())
    }

    /// Close a file editor
    pub async fn close_file(&mut self, file_path: &PathBuf) -> UiResult<()> {
        self.editors.remove(file_path);

        if self.active_editor.as_ref() == Some(file_path) {
            self.active_editor = self.editors.keys().next().cloned();
        }

        self.emit_state_updated()?;

        Ok(())
    }

    /// Close all editors
    pub async fn close_all_editors(&mut self) -> UiResult<()> {
        self.editors.clear();
        self.active_editor = None;
        self.emit_state_updated()?;
        Ok(())
    }

    /// Get the active editor
    pub fn get_active_editor(&self) -> Option<Arc<RwLock<Editor>>> {
        self.active_editor
            .as_ref()
            .and_then(|path| self.editors.get(path))
            .cloned()
    }

    /// Get the editor for a specific file
    pub fn get_editor(&self, file_path: &PathBuf) -> Option<Arc<RwLock<Editor>>> {
        self.editors.get(file_path).cloned()
    }

    /// Update UI preferences
    pub async fn update_preferences(&mut self, preferences: UiPreferences) -> UiResult<()> {
        if preferences.theme != self.preferences.theme {
            if let Some(theme) = get_theme_by_name(&preferences.theme) {
                self.current_theme = theme.clone();
                self.emit_theme_changed(theme)?;
            }
        }

        self.preferences = preferences;
        self.emit_state_updated()?;

        Ok(())
    }

    /// Update panel state
    pub fn update_panel_state(&mut self, panel_id: String, state: PanelState) -> UiResult<()> {
        self.panel_states.insert(panel_id, state.clone());

        self.event_sender
            .send(UiEvent::PanelUpdated {
                panel: crate::ui::panels::PanelInfo {
                    id: state.id,
                    title: state.title,
                    is_visible: state.is_visible,
                    position: state.position,
                    size: state.size,
                    data: state.data,
                },
            })
            .map_err(|_| UiError::EventError {
                event_type: "PanelUpdated".to_string(),
            })?;

        Ok(())
    }

    /// Toggle panel visibility
    pub fn toggle_panel(&mut self, panel_id: &str) -> UiResult<()> {
        if let Some(panel) = self.panel_states.get_mut(panel_id) {
            panel.is_visible = !panel.is_visible;
            self.update_panel_state(panel_id.to_string(), panel.clone())?;
        } else {
            return Err(UiError::PanelNotFound {
                panel_id: panel_id.to_string(),
            });
        }
        Ok(())
    }

    /// Set loading state
    pub fn set_loading(&mut self, is_loading: bool, message: Option<String>) -> UiResult<()> {
        self.is_loading = is_loading;
        self.loading_message = message.clone();

        self.event_sender
            .send(UiEvent::LoadingChanged {
                is_loading,
                message,
            })
            .map_err(|_| UiError::EventError {
                event_type: "LoadingChanged".to_string(),
            })?;

        Ok(())
    }

    /// Show a notification
    pub fn show_notification(
        &self,
        level: crate::ui::NotificationLevel,
        message: String,
        duration: Option<u64>,
    ) -> UiResult<()> {
        self.event_sender
            .send(UiEvent::Notification {
                level,
                message,
                duration,
            })
            .map_err(|_| UiError::EventError {
                event_type: "Notification".to_string(),
            })?;

        Ok(())
    }

    /// Add file to recent files list
    fn add_to_recent_files(&mut self, file_path: PathBuf) {
        self.recent_files.retain(|path| path != &file_path);

        self.recent_files.insert(0, file_path);

        self.recent_files.truncate(20);
    }

    /// Update window state
    pub fn update_window_state(&mut self, window_state: WindowState) -> UiResult<()> {
        self.window_state = window_state;
        self.emit_state_updated()?;
        Ok(())
    }

    /// Create a snapshot of current state
    pub fn create_snapshot(&self) -> AppStateSnapshot {
        AppStateSnapshot {
            project_path: self.project.as_ref().map(|p| p.root_path().clone()),
            open_files: self.editors.keys().cloned().collect(),
            active_file: self.active_editor.clone(),
            preferences: self.preferences.clone(),
            theme: self.current_theme.name.clone(),
            panel_states: self.panel_states.clone(),
            recent_files: self.recent_files.clone(),
            window_state: self.window_state.clone(),
        }
    }

    /// Restore state from snapshot
    pub async fn restore_from_snapshot(&mut self, snapshot: AppStateSnapshot) -> UiResult<()> {
        self.preferences = snapshot.preferences;
        if let Some(theme) = get_theme_by_name(&snapshot.theme) {
            self.current_theme = theme;
        }

        self.panel_states = snapshot.panel_states;

        self.window_state = snapshot.window_state;

        self.recent_files = snapshot.recent_files;

        if let Some(project_path) = snapshot.project_path {
            if let Ok(project) = self.project_manager.load_project(&project_path).await {
                self.project = Some(project);
            }
        }

        for file_path in snapshot.open_files {
            if file_path.exists() {
                self.open_file(file_path).await?;
            }
        }

        if let Some(active_file) = snapshot.active_file {
            if self.editors.contains_key(&active_file) {
                self.active_editor = Some(active_file);
            }
        }

        self.emit_state_updated()?;

        Ok(())
    }

    /// Emit state updated event
    fn emit_state_updated(&self) -> UiResult<()> {
        self.event_sender
            .send(UiEvent::StateUpdated {
                state: self.create_snapshot(),
            })
            .map_err(|_| UiError::EventError {
                event_type: "StateUpdated".to_string(),
            })?;

        Ok(())
    }

    /// Emit theme changed event
    fn emit_theme_changed(&self, theme: Theme) -> UiResult<()> {
        self.event_sender
            .send(UiEvent::ThemeChanged { theme })
            .map_err(|_| UiError::EventError {
                event_type: "ThemeChanged".to_string(),
            })?;

        Ok(())
    }
}

/// Application state manager that can be shared across Tauri commands
pub type SharedAppState = Arc<RwLock<AppState>>;

/// Initialize the app state
pub async fn initialize_app_state(
    project_manager: Arc<ProjectManager>,
) -> UiResult<(SharedAppState, mpsc::UnboundedReceiver<UiEvent>)> {
    let (state, event_receiver) = AppState::new(project_manager)?;
    let shared_state = Arc::new(RwLock::new(state));
    Ok((shared_state, event_receiver))
}

/// Setup UI event forwarding to the frontend
pub async fn setup_ui_events(
    app_handle: AppHandle,
    mut event_receiver: mpsc::UnboundedReceiver<UiEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = event_receiver.recv().await {
            if let Err(e) = app_handle.emit_all("ui-event", &event) {
                eprintln!("Failed to emit UI event: {}", e);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectManager;
    use tempfile::tempdir;

    async fn create_test_state() -> (AppState, mpsc::UnboundedReceiver<UiEvent>) {
        let temp_dir = tempdir().unwrap();
        let project_manager = Arc::new(ProjectManager::new(temp_dir.path().to_path_buf()));
        AppState::new(project_manager).unwrap()
    }

    #[tokio::test]
    async fn test_app_state_creation() {
        let (state, _) = create_test_state().await;

        assert!(state.project.is_none());
        assert!(state.editors.is_empty());
        assert!(state.active_editor.is_none());
        assert_eq!(state.preferences.theme, "dark");
        assert!(!state.is_loading);
    }

    #[tokio::test]
    async fn test_file_operations() {
        let (mut state, _) = create_test_state().await;
        let file_path = PathBuf::from("/tmp/test.rs");

        // Test opening file
        state.open_file(file_path.clone()).await.unwrap();
        assert!(state.editors.contains_key(&file_path));
        assert_eq!(state.active_editor, Some(file_path.clone()));
        assert_eq!(state.recent_files[0], file_path);

        // Test closing file
        state.close_file(&file_path).await.unwrap();
        assert!(!state.editors.contains_key(&file_path));
        assert!(state.active_editor.is_none());
    }

    #[tokio::test]
    async fn test_panel_operations() {
        let (mut state, _) = create_test_state().await;

        // Test panel toggle
        assert!(state.panel_states.get("file_explorer").unwrap().is_visible);
        state.toggle_panel("file_explorer").unwrap();
        assert!(!state.panel_states.get("file_explorer").unwrap().is_visible);

        // Test invalid panel
        assert!(state.toggle_panel("nonexistent").is_err());
    }

    #[tokio::test]
    async fn test_preferences_update() {
        let (mut state, _) = create_test_state().await;
        let mut new_prefs = state.preferences.clone();
        new_prefs.theme = "light".to_string();
        new_prefs.font_size = 16;

        state.update_preferences(new_prefs.clone()).await.unwrap();
        assert_eq!(state.preferences.font_size, 16);
        assert_eq!(state.current_theme.name, "light");
    }

    #[tokio::test]
    async fn test_state_snapshot() {
        let (mut state, _) = create_test_state().await;
        let file_path = PathBuf::from("/tmp/test.rs");

        state.open_file(file_path.clone()).await.unwrap();

        let snapshot = state.create_snapshot();
        assert!(snapshot.open_files.contains(&file_path));
        assert_eq!(snapshot.active_file, Some(file_path));
        assert_eq!(snapshot.theme, "dark");
    }

    #[tokio::test]
    async fn test_loading_state() {
        let (mut state, _) = create_test_state().await;

        state
            .set_loading(true, Some("Loading project...".to_string()))
            .unwrap();
        assert!(state.is_loading);
        assert_eq!(
            state.loading_message,
            Some("Loading project...".to_string())
        );

        state.set_loading(false, None).unwrap();
        assert!(!state.is_loading);
        assert!(state.loading_message.is_none());
    }
}
