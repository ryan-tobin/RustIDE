use crate::ui::{UiError, UiEvent, UiResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing_subscriber::fmt::format::DefaultFields;
use uuid::Uuid;

/// Information about a panel that can be displayed in the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInfo {
    pub id: String,
    pub title: String,
    pub is_visible: bool,
    pub position: PanelPosition,
    pub size: f32,               // percentage or pixels depending on position
    pub data: serde_json::Value, // panel-specific data
}

/// Panel positioning options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PanelPosition {
    Left,
    Right,
    Bottom,
    Center,
    Floating {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

/// Panel layout configuration that defines how panels are arranged
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelLayout {
    pub name: String,
    pub panels: HashMap<String, PanelInfo>,
    pub tab_groups: HashMap<String, TabGroup>,
    pub splitter_positions: HashMap<String, f32>, // splitter_id -> position
}

/// A group of tabbed panels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabGroup {
    pub id: String,
    pub position: PanelPosition,
    pub panel_ids: Vec<String>,
    pub active_panel: Option<String>,
    pub size: f32,
}

/// Panel type definitions for different IDE components
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PanelType {
    /// File explorer/project tree
    FileExplorer,
    /// Search across files
    Search,
    /// Source control (git)
    SourceControl,
    /// Extensions/plugins
    Extensions,
    /// Terminal/console
    Terminal,
    /// Problems/diagnostics
    Problems,
    /// Output logs
    Output,
    /// Debug console
    Debug,
    /// Test results
    Tests,
    /// Custom user panel
    Custom { plugin_id: String },
}

impl PanelType {
    /// Get the default title for this panel type
    pub fn default_title(&self) -> &'static str {
        match self {
            PanelType::FileExplorer => "Explorer",
            PanelType::Search => "Search",
            PanelType::SourceControl => "Source Control",
            PanelType::Extensions => "Extensions",
            PanelType::Terminal => "Terminal",
            PanelType::Problems => "Problems",
            PanelType::Output => "Output",
            PanelType::Debug => "Debug Console",
            PanelType::Tests => "Test Results",
            PanelType::Custom { .. } => "Custom Panel",
        }
    }

    /// Get the default position for this panel type
    pub fn default_position(&self) -> PanelPosition {
        match self {
            PanelType::FileExplorer
            | PanelType::Search
            | PanelType::SourceControl
            | PanelType::Extensions => PanelPosition::Left,
            PanelType::Terminal
            | PanelType::Problems
            | PanelType::Output
            | PanelType::Debug
            | PanelType::Tests => PanelPosition::Bottom,
            PanelType::Custom { .. } => PanelPosition::Right,
        }
    }

    /// Get the default size for this panel type
    pub fn default_size(&self) -> f32 {
        match self {
            PanelType::FileExplorer
            | PanelType::Search
            | PanelType::SourceControl
            | PanelType::Extensions => 300.0, // pixels
            PanelType::Terminal | PanelType::Output | PanelType::Debug => 200.0, // pixels
            PanelType::Problems | PanelType::Tests => 150.0,                     // pixels
            PanelType::Custom { .. } => 250.0,                                   // pixels
        }
    }
}

/// Panel manager for handling the docking system
#[derive(Debug)]
pub struct PanelManager {
    panels: HashMap<String, PanelInfo>,
    layouts: HashMap<String, PanelLayout>,
    current_layout: String,
    event_sender: tokio::sync::mpsc::UnboundedSender<UiEvent>,
}

impl PanelManager {
    /// Create a new panel manager
    pub fn new(event_sender: tokio::sync::mpsc::UnboundedSender<UiEvent>) -> Self {
        let mut manager = Self {
            panels: HashMap::new(),
            layouts: HashMap::new(),
            current_layout: "default".to_string(),
            event_sender,
        };

        manager.create_default_layout();
        manager
    }

