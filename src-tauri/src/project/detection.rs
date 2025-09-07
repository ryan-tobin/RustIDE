use crate::project::{ProjectError, ProjectResult};
use crate::utils::paths::PathUtils;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, instrument, warn};

/// Types of Rust projects that can be detected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectType {
    /// Single binary project
    Binary,
    /// Library project
    Library,
    /// Mixed binary/library project
    Mixed,
    /// Cargo workspace with multiple packages
    Workspace,
    /// Workspace member package
    WorkspaceMember,
    /// Custom project type
    Custom,
}

impl ProjectType {
    /// Check if this project type represents a workspace
    pub fn is_workspace(&self) -> bool {
        matches!(self, ProjectType::Workspace)
    }

    /// Check if this project type can contain source code
    pub fn has_source(&self) -> bool {
        !matches!(self, ProjectType::Workspace)
    }

    /// Get the display name for this project type
    pub fn display_name(&self) -> &'static str {
        match self {
            ProjectType::Binary => "Binary",
            ProjectType::Library => "Library",
            ProjectType::Mixed => "Mixed",
            ProjectType::Workspace => "Workspace",
            ProjectType::WorkspaceMember => "Workspace Member",
            ProjectType::Custom => "Custom",
        }
    }
}

/// Configuration for project detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    /// Maximum depth to scan for projects
    pub max_scan_depth: usize,
    /// Patterns to exclude during scanning
    pub exclude_patterns: Vec<String>,
    /// Whether to follow symbolic links
    pub follow_symlinks: bool,
    /// Minimum confidence threshold for detection
    pub confidence_threshold: f32,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            max_scan_depth: 10,
            exclude_patterns: vec![
                "target".to_string(),
                ".git".to_string(),
                "node_modules".to_string(),
                ".cargo".to_string(),
                ".rustup".to_string(),
            ],
            follow_symlinks: false,
            confidence_threshold: 0.8,
        }
    }
}

/// Information about a detected project
#[derive(Debug, Clone, Serialize)]
pub struct DetectedProject {
    /// Path to the project root
    pub path: PathBuf,
    /// Detected project type
    pub project_type: ProjectType,
    /// Project name (from Cargo.toml)
    pub name: String,
    /// Detection confidence (0.0 - 1.0)
    pub confidence: f32,
    /// Additional metadata
    pub metadata: ProjectMetadata,
}

/// Additional metadata about a detected project
#[derive(Debug, Clone, Serialize)]
pub struct ProjectMetadata {
    /// Has Cargo.toml file
    pub has_manifest: bool,
    /// Has src/ directory
    pub has_src_dir: bool,
    /// Has main.rs file
    pub has_main: bool,
    /// Has lib.rs file
    pub has_lib: bool,
    /// Has examples/ directory
    pub has_examples: bool,
    /// Has tests/ directory
    pub has_tests: bool,
    /// Has benches/ directory
    pub has_benches: bool,
    /// Workspace members (if workspace)
    pub workspace_members: Vec<String>,
    /// Rust source file count
    pub rust_file_count: usize,
    /// Total file count
    pub total_file_count: usize,
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        Self {
            has_manifest: false,
            has_src_dir: false,
            has_main: false,
            has_lib: false,
            has_examples: false,
            has_tests: false,
            has_benches: false,
            workspace_members: Vec::new(),
            rust_file_count: 0,
            total_file_count: 0,
        }
    }
}

/// Project detector for automatically identifying Rust projects
pub struct ProjectDetector {
    config: DetectionConfig,
    path_utils: PathUtils,
}

impl ProjectDetector {
    /// Create a new project detector with default configuration
    pub fn new() -> Self {
        Self::with_config(DetectionConfig::default())
    }

    /// Create a new project detector with custom configuration
    pub fn with_config(config: DetectionConfig) -> Self {
        Self {
            config,
            path_utils: PathUtils::new(),
        }
    }

