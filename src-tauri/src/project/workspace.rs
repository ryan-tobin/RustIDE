use crate::project::{manifest::ManifestParser, ProjectError, ProjectResult, ProjectType};
use crate::utils::paths::PathUtils;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, instrument, warn};

/// Workspace metadata containing all workspace information
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceMetadata {
    /// Workspace root path
    pub root_path: PathBuf,
    /// Workspace members
    pub members: Vec<WorkspaceMember>,
    /// Excluded paths
    pub excluded: Vec<String>,
    /// Default members
    pub default_members: Vec<String>,
    /// Workspace-level dependencies
    pub workspace_dependencies: HashMap<String, crate::project::manifest::Dependency>,
    /// Dependency graph between members
    pub dependency_graph: DependencyGraph,
    /// Resolver version
    pub resolver: Option<String>,
    /// Custom metadata
    pub metadata: Option<toml::Value>,
}

/// Information about a workspace member package
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceMember {
    /// Member name
    pub name: String,
    /// Relative path from workspace root
    pub path: PathBuf,
    /// Absolute path
    pub absolute_path: PathBuf,
    /// Project type
    pub project_type: ProjectType,
    /// Package version
    pub version: String,
    /// Dependencies on other workspace members
    pub workspace_dependencies: Vec<String>,
    /// External dependencies
    pub external_dependencies: Vec<String>,
    /// Features defined in this member
    pub features: Vec<String>,
    /// Whether this is a default member
    pub is_default_member: bool,
    /// Manifest metadata
    pub manifest: crate::project::manifest::CargoManifest,
}

/// Dependency graph for workspace members
#[derive(Debug, Clone, Serialize)]
pub struct DependencyGraph {
    /// Nodes in the graph (member names)
    pub nodes: Vec<String>,
    /// Edges representing dependencies (from -> to)
    pub edges: Vec<DependencyEdge>,
    /// Topologically sorted members (dependency order)
    pub build_order: Vec<String>,
    /// Circular dependencies (if any)
    pub cycles: Vec<Vec<String>>,
}

/// Edge in the dependency graph
#[derive(Debug, Clone, Serialize)]
pub struct DependencyEdge {
    /// Source member
    pub from: String,
    /// Target member
    pub to: String,
    /// Dependency type
    pub dependency_type: DependencyType,
    /// Features being used
    pub features: Vec<String>,
}

/// Type of dependency between workspace members
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DependencyType {
    /// Regular runtime dependency
    Normal,
    /// Development dependency
    Dev,
    /// Build dependency
    Build,
    /// Target-specific dependency
    Target,
}

/// Configuration for workspace operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Whether to auto-detect workspace members
    pub auto_detect_members: bool,
    /// Maximum depth for member scanning
    pub max_scan_depth: usize,
    /// Patterns to exclude when scanning
    pub exclude_patterns: Vec<String>,
    /// Whether to validate member dependencies
    pub validate_dependencies: bool,
    /// Whether to check for circular dependencies
    pub check_cycles: bool,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            auto_detect_members: true,
            max_scan_depth: 5,
            exclude_patterns: vec![
                "target".to_string(),
                ".git".to_string(),
                "node_modules".to_string(),
            ],
            validate_dependencies: true,
            check_cycles: true,
        }
    }
}

/// Workspace manager for handling Cargo workspaces
pub struct WorkspaceManager {
    /// Configuration
    config: WorkspaceConfig,
    /// Manifest parser
    manifest_parser: ManifestParser,
    /// Path utilities
    path_utils: PathUtils,
}

impl WorkspaceManager {
    /// Create a new workspace manager
    pub fn new() -> Self {
        Self::with_config(WorkspaceConfig::default())
    }

    /// Create a new workspace manager with custom configuration
    pub fn with_config(config: WorkspaceConfig) -> Self {
        Self {
            config,
            manifest_parser: ManifestParser::new(),
            path_utils: PathUtils::new(),
        }
    }