    /// Create the default panel layout
    fn create_default_layout(&mut self) {
        let mut panels = HashMap::new();
        let mut tab_groups = HashMap::new();

        let left_panels = vec![
            self.create_panel(PanelType::FileExplorer, true),
            self.create_panel(PanelType::Search, false),
            self.create_panel(PanelType::SourceControl, false),
            self.create_panel(PanelType::Extensions, false),
        ];

        for panel in left_panels.iter() {
            panels.insert(panel.id.clone(), panel.clone());
        }

        tab_groups.insert(
            "left_sidebar".to_string(),
            TabGroup {
                id: "left_sidebar".to_string(),
                position: PanelPosition::Left,
                panel_ids: left_panels.iter().map(|p| p.id.clone()).collect(),
                active_panel: Some(left_panels[0].id.clone()),
                size: 300.0,
            },
        );

        let bottom_panels = vec![
            self.create_panel(PanelType::Terminal, true),
            self.create_panel(PanelType::Problems, false),
            self.create_panel(PanelType::Output, false),
            self.create_panel(PanelType::Debug, false),
        ];

        for panel in bottom_panels.iter() {
            panels.insert(panel.id.clone(), panel.clone());
        }

        tab_groups.insert(
            "bottom_panel".to_string(),
            TabGroup {
                id: "bottom_panel".to_string(),
                position: PanelPosition::Bottom,
                panel_ids: bottom_panels.iter().map(|p| p.id.clone()).collect(),
                active_panel: Some(bottom_panels[0].id.clone()),
                size: 200.0,
            },
        );

        let layout = PanelLayout {
            name: "default".to_string(),
            panels,
            tab_groups,
            splitter_positions: HashMap::new(),
        };

        self.layouts.insert("default".to_string(), layout);

        if let Some(layout) = self.layouts.get("default") {
            for panel in layout.panels.values() {
                self.panels.insert(panel.id.clone(), panel.clone());
            }
        }
    }

    /// Create a panel with default settings
    fn create_panel(&self, panel_type: PanelType, is_visible: bool) -> PanelInfo {
        PanelInfo {
            id: format!(
                "{}_{}",
                panel_type.default_title().to_lowercase().replace(" ", "_"),
                Uuid::new_v4()
            ),
            title: panel_type.default_title().to_string(),
            is_visible,
            position: panel_type.default_position(),
            size: panel_type.default_size(),
            data: serde_json::json!({"type" : panel_type}),
        }
    }

    /// Add a new panel
    pub fn add_panel(&mut self, panel_type: PanelType, title: Option<String>) -> UiResult<String> {
        let mut panel = self.create_panel(panel_type, true);

        if let Some(title) = title {
            panel.title = title;
        }

        let panel_id = panel.id.clone();
        self.panels.insert(panel_id.clone(), panel.clone());

        if let Some(layout) = self.layouts.get_mut(&self.current_layout) {
            layout.panels.insert(panel_id.clone(), panel.clone());
        }

        self.emit_panel_updated(panel)?;

        Ok(panel_id)
    }

    /// Remove a panel
    /// Remove a panel
    pub fn remove_panel(&mut self, panel_id: &str) -> UiResult<()> {
        if self.panels.remove(panel_id).is_none() {
            return Err(UiError::PanelNotFound {
                panel_id: panel_id.to_string(),
            });
        }

        // Remove from current layout
        if let Some(layout) = self.layouts.get_mut(&self.current_layout) {
            layout.panels.remove(panel_id);

            // Remove from tab groups
            for tab_group in layout.tab_groups.values_mut() {
                tab_group.panel_ids.retain(|id| id != panel_id);
                if tab_group.active_panel.as_ref() == Some(&panel_id.to_string()) {
                    tab_group.active_panel = tab_group.panel_ids.first().cloned();
                }
            }
        }

        // Emit panel removal event
        self.event_sender
            .send(UiEvent::PanelRemoved {
                panel_id: panel_id.to_string(),
            })
            .map_err(|_| UiError::EventError {
                event_type: "PanelRemoved".to_string(),
            })?;

        Ok(())
    }

