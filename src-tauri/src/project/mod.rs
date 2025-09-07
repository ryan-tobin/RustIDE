// src-tauri/src/project/mod.rs
//! Project management system for RustIDE
//!
//! This module provides comprehensive project management capabilities including:
//! - Auto-detection of Rust projects and workspaces
//! - Cargo.toml parsing and manifest management
//! - Workspace handling with multiple packages
//! - File tree management with intelligent filtering
//! - Build system integration with cargo commands
//! - Project templates for creating new projects

use crate::utils::{config::ConfigManager, file_watcher::FileWatcher, paths::PathUtils};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub mod build;
pub mod detection;
pub mod file_tree;
pub mod manifest;
pub mod templates;
pub mod workspace;

// Re-export main types for easier access
pub use build::{BuildConfig, BuildManager, BuildOutput, BuildStatus, BuildTarget};
pub use detection::{ProjectDetector, ProjectType};
pub use file_tree::{FileNode, FileTree, FileTreeFilter, TreeUpdateEvent};
pub use manifest::{CargoManifest, Dependency, ManifestParser, PackageMetadata};
pub use templates::{ProjectTemplate, TemplateEngine, TemplateType};
pub use workspace::{WorkspaceManager, WorkspaceMember, WorkspaceMetadata};

/// Global project manager state
pub type ProjectMap = Arc<RwLock<HashMap<Uuid, Project>>>;

/// Errors that can occur in project operations
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("Project not found: {id}")]
    ProjectNotFound { id: String },

    #[error("Invalid project path: {path}")]
    InvalidPath { path: String },

    #[error("Cargo.toml not found in: {path}")]
    ManifestNotFound { path: String },

    #[error("Invalid Cargo.toml format: {message}")]
    InvalidManifest { message: String },

    #[error("Workspace error: {message}")]
    WorkspaceError { message: String },

    #[error("Build error: {message}")]
    BuildError { message: String },

    #[error("File system error: {message}")]
    FileSystemError { message: String },

    #[error("Template error: {message}")]
    TemplateError { message: String },

    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },

    #[error("Parse error: {source}")]
    ParseError {
        #[from]
        source: toml::de::Error,
    },

    #[error("Process error: {message}")]
    ProcessError { message: String },
}

pub type ProjectResult<T> = Result<T, ProjectError>;

/// Configuration for project management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Auto-detect projects when opening folders
    pub auto_detect: bool,
    /// Watch for file changes in projects
    pub watch_files: bool,
    /// Exclude patterns for file watching
    pub watch_exclude_patterns: Vec<String>,
    /// Cargo executable path
    pub cargo_path: String,
    /// Rust toolchain to use
    pub toolchain: Option<String>,
    /// Build configuration
    pub build_config: BuildConfig,
    /// File tree configuration
    pub file_tree_config: FileTreeConfig,
    /// Template settings
    pub template_config: TemplateConfig,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            auto_detect: true,
            watch_files: true,
            watch_exclude_patterns: vec![
                "target/".to_string(),
                ".git/".to_string(),
                "node_modules/".to_string(),
                "*.tmp".to_string(),
                "*.lock".to_string(),
            ],
            cargo_path: "cargo".to_string(),
            toolchain: None,
            build_config: BuildConfig::default(),
            file_tree_config: FileTreeConfig::default(),
            template_config: TemplateConfig::default(),
        }
    }
}

/// File tree configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTreeConfig {
    /// Maximum depth to scan
    pub max_depth: usize,
    /// Show hidden files
    pub show_hidden: bool,
    /// Ignore patterns
    pub ignore_patterns: Vec<String>,
    /// Auto-expand certain directories
    pub auto_expand_patterns: Vec<String>,
}

impl Default for FileTreeConfig {
    fn default() -> Self {
        Self {
            max_depth: 50,
            show_hidden: false,
            ignore_patterns: vec![
                "target".to_string(),
                ".git".to_string(),
                "node_modules".to_string(),
                ".DS_Store".to_string(),
                "Thumbs.db".to_string(),
            ],
            auto_expand_patterns: vec!["src".to_string(), "examples".to_string()],
        }
    }
}

/// Template configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    /// Templates directory path
    pub templates_dir: Option<PathBuf>,
    /// Default author name
    pub default_author: Option<String>,
    /// Default license
    pub default_license: Option<String>,
    /// Git integration
    pub init_git: bool,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            templates_dir: None,
            default_author: None,
            default_license: Some("MIT OR Apache-2.0".to_string()),
            init_git: true,
        }
    }
}

/// Main project structure
#[derive(Debug, Clone)]
pub struct Project {
    /// Unique identifier
    pub id: Uuid,
    /// Project name
    pub name: String,
    /// Root path of the project
    pub root_path: PathBuf,
    /// Project type
    pub project_type: ProjectType,
    /// Cargo manifest
    pub manifest: CargoManifest,
    /// Workspace information (if applicable)
    pub workspace: Option<WorkspaceMetadata>,
    /// File tree
    pub file_tree: FileTree,
    /// Build manager
    pub build_manager: BuildManager,
    /// Project configuration
    pub config: ProjectConfig,
    /// File watcher (if watching is enabled)
    pub file_watcher: Option<FileWatcher>,
    /// Last modified timestamp
    pub last_modified: std::time::SystemTime,
}