    /// Load workspace metadata from a workspace root
    #[instrument(skip(self))]
    pub async fn load_workspace(&self, workspace_root: &Path) -> ProjectResult<WorkspaceMetadata> {
        info!("Loading workspace from: {}", workspace_root.display());

        // Parse the root manifest
        let root_manifest = self.manifest_parser.parse_manifest(workspace_root).await?;

        // Verify this is actually a workspace
        let workspace_config = root_manifest.workspace.ok_or_else(|| {
            ProjectError::WorkspaceError {
                message: "No [workspace] section found in Cargo.toml".to_string(),
            }
        })?;

        // Load all workspace members
        let members = self.load_workspace_members(workspace_root, &workspace_config).await?;

        // Build dependency graph
        let dependency_graph = self.build_dependency_graph(&members)?;

        // Validate the workspace if configured
        if self.config.validate_dependencies {
            self.validate_workspace(&members, &dependency_graph)?;
        }

        let metadata = WorkspaceMetadata {
            root_path: workspace_root.to_path_buf(),
            members,
            excluded: workspace_config.exclude,
            default_members: workspace_config.default_members,
            workspace_dependencies: workspace_config.dependencies,
            dependency_graph,
            resolver: workspace_config.resolver,
            metadata: workspace_config.metadata,
        };

        info!(
            "Loaded workspace with {} members",
            metadata.members.len()
        );

        Ok(metadata)
    }

    /// Load all workspace members
    async fn load_workspace_members(
        &self,
        workspace_root: &Path,
        workspace_config: &crate::project::manifest::WorkspaceConfig,
    ) -> ProjectResult<Vec<WorkspaceMember>> {
        let mut members = Vec::new();
        let mut processed_paths = HashSet::new();

        // Load explicitly listed members
        for member_path in &workspace_config.members {
            let member_dirs = self.resolve_member_pattern(workspace_root, member_path).await?;
            
            for member_dir in member_dirs {
                if processed_paths.contains(&member_dir) {
                    continue;
                }

                // Skip excluded paths
                if self.is_excluded_path(&member_dir, workspace_root, &workspace_config.exclude) {
                    continue;
                }

                if let Ok(member) = self.load_workspace_member(workspace_root, &member_dir, workspace_config).await {
                    members.push(member);
                    processed_paths.insert(member_dir);
                }
            }
        }

        // Auto-detect additional members if configured
        if self.config.auto_detect_members {
            let auto_detected = self.auto_detect_members(workspace_root, &processed_paths).await?;
            for member in auto_detected {
                if !processed_paths.contains(&member.absolute_path) {
                    members.push(member);
                }
            }
        }

        Ok(members)
    }

    /// Load a single workspace member
    async fn load_workspace_member(
        &self,
        workspace_root: &Path,
        member_path: &Path,
        workspace_config: &crate::project::manifest::WorkspaceConfig,
    ) -> ProjectResult<WorkspaceMember> {
        // Parse member manifest
        let manifest = self.manifest_parser.parse_manifest(member_path).await?;

        // Determine project type
        let project_type = self.determine_member_project_type(member_path)?;

        // Calculate relative path
        let relative_path = self.path_utils.get_relative_path(workspace_root, member_path)?;

        // Analyze dependencies
        let (workspace_deps, external_deps) = self.analyze_member_dependencies(&manifest, workspace_config);

        // Extract features
        let features: Vec<String> = manifest.features.keys().cloned().collect();

        // Check if it's a default member
        let is_default_member = workspace_config.default_members.is_empty() ||
            workspace_config.default_members.iter().any(|default_path| {
                let default_full_path = workspace_root.join(default_path);
                default_full_path == member_path
            });

        Ok(WorkspaceMember {
            name: manifest.package.name.clone(),
            path: relative_path,
            absolute_path: member_path.to_path_buf(),
            project_type,
            version: manifest.package.version.clone(),
            workspace_dependencies: workspace_deps,
            external_dependencies: external_deps,
            features,
            is_default_member,
            manifest,
        })
    }