    /// Update panel properties
    pub fn update_panel(&mut self, panel_id: &str, updates: PanelUpdate) -> UiResult<()> {
        let panel = self
            .panels
            .get_mut(panel_id)
            .ok_or_else(|| UiError::PanelNotFound {
                panel_id: panel_id.to_string(),
            })?;

        if let Some(title) = updates.title {
            panel.title = title;
        }

        if let Some(is_visible) = updates.is_visible {
            panel.is_visible = is_visible;
        }

        if let Some(position) = updates.position {
            panel.position = position;
        }

        if let Some(size) = updates.size {
            panel.size = size;
        }

        if let Some(data) = updates.data {
            panel.data = data;
        }

        // Update in current layout
        if let Some(layout) = self.layouts.get_mut(&self.current_layout) {
            layout.panels.insert(panel_id.to_string(), panel.clone());
        }

        // Emit update event
        self.emit_panel_updated(panel.clone())?;

        Ok(())
    }

    /// Toggle panel visibility
    pub fn toggle_panel(&mut self, panel_id: &str) -> UiResult<()> {
        let is_visible = self
            .panels
            .get(panel_id)
            .map(|p| !p.is_visible)
            .ok_or_else(|| UiError::PanelNotFound {
                panel_id: panel_id.to_string(),
            })?;

        self.update_panel(
            panel_id,
            PanelUpdate {
                is_visible: Some(is_visible),
                ..Default::default()
            },
        )
    }

    /// Move panel to a different position
    pub fn move_panel(&mut self, panel_id: &str, new_position: PanelPosition) -> UiResult<()> {
        self.update_panel(
            panel_id,
            PanelUpdate {
                position: Some(new_position),
                ..Default::default()
            },
        )
    }

    /// Resize a panel
    pub fn resize_panel(&mut self, panel_id: &str, new_size: f32) -> UiResult<()> {
        self.update_panel(
            panel_id,
            PanelUpdate {
                size: Some(new_size),
                ..Default::default()
            },
        )
    }

    /// Get panel information
    pub fn get_panel(&self, panel_id: &str) -> Option<&PanelInfo> {
        self.panels.get(panel_id)
    }

    /// Get all panels
    pub fn get_all_panels(&self) -> &HashMap<String, PanelInfo> {
        &self.panels
    }

    /// Get current layout
    pub fn get_current_layout(&self) -> Option<&PanelLayout> {
        self.layouts.get(&self.current_layout)
    }

    /// Switch to a different layout
    pub fn switch_layout(&mut self, layout_name: &str) -> UiResult<()> {
        if !self.layouts.contains_key(layout_name) {
            return Err(UiError::LayoutError {
                operation: format!("Layout '{}' not found", layout_name),
            });
        }

        self.current_layout = layout_name.to_string();

        // Update panels from new layout
        if let Some(layout) = self.layouts.get(layout_name) {
            self.panels = layout.panels.clone();

            // Emit layout change event
            self.event_sender
                .send(UiEvent::LayoutChanged {
                    layout: layout.clone(),
                })
                .map_err(|_| UiError::EventError {
                    event_type: "LayoutChanged".to_string(),
                })?;
        }

        Ok(())
    }

    /// Save current state as a new layout
    pub fn save_layout(&mut self, name: String) -> UiResult<()> {
        let mut layout = PanelLayout {
            name: name.clone(),
            panels: self.panels.clone(),
            tab_groups: HashMap::new(),
            splitter_positions: HashMap::new(),
        };

        // Build tab groups from current panel positions
        self.build_tab_groups(&mut layout);

        self.layouts.insert(name, layout);
        Ok(())
    }