    /// Detect the project type for a given directory
    #[instrument(skip(self))]
    pub fn detect_project_type(&self, project_path: &Path) -> ProjectResult<ProjectType> {
        if !project_path.exists() {
            return Err(ProjectError::InvalidPath {
                path: project_path.to_string_lossy().to_string(),
            });
        }

        let manifest_path = project_path.join("Cargo.toml");
        if !manifest_path.exists() {
            return Err(ProjectError::ManifestNotFound {
                path: project_path.to_string_lossy().to_string(),
            });
        }

        let manifest_content =
            std::fs::read_to_string(&manifest_path).map_err(|e| ProjectError::FileSystemError {
                message: format!("Failed to read Cargo.toml: {}", e),
            })?;

        let manifest: toml::Value =
            toml::from_str(&manifest_content).map_err(|e| ProjectError::InvalidManifest {
                message: e.to_string(),
            })?;

        if manifest.get("workspace").is_some() {
            return Ok(ProjectType::Workspace);
        }

        let package = manifest.get("package");
        if package.is_none() {
            return Err(ProjectError::InvalidManifest {
                message: "No [package] section found".to_string(),
            });
        }

        let src_dir = project_path.join("src");
        if !src_dir.exists() {
            debug!("No src/ directory found, assuming custom project");
            return Ok(ProjectType::Custom);
        }

        let has_main = src_dir.join("main.rs").exists();
        let has_lib = src_dir.join("lib.rs").exists();

        let project_type = match (has_main, has_lib) {
            (true, true) => ProjectType::Mixed,
            (true, false) => ProjectType::Binary,
            (false, true) => ProjectType::Library,
            (false, false) => {
                if self.has_custom_binaries(project_path, &manifest) {
                    ProjectType::Binary
                } else {
                    ProjectType::Custom
                }
            }
        };

        debug!(
            "Detected project type: {} for {}",
            project_type.display_name(),
            project_path.display()
        );

        Ok(project_type)
    }

    /// Scan a directory recursively for Rust projects
    #[instrument(skip(self))]
    pub async fn scan_for_projects(&self, root_dir: &Path) -> Result<Vec<PathBuf>> {
        info!("Scanning for projects in: {}", root_dir.display());

        let mut projects = Vec::new();
        let mut visited = HashSet::new();

        self.scan_directory_recursive(root_dir, &mut projects, &mut visited, 0)
            .await?;

        info!(
            "Found {} projects in {}",
            projects.len(),
            root_dir.display()
        );
        Ok(projects)
    }

    /// Perform comprehensive project detection with metadata
    pub async fn detect_project_comprehensive(&self, path: &Path) -> Result<DetectedProject> {
        let project_type = self.detect_project_type(path)?;
        let metadata = self.gather_project_metadata(path).await?;

        let confidence = self.calculate_confidence(&metadata, project_type);

        let name = self.extract_project_name(path)?;

        Ok(DetectedProject {
            path: path.to_path_buf(),
            project_type,
            name,
            confidence,
            metadata,
        })
    }

    /// Validate that a path contains a valid Rust project
    pub fn validate_project(&self, path: &Path) -> ProjectResult<()> {
        if !path.exists() {
            return Err(ProjectError::InvalidPath {
                path: path.to_string_lossy().to_string(),
            });
        }

        if !path.is_dir() {
            return Err(ProjectError::InvalidPath {
                path: format!("{} is not a directory", path.display()),
            });
        }

        let manifest_path = path.join("Cargo.toml");
        if !manifest_path.exists() {
            return Err(ProjectError::ManifestNotFound {
                path: path.to_string_lossy().to_string(),
            });
        }

        let manifest_content =
            std::fs::read_to_string(&manifest_path).map_err(|e| ProjectError::FileSystemError {
                message: format!("Failed to read Cargo.toml: {}", e),
            })?;

        toml::from_str::<toml::Value>
            > (&manifest_content).map_err(|e| ProjectError::InvalidManifest {
                message: e.to_string(),
            })?;
    }

    /// Check if a directory should be excluded from scanning
    fn should_exclude_directory(&self, dir_name: &str) -> bool {
        self.config
            .exclude_patterns
            .iter()
            .any(|pattern| dir_name.contains(pattern))
    }