    /// Resolve glob patterns in member paths
    async fn resolve_member_pattern(
        &self,
        workspace_root: &Path,
        pattern: &str,
    ) -> ProjectResult<Vec<PathBuf>> {
        let mut resolved_paths = Vec::new();

        // Handle simple patterns (no glob)
        if !pattern.contains('*') && !pattern.contains('?') {
            let path = workspace_root.join(pattern);
            if path.exists() && path.is_dir() {
                resolved_paths.push(path);
            }
            return Ok(resolved_paths);
        }

        // Handle glob patterns
        let pattern_path = workspace_root.join(pattern);
        let pattern_str = pattern_path.to_string_lossy();

        // Simple glob implementation for basic patterns
        if pattern.ends_with("/*") {
            let base_path = workspace_root.join(&pattern[..pattern.len() - 2]);
            if base_path.exists() && base_path.is_dir() {
                let mut entries = fs::read_dir(&base_path).await.map_err(|e| {
                    ProjectError::FileSystemError {
                        message: format!("Failed to read directory {}: {}", base_path.display(), e),
                    }
                })?;

                while let Some(entry) = entries.next_entry().await.map_err(|e| {
                    ProjectError::FileSystemError {
                        message: e.to_string(),
                    }
                })? {
                    let path = entry.path();
                    if path.is_dir() && path.join("Cargo.toml").exists() {
                        resolved_paths.push(path);
                    }
                }
            }
        }

        Ok(resolved_paths)
    }

    /// Check if a path should be excluded
    fn is_excluded_path(
        &self,
        member_path: &Path,
        workspace_root: &Path,
        excludes: &[String],
    ) -> bool {
        if let Ok(relative_path) = self.path_utils.get_relative_path(workspace_root, member_path) {
            let relative_str = relative_path.to_string_lossy();
            
            for exclude_pattern in excludes {
                if relative_str.starts_with(exclude_pattern) {
                    return true;
                }
            }
        }

        false
    }

    /// Auto-detect workspace members by scanning directories
    async fn auto_detect_members(
        &self,
        workspace_root: &Path,
        already_processed: &HashSet<PathBuf>,
    ) -> ProjectResult<Vec<WorkspaceMember>> {
        let mut members = Vec::new();

        self.scan_for_members_recursive(
            workspace_root,
            workspace_root,
            &mut members,
            already_processed,
            0,
        ).await?;

        Ok(members)
    }

    /// Recursively scan for workspace members
    async fn scan_for_members_recursive(
        &self,
        workspace_root: &Path,
        current_dir: &Path,
        members: &mut Vec<WorkspaceMember>,
        already_processed: &HashSet<PathBuf>,
        depth: usize,
    ) -> ProjectResult<()> {
        if depth >= self.config.max_scan_depth {
            return Ok(());
        }

        let mut entries = fs::read_dir(current_dir).await.map_err(|e| {
            ProjectError::FileSystemError {
                message: e.to_string(),
            }
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            ProjectError::FileSystemError {
                message: e.to_string(),
            }
        })? {
            let path = entry.path();
            
            if !path.is_dir() {
                continue;
            }

            let dir_name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            // Skip excluded directories
            if self.config.exclude_patterns.iter().any(|pattern| dir_name.contains(pattern)) {
                continue;
            }

            // Skip already processed paths
            if already_processed.contains(&path) {
                continue;
            }

            // Check if this directory contains a Rust project
            let manifest_path = path.join("Cargo.toml");
            if manifest_path.exists() {
                // Try to load as workspace member
                if let Ok(member) = self.load_workspace_member(
                    workspace_root,
                    &path,
                    &crate::project::manifest::WorkspaceConfig {
                        members: Vec::new(),
                        exclude: Vec::new(),
                        default_members: Vec::new(),
                        dependencies: HashMap::new(),
                        metadata: None,
                        resolver: None,
                    },
                ).await {
                    members.push(member);
                }
            } else {
                // Recursively scan subdirectory
                self.scan_for_members_recursive(
                    workspace_root,
                    &path,
                    members,
                    already_processed,
                    depth + 1,
                ).await?;
            }
        }

        Ok(())
    }