impl Project {
    /// Create a new project from a root path
    pub async fn from_path(root_path: PathBuf, config: ProjectConfig) -> ProjectResult<Self> {
        info!("Creating project from path: {}", root_path.display());

        // Validate path exists
        if !root_path.exists() {
            return Err(ProjectError::InvalidPath {
                path: root_path.to_string_lossy().to_string(),
            });
        }

        // Detect project type
        let detector = ProjectDetector::new();
        let project_type = detector.detect_project_type(&root_path)?;

        // Parse manifest
        let manifest_parser = ManifestParser::new();
        let manifest = manifest_parser.parse_manifest(&root_path).await?;

        // Initialize workspace if applicable
        let workspace = if project_type.is_workspace() {
            let workspace_manager = WorkspaceManager::new();
            Some(workspace_manager.load_workspace(&root_path).await?)
        } else {
            None
        };

        // Create file tree
        let mut file_tree = FileTree::new(root_path.clone(), config.file_tree_config.clone());
        file_tree.scan().await?;

        // Initialize build manager
        let build_manager = BuildManager::new(root_path.clone(), config.build_config.clone());

        // Get project name
        let name = manifest.package.name.clone();

        // Setup file watcher if enabled
        let file_watcher = if config.watch_files {
            let mut watcher = FileWatcher::new();
            watcher.watch_directory(&root_path, config.watch_exclude_patterns.clone())?;
            Some(watcher)
        } else {
            None
        };

        let project = Self {
            id: Uuid::new_v4(),
            name,
            root_path,
            project_type,
            manifest,
            workspace,
            file_tree,
            build_manager,
            config,
            file_watcher,
            last_modified: std::time::SystemTime::now(),
        };

        info!("Successfully created project: {}", project.name);
        Ok(project)
    }

    /// Get the project's main source directory
    pub fn src_dir(&self) -> PathBuf {
        self.root_path.join("src")
    }

    /// Get the project's target directory
    pub fn target_dir(&self) -> PathBuf {
        self.root_path.join("target")
    }

    /// Get all source files in the project
    pub fn source_files(&self) -> Vec<PathBuf> {
        self.file_tree
            .all_files()
            .into_iter()
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext == "rs")
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Check if this is a workspace project
    pub fn is_workspace(&self) -> bool {
        self.workspace.is_some()
    }

    /// Get workspace members (if this is a workspace)
    pub fn workspace_members(&self) -> Option<&Vec<WorkspaceMember>> {
        self.workspace.as_ref().map(|ws| &ws.members)
    }

    /// Update the project's file tree
    pub async fn refresh_file_tree(&mut self) -> ProjectResult<()> {
        self.file_tree
            .scan()
            .await
            .map_err(|e| ProjectError::FileSystemError {
                message: e.to_string(),
            })?;
        self.last_modified = std::time::SystemTime::now();
        Ok(())
    }

    /// Update the project's manifest
    pub async fn refresh_manifest(&mut self) -> ProjectResult<()> {
        let manifest_parser = ManifestParser::new();
        self.manifest = manifest_parser.parse_manifest(&self.root_path).await?;
        self.last_modified = std::time::SystemTime::now();
        Ok(())
    }

    /// Get project statistics
    pub fn statistics(&self) -> ProjectStatistics {
        let source_files = self.source_files();
        let total_files = self.file_tree.file_count();

        ProjectStatistics {
            total_files,
            source_files: source_files.len(),
            directories: self.file_tree.directory_count(),
            dependencies: self.manifest.dependencies.len(),
            dev_dependencies: self.manifest.dev_dependencies.len(),
            workspace_members: self.workspace_members().map(|m| m.len()).unwrap_or(0),
            last_build: self.build_manager.last_build_time(),
        }
    }

    /// Check if the project needs refresh
    pub fn needs_refresh(&self) -> bool {
        if let Some(watcher) = &self.file_watcher {
            watcher.has_pending_changes()
        } else {
            false
        }
    }
}

/// Project statistics for display
#[derive(Debug, Clone, Serialize)]
pub struct ProjectStatistics {
    pub total_files: usize,
    pub source_files: usize,
    pub directories: usize,
    pub dependencies: usize,
    pub dev_dependencies: usize,
    pub workspace_members: usize,
    pub last_build: Option<std::time::SystemTime>,
}

/// Project manager for handling multiple projects
pub struct ProjectManager {
    /// All open projects
    projects: Arc<RwLock<HashMap<Uuid, Project>>>,
    /// Configuration
    config: ProjectConfig,
    /// Template engine
    template_engine: TemplateEngine,
    /// Project detector
    detector: ProjectDetector,
}