    /// Recursive directory scanning implementation
    async fn scan_directory_recursive(
        &self,
        dir: &Path,
        projects: &mut Vec<PathBuf>,
        visited: &mut HashSet<PathBuf>,
        depth: usize,
    ) -> Result<()> {
        if depth >= self.config.max_scan_depth {
            return Ok(());
        }

        let canonical_path = self.path_utils.normalize_path(dir)?;
        if visited.contains(&canonical_path) {
            return Ok(());
        }
        visited.insert(canonical_path);

        if self.is_rust_project(dir) {
            projects.push(dir.to_path_buf());

            if self.is_workspace(dir) {
                self.scan_workspace_members(dir, projects).await?;
            }

            if !self.is_workspace(dir) {
                return Ok(());
            }
        }

        let mut entries = match fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if self.should_exclude_directory(dir_name) {
                    continue;
                }

                if dir_name.starts_with(".") && dir_name != "." && dir_name != ".." {
                    continue;
                }

                self.scan_directory_recursive(&path, projects, visited, depth + 1)
                    .await?;
            }
        }

        Ok(())
    }

    /// Check if a directory contains a Rust project
    fn is_rust_project(&self, dir: &Path) -> bool {
        dir.join("Cargo.toml").exists()
    }

    /// Check if a directory contains a Cargo workspace
    fn is_workspace(&self, dir: &Path) -> bool {
        let manifest_path = dir.join("Cargo.toml");
        if !manifest_path.exists() {
            return false;
        }

        if let Ok(content) = std::fs::read_to_string(manifest_path) {
            if let Ok(manifest) = toml::from_str::<toml::Value>(&content) {
                return manifest.get("workspace").is_some();
            }
        }

        false
    }

    /// Scan workspace members for additional projects
    async fn scan_workspace_members(
        &self,
        workspace_dir: &Path,
        projects: &mut Vec<PathBuf>,
    ) -> Result<()> {
        let manifest_path = workspace_dir.join("Cargo.toml");
        let content = fs::read_to_string(manifest_path).await?;
        let manifest: toml::Value = toml::from_str(&content)?;

        if let Some(workspace) = manifet.get("workspace") {
            if let Some(members) = workspace.get("members") {
                if let Some(members_array) = members.as_array() {
                    for member in members_array {
                        if let Some(member_path) = member.as_str() {
                            let full_path = workspace_dir.join(member_path);
                            if full_path.exists() && self.is_rust_project(&full_path) {
                                projects.push(full_path);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if the project has custom bianry configurations
    fn has_custom_binaries(&self, project_path: &Path, manifest: &toml::Value) -> bool {
        if project_path.join("bin").exists() {
            return true;
        }

        if let Some(bins) = manifest.get("bin") {
            if bins.is_array() && !bins.as_array().unwrap().is_empty() {
                return true;
            }
        }

        false
    }

    /// Gather comprehensive metadata about a project
    async fn gather_project_metadata(&self, path: &Path) -> Result<ProjectMetadata> {
        let mut metadata = ProjectMetadata::default();

        metadata.has_manifest = path.join("Cargo.toml").exists();
        metadata.has_src_dir = path.join("src").exists();
        metadata.has_main = path.join("src").join("main.rs").exists();
        metadata.has_lib = path.join("src").join("lib.rs").exists();
        metadata.has_examples = path.join("examples").exists();
        metadata.has_tests = path.join("tests").exists();
        metadata.has_benches = path.join("benches").exists();

        let (rust_count, total_count) = self.count_files(path).await?;
        metadata.rust_file_count = rust_count;
        metadata.total_file_count = total_count;

        if self.is_workspace(path) {
            metadata.workspace_members = self.get_workspace_member_names(path).await?;
        }

        Ok(metadata)
    }

    /// Count Rust and total files in a project
    async fn count_files(&self, path: &Path) -> Result<(usize, usize)> {
        let mut rust_count = 0;
        let mut total_count = 0;

        self.count_files_recursive(path, &mut rust_count, &mut total_count, 0)
            .await?;

        Ok((rust_count, total_count))
    }

    /// Recursive file counting
    async fn count_files_recursive(
        &self,
        dir: &Path,
        rust_count: &mut usize,
        total_count: &mut usize,
        depth: usize,
    ) -> Result<()> {
        if depth >= 5 {
            return Ok(());
        }

        let mut entries = fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_file() {
                *total_count += 1;
                if let Some(ext) = path.extension() {
                    if ext == "rs" {
                        *rust_count += 1;
                    }
                }
            } else if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if !self.should_exclude_directory(dir_name) && !dir_name.starts_with('.') {
                    self.count_files_recursive(&path, rust_count, total_count, depth + 1)
                        .await?;
                }
            }
        }

        Ok(())
    }

    /// Get workspace member names
    async fn get_workspace_member_names(&self, workspace_path: &Path) -> Result<Vec<String>> {
        let manifest_path = workspace_path.join("Cargo.toml");
        let content = fs::read_to_string(manifest_path).await?;
        let manifest: toml::Value = toml::from_str(&content)?;

        let mut members = Vec::new();

        if let Some(workspace) = manifest.get("workspace") {
            if let Some(members_value) = workspace.get("members") {
                if let Some(members_array) = members_value.as_array() {
                    for member in members_array {
                        if let Some(member_str) = member.as_str() {
                            members.push(member_str.to_string());
                        }
                    }
                }
            }
        }

        Ok(members)
    }

    /// Calculate detection confidence based on metadata
    fn calculate_confidence(&self, metadata: &ProjectMetadata, project_type: ProjectType) -> f32 {
        let mut confidence = 0.0;

        if metadata.has_manifest {
            confidence += 0.3;
        }

        if metadata.has_src_dir {
            confidence += 0.2;
        }

        match project_type {
            ProjectType::Binary => {
                if metadata.has_main {
                    confidence += 0.3;
                }
            }
            ProjectType::Library => {
                if metadata.has_lib {
                    confidence += 0.3;
                }
            }
            ProjectType::Mixed => {
                if metadata.has_main && metadata.has_lib {
                    confidence += 0.3;
                }
            }
            ProjectType::Workspace => {
                if !metadata.workspace_members.is_empty() {
                    confidence += 0.3;
                }
            }
            _ => {}
        }

        if metadata.rust_file_count > 0 {
            confidence += 0.1;
        }

        if metadata.has_examples {
            confidence += 0.05;
        }

        if metadata.has_tests {
            confidence += 0.05;
        }

        confidence.min(1.0)
    }

    /// Extract project name from Cargo.toml
    fn extract_project_name(&self, path: &Path) -> ProjectResult<String> {
        let manifest_path = path.join("Cargo.toml");
        let content =
            std::fs::read_to_string(manifest_path).map_err(|e| ProjectError::FileSystemError {
                message: e.to_string(),
            })?;

        let manifest: toml::Value =
            toml::from_str(&content).map_err(|e| ProjectError::InvalidManifest {
                message: e.to_string(),
            })?;

        // For workspaces, use the directory name
        if manifest.get("workspace").is_some() {
            return Ok(path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("workspace")
                .to_string());
        }

        // For packages, get name from package section
        if let Some(package) = manifest.get("package") {
            if let Some(name) = package.get("name") {
                if let Some(name_str) = name.as_str() {
                    return Ok(name_str.to_string());
                }
            }
        }

        Err(ProjectError::InvalidManifest {
            message: "No package name found".to_string(),
        })
    }

    /// Get configuration
    pub fn config(&self) -> &DetectionConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: DetectionConfig) {
        self.config = config;
    }
}

impl Default for ProjectDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Utility functions for project detection
pub mod utils {
    use super::*;

    /// Check if a path looks like a Rust project based on heuristics
    pub fn looks_like_rust_project(path: &Path) -> bool {
        // Must have Cargo.toml
        if !path.join("Cargo.toml").exists() {
            return false;
        }

        // Should have src/ directory or be a workspace
        let has_src = path.join("src").exists();
        let has_workspace_marker = path.join("Cargo.toml").exists() && {
            if let Ok(content) = std::fs::read_to_string(path.join("Cargo.toml")) {
                content.contains("[workspace]")
            } else {
                false
            }
        };

        has_src || has_workspace_marker
    }

    /// Get the most likely project root from a given path
    pub fn find_project_root(start_path: &Path) -> Option<PathBuf> {
        let mut current = start_path;

        loop {
            if looks_like_rust_project(current) {
                return Some(current.to_path_buf());
            }

            if let Some(parent) = current.parent() {
                current = parent;
            } else {
                break;
            }
        }

        None
    }

    /// Check if a directory name indicates it should be ignored
    pub fn should_ignore_directory(name: &str) -> bool {
        matches!(
            name,
            "target" | ".git" | "node_modules" | ".cargo" | ".rustup" | ".vscode" | ".idea"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    async fn create_test_binary_project(dir: &Path) -> Result<()> {
        let cargo_toml = r#"
[package]
name = "test-binary"
version = "0.1.0"
edition = "2021"
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

    async fn create_test_library_project(dir: &Path) -> Result<()> {
        let cargo_toml = r#"
[package]
name = "test-library"
version = "0.1.0"
edition = "2021"
"#;

        let lib_rs = r#"
pub fn hello() -> String {
    "Hello, world!".to_string()
}
"#;

        fs::write(dir.join("Cargo.toml"), cargo_toml).await?;
        fs::create_dir_all(dir.join("src")).await?;
        fs::write(dir.join("src").join("lib.rs"), lib_rs).await?;

        Ok(())
    }

    async fn create_test_workspace(dir: &Path) -> Result<()> {
        let cargo_toml = r#"
[workspace]
members = ["app", "lib"]
"#;

        fs::write(dir.join("Cargo.toml"), cargo_toml).await?;

        // Create app member
        let app_dir = dir.join("app");
        fs::create_dir_all(&app_dir).await?;
        create_test_binary_project(&app_dir).await?;

        // Create lib member
        let lib_dir = dir.join("lib");
        fs::create_dir_all(&lib_dir).await?;
        create_test_library_project(&lib_dir).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_detect_binary_project() {
        let temp_dir = TempDir::new().unwrap();
        create_test_binary_project(temp_dir.path()).await.unwrap();

        let detector = ProjectDetector::new();
        let project_type = detector.detect_project_type(temp_dir.path()).unwrap();

        assert_eq!(project_type, ProjectType::Binary);
    }

    #[tokio::test]
    async fn test_detect_library_project() {
        let temp_dir = TempDir::new().unwrap();
        create_test_library_project(temp_dir.path()).await.unwrap();

        let detector = ProjectDetector::new();
        let project_type = detector.detect_project_type(temp_dir.path()).unwrap();

        assert_eq!(project_type, ProjectType::Library);
    }

    #[tokio::test]
    async fn test_detect_workspace() {
        let temp_dir = TempDir::new().unwrap();
        create_test_workspace(temp_dir.path()).await.unwrap();

        let detector = ProjectDetector::new();
        let project_type = detector.detect_project_type(temp_dir.path()).unwrap();

        assert_eq!(project_type, ProjectType::Workspace);
    }

    #[tokio::test]
    async fn test_scan_for_projects() {
        let temp_dir = TempDir::new().unwrap();

        // Create multiple projects
        let project1 = temp_dir.path().join("project1");
        fs::create_dir_all(&project1).await.unwrap();
        create_test_binary_project(&project1).await.unwrap();

        let project2 = temp_dir.path().join("project2");
        fs::create_dir_all(&project2).await.unwrap();
        create_test_library_project(&project2).await.unwrap();

        let detector = ProjectDetector::new();
        let projects = detector.scan_for_projects(temp_dir.path()).await.unwrap();

        assert_eq!(projects.len(), 2);
        assert!(projects.contains(&project1));
        assert!(projects.contains(&project2));
    }

    #[tokio::test]
    async fn test_comprehensive_detection() {
        let temp_dir = TempDir::new().unwrap();
        create_test_binary_project(temp_dir.path()).await.unwrap();

        let detector = ProjectDetector::new();
        let detected = detector
            .detect_project_comprehensive(temp_dir.path())
            .await
            .unwrap();

        assert_eq!(detected.project_type, ProjectType::Binary);
        assert_eq!(detected.name, "test-binary");
        assert!(detected.confidence > 0.8);
        assert!(detected.metadata.has_manifest);
        assert!(detected.metadata.has_src_dir);
        assert!(detected.metadata.has_main);
    }

    #[tokio::test]
    async fn test_workspace_member_scanning() {
        let temp_dir = TempDir::new().unwrap();
        create_test_workspace(temp_dir.path()).await.unwrap();

        let detector = ProjectDetector::new();
        let projects = detector.scan_for_projects(temp_dir.path()).await.unwrap();

        // Should find workspace + 2 members = 3 projects
        assert_eq!(projects.len(), 3);
    }

    #[test]
    fn test_project_validation() {
        let temp_dir = TempDir::new().unwrap();
        let detector = ProjectDetector::new();

        // Invalid path
        let invalid_path = temp_dir.path().join("nonexistent");
        assert!(detector.validate_project(&invalid_path).is_err());

        // Valid path will be tested with async setup
    }

    #[test]
    fn test_utils_functions() {
        let temp_dir = TempDir::new().unwrap();

        // Test looks_like_rust_project
        assert!(!utils::looks_like_rust_project(temp_dir.path()));

        // Test should_ignore_directory
        assert!(utils::should_ignore_directory("target"));
        assert!(utils::should_ignore_directory(".git"));
        assert!(!utils::should_ignore_directory("src"));
    }
}