    /// Determine the project type of a workspace member
    fn determine_member_project_type(&self, member_path: &Path) -> ProjectResult<ProjectType> {
        let src_dir = member_path.join("src");
        
        if !src_dir.exists() {
            return Ok(ProjectType::Custom);
        }

        let has_main = src_dir.join("main.rs").exists();
        let has_lib = src_dir.join("lib.rs").exists();

        let project_type = match (has_main, has_lib) {
            (true, true) => ProjectType::Mixed,
            (true, false) => ProjectType::Binary,
            (false, true) => ProjectType::Library,
            (false, false) => ProjectType::Custom,
        };

        Ok(project_type)
    }

    /// Analyze dependencies of a workspace member
    fn analyze_member_dependencies(
        &self,
        manifest: &crate::project::manifest::CargoManifest,
        workspace_config: &crate::project::manifest::WorkspaceConfig,
    ) -> (Vec<String>, Vec<String>) {
        let mut workspace_deps = Vec::new();
        let mut external_deps = Vec::new();

        // Get all workspace member names
        let workspace_members: HashSet<String> = workspace_config.members
            .iter()
            .filter_map(|path| {
                // Extract package name from path (simplified)
                Path::new(path).file_name()
                    .and_then(|name| name.to_str())
                    .map(|s| s.to_string())
            })
            .collect();

        // Analyze regular dependencies
        for (dep_name, dependency) in &manifest.dependencies {
            if workspace_members.contains(dep_name) || dependency.path.is_some() {
                workspace_deps.push(dep_name.clone());
            } else {
                external_deps.push(dep_name.clone());
            }
        }

        // Also check dev and build dependencies
        for dep_name in manifest.dev_dependencies.keys() {
            if workspace_members.contains(dep_name) {
                workspace_deps.push(dep_name.clone());
            }
        }

        for dep_name in manifest.build_dependencies.keys() {
            if workspace_members.contains(dep_name) {
                workspace_deps.push(dep_name.clone());
            }
        }

        workspace_deps.sort();
        workspace_deps.dedup();
        external_deps.sort();
        external_deps.dedup();

        (workspace_deps, external_deps)
    }

    /// Build dependency graph for workspace members
    fn build_dependency_graph(&self, members: &[WorkspaceMember]) -> ProjectResult<DependencyGraph> {
        let nodes: Vec<String> = members.iter().map(|m| m.name.clone()).collect();
        let mut edges = Vec::new();

        // Build edges from dependencies
        for member in members {
            // Add workspace dependency edges
            for dep_name in &member.workspace_dependencies {
                if nodes.contains(dep_name) {
                    edges.push(DependencyEdge {
                        from: member.name.clone(),
                        to: dep_name.clone(),
                        dependency_type: DependencyType::Normal,
                        features: Vec::new(), // TODO: Extract actual features
                    });
                }
            }

            // Add dev dependency edges
            for (dep_name, dependency) in &member.manifest.dev_dependencies {
                if nodes.contains(dep_name) {
                    edges.push(DependencyEdge {
                        from: member.name.clone(),
                        to: dep_name.clone(),
                        dependency_type: DependencyType::Dev,
                        features: dependency.features.clone(),
                    });
                }
            }

            // Add build dependency edges
            for (dep_name, dependency) in &member.manifest.build_dependencies {
                if nodes.contains(dep_name) {
                    edges.push(DependencyEdge {
                        from: member.name.clone(),
                        to: dep_name.clone(),
                        dependency_type: DependencyType::Build,
                        features: dependency.features.clone(),
                    });
                }
            }
        }

        // Calculate build order (topological sort)
        let build_order = self.topological_sort(&nodes, &edges)?;

        // Detect cycles
        let cycles = self.detect_cycles(&nodes, &edges);

        Ok(DependencyGraph {
            nodes,
            edges,
            build_order,
            cycles,
        })
    }

    /// Perform topological sort to determine build order
    fn topological_sort(&self, nodes: &[String], edges: &[DependencyEdge]) -> ProjectResult<Vec<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();

        // Initialize
        for node in nodes {
            in_degree.insert(node.clone(), 0);
            graph.insert(node.clone(), Vec::new());
        }