impl ProjectManager {
    /// Create a new project manager
    pub fn new(config: ProjectConfig) -> Self {
        Self {
            projects: Arc::new(RwLock::new(HashMap::new())),
            template_engine: TemplateEngine::new(config.template_config.clone()),
            detector: ProjectDetector::new(),
            config,
        }
    }

    /// Open a project from a path
    pub async fn open_project(&self, path: PathBuf) -> ProjectResult<Uuid> {
        info!("Opening project at: {}", path.display());

        // Check if project is already open
        {
            let projects = self.projects.read().await;
            for project in projects.values() {
                if project.root_path == path {
                    info!("Project already open: {}", project.name);
                    return Ok(project.id);
                }
            }
        }

        // Create new project
        let project = Project::from_path(path, self.config.clone()).await?;
        let project_id = project.id;

        // Add to projects map
        {
            let mut projects = self.projects.write().await;
            projects.insert(project_id, project);
        }

        info!("Successfully opened project with ID: {}", project_id);
        Ok(project_id)
    }

    /// Close a project
    pub async fn close_project(&self, project_id: Uuid) -> ProjectResult<()> {
        let mut projects = self.projects.write().await;
        if let Some(project) = projects.remove(&project_id) {
            info!("Closed project: {}", project.name);
            Ok(())
        } else {
            Err(ProjectError::ProjectNotFound {
                id: project_id.to_string(),
            })
        }
    }

    /// Get a project by ID
    pub async fn get_project(&self, project_id: Uuid) -> ProjectResult<Project> {
        let projects = self.projects.read().await;
        projects
            .get(&project_id)
            .cloned()
            .ok_or(ProjectError::ProjectNotFound {
                id: project_id.to_string(),
            })
    }

    /// Get all open projects
    pub async fn list_projects(&self) -> Vec<Project> {
        let projects = self.projects.read().await;
        projects.values().cloned().collect()
    }

    /// Create a new project from template
    pub async fn create_project(
        &self,
        template_type: TemplateType,
        name: String,
        path: PathBuf,
        options: HashMap<String, String>,
    ) -> ProjectResult<Uuid> {
        info!("Creating new project: {} at {}", name, path.display());

        // Create project from template
        self.template_engine
            .create_project(template_type, &name, &path, options)
            .await
            .map_err(|e| ProjectError::TemplateError {
                message: e.to_string(),
            })?;

        // Open the created project
        let project_path = path.join(&name);
        self.open_project(project_path).await
    }

    /// Auto-detect projects in a directory
    pub async fn detect_projects(&self, root_dir: &Path) -> ProjectResult<Vec<PathBuf>> {
        self.detector
            .scan_for_projects(root_dir)
            .await
            .map_err(|e| ProjectError::FileSystemError {
                message: e.to_string(),
            })
    }

    /// Get the shared projects map for use in Tauri commands
    pub fn projects_map(&self) -> Arc<RwLock<HashMap<Uuid, Project>>> {
        self.projects.clone()
    }

    /// Update configuration
    pub fn update_config(&mut self, config: ProjectConfig) {
        self.config = config;
    }

    /// Refresh all projects
    pub async fn refresh_all_projects(&self) -> ProjectResult<()> {
        let mut projects = self.projects.write().await;
        for project in projects.values_mut() {
            if project.needs_refresh() {
                project.refresh_file_tree().await?;
                project.refresh_manifest().await?;
            }
        }
        Ok(())
    }
}

/// Initialize the project management system
pub fn init_project_system(config: ProjectConfig) -> ProjectManager {
    info!("Initializing project management system");
    ProjectManager::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    async fn create_test_project(dir: &Path) -> Result<()> {
        let cargo_toml = r#"
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1.0"
        "#;

        let main_rs = r#"
fn main() {
    println!("Hello, world!");
}
        "#;

        fs::write(dir.join("Cargo.toml"), cargo_toml).await?;
        fs::create_dir_all(dir.join("src")).await?;
        fs::write(dir.join("src").join("main.rs"), main_rs).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_project_creation() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = ProjectConfig::default();
        let project = Project::from_path(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        assert_eq!(project.name, "test-project");
        assert_eq!(project.project_type, ProjectType::Binary);
        assert!(!project.is_workspace());
    }

    #[tokio::test]
    async fn test_project_manager() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = ProjectConfig::default();
        let manager = ProjectManager::new(config);

        let project_id = manager
            .open_project(temp_dir.path().to_path_buf())
            .await
            .unwrap();

        let project = manager.get_project(project_id).await.unwrap();
        assert_eq!(project.name, "test-project");

        let projects = manager.list_projects().await;
        assert_eq!(projects.len(), 1);

        manager.close_project(project_id).await.unwrap();
        let projects = manager.list_projects().await;
        assert_eq!(projects.len(), 0);
    }

    #[tokio::test]
    async fn test_project_statistics() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = ProjectConfig::default();
        let project = Project::from_path(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let stats = project.statistics();
        assert!(stats.source_files > 0);
        assert!(stats.total_files > 0);
        assert_eq!(stats.dependencies, 1); // serde
    }
}