    /// Build tab groups from panel positions
    fn build_tab_groups(&self, layout: &mut PanelLayout) {
        let mut position_groups: HashMap<String, Vec<String>> = HashMap::new();

        // Group panels by position
        for (panel_id, panel) in &layout.panels {
            let key = match &panel.position {
                PanelPosition::Left => "left".to_string(),
                PanelPosition::Right => "right".to_string(),
                PanelPosition::Bottom => "bottom".to_string(),
                PanelPosition::Center => "center".to_string(),
                PanelPosition::Floating { .. } => format!("floating_{}", panel_id),
            };

            position_groups
                .entry(key)
                .or_default()
                .push(panel_id.clone());
        }

        // Create tab groups
        for (position_key, panel_ids) in position_groups {
            if panel_ids.len() > 1 {
                let position = if position_key.starts_with("floating_") {
                    // Find the floating panel's position
                    if let Some(panel_id) = panel_ids.first() {
                        if let Some(panel) = layout.panels.get(panel_id) {
                            panel.position.clone()
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    match position_key.as_str() {
                        "left" => PanelPosition::Left,
                        "right" => PanelPosition::Right,
                        "bottom" => PanelPosition::Bottom,
                        "center" => PanelPosition::Center,
                        _ => continue,
                    }
                };

                let tab_group = TabGroup {
                    id: format!("{}_group", position_key),
                    position,
                    panel_ids: panel_ids.clone(),
                    active_panel: panel_ids.first().cloned(),
                    size: layout
                        .panels
                        .get(&panel_ids[0])
                        .map(|p| p.size)
                        .unwrap_or(200.0),
                };

                layout.tab_groups.insert(tab_group.id.clone(), tab_group);
            }
        }
    }

    /// get available layouts
    pub fn get_available_layouts(&self) -> Vec<String> {
        self.layouts.keys().cloned().collect()
    }

    /// Reset to default layout
    pub fn reset_to_default(&mut self) -> UiResult<()> {
        self.create_default_layout();
        self.switch_layout("default")
    }

    /// Emit panel updated event
    fn emit_panel_updated(&self, panel: PanelInfo) -> UiResult<()> {
        self.event_sender
            .send(UiEvent::PanelUpdated { panel })
            .map_err(|_| UiError::EventError {
                event_type: "PanelUpdated".to_string(),
            })
    }
}

/// Panel update structure for partial updates
#[derive(Debug, Default)]
pub struct PanelUpdate {
    pub title: Option<String>,
    pub is_visible: Option<bool>,
    pub position: Option<PanelPosition>,
    pub size: Option<f32>,
    pub data: Option<serde_json::Value>,
}

/// Predefined layout configs
pub struct LayoutPresets;

impl LayoutPresets {
    /// Minimal layour with just file explorer and terminal
    pub fn minimal() -> PanelLayout {
        let mut panels = HashMap::new();
        let mut tab_groups = HashMap::new();

        let explorer = PanelInfo {
            id: "file_explorer".to_string(),
            title: "Explorer".to_string(),
            is_visible: true,
            position: PanelPosition::Left,
            size: 250.0,
            data: serde_json::json!({"type" : "FileExplorer"}),
        };
        panels.insert(explorer.id.clone(), explorer);

        let terminal = PanelInfo {
            id: "terminal".to_string(),
            title: "Terminal".to_string(),
            is_visible: true,
            position: PanelPosition::Bottom,
            size: 150.0,
            data: serde_json::json!({"type" : "Terminal"}),
        };
        panels.insert(terminal.id.clone(), terminal);

        PanelLayout {
            name: "minimal".to_string(),
            panels,
            tab_groups,
            splitter_positions: HashMap::new(),
        }
    }

    /// Dev layour with debugging panels
    pub fn development() -> PanelLayout {
        let mut panels = HashMap::new();
        let mut tab_groups = HashMap::new();

        let left_panels = vec![
            ("file_explorer", "Explorer", "FileExplorer"),
            ("search", "Search", "Search"),
            ("source_control", "Source Control", "SourceControl"),
        ];

        for (id, title, panel_type) in left_panels {
            let panel = PanelInfo {
                id: id.to_string(),
                title: title.to_string(),
                is_visible: true,
                position: PanelPosition::Left,
                size: 300.0,
                data: serde_json::json!({"type" : panel_type}),
            };
            panels.insert(panel.id.clone(), panel);
        }

        let bottom_panels = vec![
            ("terminal", "Terminal", "Terminal"),
            ("problems", "Problems", "Problems"),
            ("debug", "Debug Console", "Debug"),
            ("tests", "Test Results", "Tests"),
        ];

        for (id, title, panel_type) in bottom_panels {
            let panel = PanelInfo {
                id: id.to_string(),
                title: title.to_string(),
                is_visible: id == "terminal",
                position: PanelPosition::Bottom,
                size: 200.0,
                data: serde_json::json!({"type" : panel_type}),
            };
            panels.insert(panel.id.clone(), panel);
        }

        PanelLayout {
            name: "development".to_string(),
            panels,
            tab_groups,
            splitter_positions: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn create_test_manager() -> PanelManager {
        let (sender, _) = mpsc::unbounded_channel();
        PanelManager::new(sender)
    }

    #[test]
    fn test_panel_manager_creation() {
        let manager = create_test_manager();

        assert_eq!(manager.current_layout, "default");
        assert!(manager.layouts.contains_key("default"));
        assert!(!manager.panels.is_empty());
    }

    #[test]
    fn test_add_remove_panel() {
        let mut manager = create_test_manager();

        let panel_id = manager
            .add_panel(
                PanelType::Custom {
                    plugin_id: "test".to_string(),
                },
                Some("Test Panel".to_string()),
            )
            .unwrap();

        assert!(manager.get_panel(&panel_id).is_some());
        assert_eq!(manager.get_panel(&panel_id).unwrap().title, "Test Panel");

        manager.remove_panel(&panel_id).unwrap();
        assert!(manager.get_panel(&panel_id).is_none());
    }

    #[test]
    fn test_panel_operations() {
        let mut manager = create_test_manager();

        let panel_id = manager.add_panel(PanelType::Terminal, None).unwrap();

        // Test toggle
        let original_visibility = manager.get_panel(&panel_id).unwrap().is_visible;
        manager.toggle_panel(&panel_id).unwrap();
        assert_eq!(
            manager.get_panel(&panel_id).unwrap().is_visible,
            !original_visibility
        );

        // Test move
        manager.move_panel(&panel_id, PanelPosition::Right).unwrap();
        assert_eq!(
            manager.get_panel(&panel_id).unwrap().position,
            PanelPosition::Right
        );

        // Test resize
        manager.resize_panel(&panel_id, 500.0).unwrap();
        assert_eq!(manager.get_panel(&panel_id).unwrap().size, 500.0);
    }

    #[test]
    fn test_layout_operations() {
        let mut manager = create_test_manager();

        // Save current layout
        manager.save_layout("test_layout".to_string()).unwrap();
        assert!(manager
            .get_available_layouts()
            .contains(&"test_layout".to_string()));

        // Switch layout
        manager.switch_layout("test_layout").unwrap();
        assert_eq!(manager.current_layout, "test_layout");

        // Test invalid layout
        assert!(manager.switch_layout("nonexistent").is_err());
    }

    #[test]
    fn test_layout_presets() {
        let minimal = LayoutPresets::minimal();
        assert_eq!(minimal.name, "minimal");
        assert_eq!(minimal.panels.len(), 2);

        let development = LayoutPresets::development();
        assert_eq!(development.name, "development");
        assert!(development.panels.len() > 2);
    }

    #[test]
    fn test_panel_types() {
        assert_eq!(PanelType::FileExplorer.default_title(), "Explorer");
        assert_eq!(
            PanelType::FileExplorer.default_position(),
            PanelPosition::Left
        );
        assert_eq!(
            PanelType::Terminal.default_position(),
            PanelPosition::Bottom
        );
    }
}