        // Build adjacency list and calculate in-degrees
        for edge in edges {
            // Skip dev dependencies for build order (they don't affect build)
            if edge.dependency_type == DependencyType::Dev {
                continue;
            }

            if let Some(deps) = graph.get_mut(&edge.to) {
                deps.push(edge.from.clone());
            }
            
            if let Some(degree) = in_degree.get_mut(&edge.from) {
                *degree += 1;
            }
        }

        // Kahn's algorithm
        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(node, _)| node.clone())
            .collect();

        let mut result = Vec::new();

        while let Some(node) = queue.pop() {
            result.push(node.clone());

            if let Some(dependencies) = graph.get(&node) {
                for dep in dependencies {
                    if let Some(degree) = in_degree.get_mut(dep) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(dep.clone());
                        }
                    }
                }
            }
        }

        // Check for cycles
        if result.len() != nodes.len() {
            warn!("Circular dependencies detected in workspace");
        }

        Ok(result)
    }

    /// Detect circular dependencies
    fn detect_cycles(&self, nodes: &[String], edges: &[DependencyEdge]) -> Vec<Vec<String>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        // Build adjacency list (excluding dev dependencies)
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for node in nodes {
            graph.insert(node.clone(), Vec::new());
        }

        for edge in edges {
            if edge.dependency_type != DependencyType::Dev {
                if let Some(deps) = graph.get_mut(&edge.from) {
                    deps.push(edge.to.clone());
                }
            }
        }

        // DFS to detect cycles
        for node in nodes {
            if !visited.contains(node) {
                let mut path = Vec::new();
                self.dfs_detect_cycle(
                    node,
                    &graph,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        cycles
    }

    /// DFS helper for cycle detection
    fn dfs_detect_cycle(
        &self,
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());
        path.push(node.to_string());

        if let Some(neighbors) = graph.get(node) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    self.dfs_detect_cycle(neighbor, graph, visited, rec_stack, path, cycles);
                } else if rec_stack.contains(neighbor) {
                    // Found a cycle
                    if let Some(cycle_start) = path.iter().position(|n| n == neighbor) {
                        let cycle = path[cycle_start..].to_vec();
                        cycles.push(cycle);
                    }
                }
            }
        }

        rec_stack.remove(node);
        path.pop();
    }

    /// Validate workspace consistency
    fn validate_workspace(
        &self,
        members: &[WorkspaceMember],
        dependency_graph: &DependencyGraph,
    ) -> ProjectResult<()> {
        // Check for circular dependencies
        if self.config.check_cycles && !dependency_graph.cycles.is_empty() {
            let cycle_descriptions: Vec<String> = dependency_graph.cycles
                .iter()
                .map(|cycle| cycle.join(" -> "))
                .collect();

            warn!("Circular dependencies found: {}", cycle_descriptions.join(", "));
        }

        // Validate member dependencies exist
        let member_names: HashSet<String> = members.iter().map(|m| m.name.clone()).collect();

        for member in members {
            for workspace_dep in &member.workspace_dependencies {
                if !member_names.contains(workspace_dep) {
                    return Err(ProjectError::WorkspaceError {
                        message: format!(
                            "Member '{}' depends on non-existent workspace member '{}'",
                            member.name, workspace_dep
                        ),
                    });
                }
            }
        }

        debug!("Workspace validation completed successfully");
        Ok(())
    }

    /// Get workspace member by name
    pub fn get_member_by_name<'a>(
        workspace: &'a WorkspaceMetadata,
        name: &str,
    ) -> Option<&'a WorkspaceMember> {
        workspace.members.iter().find(|member| member.name == name)
    }

    /// Get workspace members that depend on a specific member
    pub fn get_dependents(
        workspace: &WorkspaceMetadata,
        target_member: &str,
    ) -> Vec<&WorkspaceMember> {
        workspace
            .members
            .iter()
            .filter(|member| member.workspace_dependencies.contains(&target_member.to_string()))
            .collect()
    }

    /// Get transitive dependencies of a member
    pub fn get_transitive_dependencies(
        workspace: &WorkspaceMetadata,
        member_name: &str,
    ) -> Vec<String> {
        let mut dependencies = HashSet::new();
        let mut to_visit = vec![member_name.to_string()];

        while let Some(current) = to_visit.pop() {
            if let Some(member) = Self::get_member_by_name(workspace, &current) {
                for dep in &member.workspace_dependencies {
                    if dependencies.insert(dep.clone()) {
                        to_visit.push(dep.clone());
                    }
                }
            }
        }

        dependencies.into_iter().collect()
    }

    /// Get configuration
    pub fn config(&self) -> &WorkspaceConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: WorkspaceConfig) {
        self.config = config;
    }
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    async fn create_test_workspace(dir: &Path) -> Result<()> {
        // Root Cargo.toml
        let root_toml = r#"
[workspace]
members = ["app", "lib", "utils"]
exclude = ["old-stuff"]

[workspace.dependencies]
serde = "1.0"
"#;
        fs::write(dir.join("Cargo.toml"), root_toml).await?;

        // App member
        let app_dir = dir.join("app");
        fs::create_dir_all(&app_dir).await?;
        let app_toml = r#"
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
lib = { path = "../lib" }
utils = { path = "../utils" }
"#;
        fs::write(app_dir.join("Cargo.toml"), app_toml).await?;
        fs::create_dir_all(app_dir.join("src")).await?;
        fs::write(app_dir.join("src").join("main.rs"), "fn main() {}").await?;

        // Lib member
        let lib_dir = dir.join("lib");
        fs::create_dir_all(&lib_dir).await?;
        let lib_toml = r#"
[package]
name = "lib"
version = "0.1.0"
edition = "2021"

[dependencies]
utils = { path = "../utils" }
"#;
        fs::write(lib_dir.join("Cargo.toml"), lib_toml).await?;
        fs::create_dir_all(lib_dir.join("src")).await?;
        fs::write(lib_dir.join("src").join("lib.rs"), "pub fn hello() {}").await?;

        // Utils member
        let utils_dir = dir.join("utils");
        fs::create_dir_all(&utils_dir).await?;
        let utils_toml = r#"
[package]
name = "utils"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(utils_dir.join("Cargo.toml"), utils_toml).await?;
        fs::create_dir_all(utils_dir.join("src")).await?;
        fs::write(utils_dir.join("src").join("lib.rs"), "pub fn util() {}").await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_load_workspace() {
        let temp_dir = TempDir::new().unwrap();
        create_test_workspace(temp_dir.path()).await.unwrap();

        let manager = WorkspaceManager::new();
        let workspace = manager.load_workspace(temp_dir.path()).await.unwrap();

        assert_eq!(workspace.members.len(), 3);
        assert!(workspace.members.iter().any(|m| m.name == "app"));
        assert!(workspace.members.iter().any(|m| m.name == "lib"));
        assert!(workspace.members.iter().any(|m| m.name == "utils"));
    }

    #[tokio::test]
    async fn test_dependency_graph() {
        let temp_dir = TempDir::new().unwrap();
        create_test_workspace(temp_dir.path()).await.unwrap();

        let manager = WorkspaceManager::new();
        let workspace = manager.load_workspace(temp_dir.path()).await.unwrap();

        let graph = &workspace.dependency_graph;
        
        // Check nodes
        assert_eq!(graph.nodes.len(), 3);
        assert!(graph.nodes.contains(&"app".to_string()));
        assert!(graph.nodes.contains(&"lib".to_string()));
        assert!(graph.nodes.contains(&"utils".to_string()));

        // Check edges
        assert!(!graph.edges.is_empty());
        
        // Check build order (utils should come first, then lib, then app)
        let utils_pos = graph.build_order.iter().position(|n| n == "utils").unwrap();
        let lib_pos = graph.build_order.iter().position(|n| n == "lib").unwrap();
        let app_pos = graph.build_order.iter().position(|n| n == "app").unwrap();
        
        assert!(utils_pos < lib_pos);
        assert!(lib_pos < app_pos);
    }

    #[tokio::test]
    async fn test_member_analysis() {
        let temp_dir = TempDir::new().unwrap();
        create_test_workspace(temp_dir.path()).await.unwrap();

        let manager = WorkspaceManager::new();
        let workspace = manager.load_workspace(temp_dir.path()).await.unwrap();

        // Test app member
        let app_member = WorkspaceManager::get_member_by_name(&workspace, "app").unwrap();
        assert_eq!(app_member.project_type, ProjectType::Binary);
        assert!(app_member.workspace_dependencies.contains(&"lib".to_string()));
        assert!(app_member.workspace_dependencies.contains(&"utils".to_string()));

        // Test lib member
        let lib_member = WorkspaceManager::get_member_by_name(&workspace, "lib").unwrap();
        assert_eq!(lib_member.project_type, ProjectType::Library);
        assert!(lib_member.workspace_dependencies.contains(&"utils".to_string()));

        // Test utils member
        let utils_member = WorkspaceManager::get_member_by_name(&workspace, "utils").unwrap();
        assert_eq!(utils_member.project_type, ProjectType::Library);
        assert!(utils_member.workspace_dependencies.is_empty());
    }

    #[tokio::test]
    async fn test_dependency_utilities() {
        let temp_dir = TempDir::new().unwrap();
        create_test_workspace(temp_dir.path()).await.unwrap();

        let manager = WorkspaceManager::new();
        let workspace = manager.load_workspace(temp_dir.path()).await.unwrap();

        // Test dependents
        let utils_dependents = WorkspaceManager::get_dependents(&workspace, "utils");
        assert_eq!(utils_dependents.len(), 2); // lib and app depend on utils

        let lib_dependents = WorkspaceManager::get_dependents(&workspace, "lib");
        assert_eq!(lib_dependents.len(), 1); // only app depends on lib

        // Test transitive dependencies
        let app_transitive = WorkspaceManager::get_transitive_dependencies(&workspace, "app");
        assert!(app_transitive.contains(&"lib".to_string()));
        assert!(app_transitive.contains(&"utils".to_string()));

        let lib_transitive = WorkspaceManager::get_transitive_dependencies(&workspace, "lib");
        assert!(lib_transitive.contains(&"utils".to_string()));
        assert!(!lib_transitive.contains(&"lib".to_string()));
    }

    #[tokio::test]
    async fn test_circular_dependency_detection() {
        let temp_dir = TempDir::new().unwrap();
        
        // Create workspace with circular dependency
        let root_toml = r#"
[workspace]
members = ["a", "b"]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), root_toml).await.unwrap();

        // Package A depends on B
        let a_dir = temp_dir.path().join("a");
        fs::create_dir_all(&a_dir).await.unwrap();
        let a_toml = r#"
[package]
name = "a"
version = "0.1.0"
edition = "2021"

[dependencies]
b = { path = "../b" }
"#;
        fs::write(a_dir.join("Cargo.toml"), a_toml).await.unwrap();
        fs::create_dir_all(a_dir.join("src")).await.unwrap();
        fs::write(a_dir.join("src").join("lib.rs"), "").await.unwrap();

        // Package B depends on A (circular)
        let b_dir = temp_dir.path().join("b");
        fs::create_dir_all(&b_dir).await.unwrap();
        let b_toml = r#"
[package]
name = "b"
version = "0.1.0"
edition = "2021"

[dependencies]
a = { path = "../a" }
"#;
        fs::write(b_dir.join("Cargo.toml"), b_toml).await.unwrap();
        fs::create_dir_all(b_dir.join("src")).await.unwrap();
        fs::write(b_dir.join("src").join("lib.rs"), "").await.unwrap();

        let manager = WorkspaceManager::new();
        let workspace = manager.load_workspace(temp_dir.path()).await.unwrap();

        // Should detect the circular dependency
        assert!(!workspace.dependency_graph.cycles.is_empty());
    }

    #[tokio::test]
    async fn test_workspace_validation() {
        let temp_dir = TempDir::new().unwrap();

        // Create invalid workspace (member depends on non-existent package)
        let root_toml = r#"
[workspace]
members = ["app"]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), root_toml).await.unwrap();

        let app_dir = temp_dir.path().join("app");
        fs::create_dir_all(&app_dir).await.unwrap();
        let app_toml = r#"
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
nonexistent = { path = "../nonexistent" }
"#;
        fs::write(app_dir.join("Cargo.toml"), app_toml).await.unwrap();
        fs::create_dir_all(app_dir.join("src")).await.unwrap();
        fs::write(app_dir.join("src").join("main.rs"), "fn main() {}").await.unwrap();

        let manager = WorkspaceManager::new();
        
        // Should fail validation due to missing dependency
        let result = manager.load_workspace(temp_dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_glob_pattern_resolution() {
        let temp_dir = TempDir::new().unwrap();

        // Create workspace with glob pattern
        let root_toml = r#"
[workspace]
members = ["packages/*"]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), root_toml).await.unwrap();

        // Create packages directory with multiple packages
        let packages_dir = temp_dir.path().join("packages");
        fs::create_dir_all(&packages_dir).await.unwrap();

        for pkg_name in ["pkg1", "pkg2", "pkg3"] {
            let pkg_dir = packages_dir.join(pkg_name);
            fs::create_dir_all(&pkg_dir).await.unwrap();
            
            let pkg_toml = format!(r#"
[package]
name = "{}"
version = "0.1.0"
edition = "2021"
"#, pkg_name);
            fs::write(pkg_dir.join("Cargo.toml"), pkg_toml).await.unwrap();
            fs::create_dir_all(pkg_dir.join("src")).await.unwrap();
            fs::write(pkg_dir.join("src").join("lib.rs"), "").await.unwrap();
        }

        let manager = WorkspaceManager::new();
        let workspace = manager.load_workspace(temp_dir.path()).await.unwrap();

        assert_eq!(workspace.members.len(), 3);
        assert!(workspace.members.iter().any(|m| m.name == "pkg1"));
        assert!(workspace.members.iter().any(|m| m.name == "pkg2"));
        assert!(workspace.members.iter().any(|m| m.name == "pkg3"));
    }

    #[tokio::test]
    async fn test_excluded_members() {
        let temp_dir = TempDir::new().unwrap();

        // Create workspace with excluded member
        let root_toml = r#"
[workspace]
members = ["app", "lib"]
exclude = ["old-app"]
"#;
        fs::write(temp_dir.path().join("Cargo.toml"), root_toml).await.unwrap();

        // Create regular members
        for member in ["app", "lib"] {
            let member_dir = temp_dir.path().join(member);
            fs::create_dir_all(&member_dir).await.unwrap();
            let member_toml = format!(r#"
[package]
name = "{}"
version = "0.1.0"
edition = "2021"
"#, member);
            fs::write(member_dir.join("Cargo.toml"), member_toml).await.unwrap();
            fs::create_dir_all(member_dir.join("src")).await.unwrap();
            fs::write(member_dir.join("src").join("lib.rs"), "").await.unwrap();
        }

        // Create excluded member
        let excluded_dir = temp_dir.path().join("old-app");
        fs::create_dir_all(&excluded_dir).await.unwrap();
        let excluded_toml = r#"
[package]
name = "old-app"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(excluded_dir.join("Cargo.toml"), excluded_toml).await.unwrap();
        fs::create_dir_all(excluded_dir.join("src")).await.unwrap();
        fs::write(excluded_dir.join("src").join("main.rs"), "fn main() {}").await.unwrap();

        let manager = WorkspaceManager::new();
        let workspace = manager.load_workspace(temp_dir.path()).await.unwrap();

        // Should only have 2 members (excluded member should not be included)
        assert_eq!(workspace.members.len(), 2);
        assert!(workspace.members.iter().any(|m| m.name == "app"));
        assert!(workspace.members.iter().any(|m| m.name == "lib"));
        assert!(!workspace.members.iter().any(|m| m.name == "old-app"));
    }

    #[test]
    fn test_dependency_type_serialization() {
        let dep_type = DependencyType::Normal;
        let serialized = serde_json::to_string(&dep_type).unwrap();
        let deserialized: DependencyType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(dep_type, deserialized);
    }

    #[test]
    fn test_workspace_config_defaults() {
        let config = WorkspaceConfig::default();
        assert!(config.auto_detect_members);
        assert_eq!(config.max_scan_depth, 5);
        assert!(config.validate_dependencies);
        assert!(config.check_cycles);
    }
}