use crate::project::{ProjectError, ProjectResult, FileTreeConfig};
use crate::utils::{file_watcher::FileWatcher, paths::PathUtils};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;
use tracing::{debug, info, instrument, warn};

/// Represents a node in the file tree
#[derive(Debug, Clone, Serialize)]
pub struct FileNode {
    /// Node name (file or directory name)
    pub name: String,
    /// Full path to the node
    pub path: PathBuf,
    /// Relative path from project root
    pub relative_path: PathBuf,
    /// Node type
    pub node_type: FileNodeType,
    /// File size (for files)
    pub size: Option<u64>,
    /// Last modified time
    pub modified: Option<SystemTime>,
    /// Whether this node is expanded in the UI
    pub expanded: bool,
    /// Child nodes (for directories)
    pub children: Vec<FileNode>,
    /// File extension (for files)
    pub extension: Option<String>,
    /// Whether this file is ignored by git/project settings
    pub ignored: bool,
    /// Custom metadata
    pub metadata: FileNodeMetadata,
}

/// Type of file tree node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FileNodeType {
    /// Regular file
    File,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
    /// Special file (device, pipe, etc.)
    Special,
}

/// Additional metadata for file nodes
#[derive(Debug, Clone, Serialize)]
pub struct FileNodeMetadata {
    /// Whether this is a Rust source file
    pub is_rust_file: bool,
    /// Whether this is a configuration file
    pub is_config_file: bool,
    /// Whether this is a test file
    pub is_test_file: bool,
    /// Whether this is a documentation file
    pub is_doc_file: bool,
    /// Whether this is a build artifact
    pub is_build_artifact: bool,
    /// Git status (if available)
    pub git_status: Option<GitStatus>,
    /// Line count (for text files)
    pub line_count: Option<usize>,
    /// Whether this file is binary
    pub is_binary: bool,
}

/// Git status for files
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GitStatus {
    /// File is untracked
    Untracked,
    /// File is modified
    Modified,
    /// File is staged
    Staged,
    /// File is deleted
    Deleted,
    /// File is renamed
    Renamed,
    /// File is up to date
    Clean,
}

/// Filter configuration for file tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTreeFilter {
    /// Show hidden files (starting with .)
    pub show_hidden: bool,
    /// Show ignored files
    pub show_ignored: bool,
    /// File extension filters
    pub extension_filters: Vec<String>,
    /// Name pattern filters
    pub name_patterns: Vec<String>,
    /// Maximum depth to show
    pub max_depth: Option<usize>,
    /// Only show specific file types
    pub file_type_filters: Vec<FileNodeType>,
}

impl Default for FileTreeFilter {
    fn default() -> Self {
        Self {
            show_hidden: false,
            show_ignored: false,
            extension_filters: Vec::new(),
            name_patterns: Vec::new(),
            max_depth: None,
            file_type_filters: Vec::new(),
        }
    }
}

/// Events that can occur in the file tree
#[derive(Debug, Clone, Serialize)]
pub enum TreeUpdateEvent {
    /// File or directory was created
    Created { path: PathBuf },
    /// File or directory was modified
    Modified { path: PathBuf },
    /// File or directory was deleted
    Deleted { path: PathBuf },
    /// File or directory was renamed
    Renamed { from: PathBuf, to: PathBuf },
    /// Directory contents changed
    DirectoryChanged { path: PathBuf },
    /// Full tree refresh needed
    FullRefresh,
}

/// File tree statistics
#[derive(Debug, Clone, Serialize)]
pub struct FileTreeStats {
    /// Total number of files
    pub total_files: usize,
    /// Total number of directories
    pub total_directories: usize,
    /// Number of Rust source files
    pub rust_files: usize,
    /// Number of test files
    pub test_files: usize,
    /// Number of hidden files
    pub hidden_files: usize,
    /// Number of ignored files
    pub ignored_files: usize,
    /// Total size of all files
    pub total_size: u64,
    /// Number of symlinks
    pub symlinks: usize,
}

/// Main file tree structure
pub struct FileTree {
    /// Project root path
    root_path: PathBuf,
    /// Root node of the tree
    root_node: Option<FileNode>,
    /// Configuration
    config: FileTreeConfig,
    /// Current filter
    filter: FileTreeFilter,
    /// Ignore patterns (from .gitignore, etc.)
    ignore_patterns: Vec<String>,
    /// File watcher for real-time updates
    file_watcher: Option<FileWatcher>,
    /// Path utilities
    path_utils: PathUtils,
    /// Cache of expanded paths
    expanded_paths: HashSet<PathBuf>,
    /// Last scan time
    last_scan: Option<SystemTime>,
    /// Event listeners
    event_listeners: Vec<Box<dyn Fn(&TreeUpdateEvent) + Send + Sync>>,
}

impl FileTree {
    /// Create a new file tree
    pub fn new(root_path: PathBuf, config: FileTreeConfig) -> Self {
        Self {
            root_path,
            root_node: None,
            config,
            filter: FileTreeFilter::default(),
            ignore_patterns: Vec::new(),
            file_watcher: None,
            path_utils: PathUtils::new(),
            expanded_paths: HashSet::new(),
            last_scan: None,
            event_listeners: Vec::new(),
        }
    }

    /// Scan the file tree from the root
    #[instrument(skip(self))]
    pub async fn scan(&mut self) -> Result<()> {
        info!("Scanning file tree at: {}", self.root_path.display());

        // Load ignore patterns
        self.load_ignore_patterns().await?;

        // Scan the root directory
        let root_node = self.scan_directory(&self.root_path, 0).await?;
        self.root_node = Some(root_node);
        self.last_scan = Some(SystemTime::now());

        // Setup file watcher if not already setup
        if self.file_watcher.is_none() {
            self.setup_file_watcher().await?;
        }

        info!("File tree scan completed");
        Ok(())
    }

    /// Scan a directory recursively
    async fn scan_directory(&self, dir_path: &Path, depth: usize) -> Result<FileNode> {
        let metadata = fs::metadata(dir_path).await.with_context(|| {
            format!("Failed to get metadata for {}", dir_path.display())
        })?;

        let name = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let relative_path = self.path_utils.get_relative_path(&self.root_path, dir_path)?;

        let mut node = FileNode {
            name: name.clone(),
            path: dir_path.to_path_buf(),
            relative_path,
            node_type: FileNodeType::Directory,
            size: None,
            modified: metadata.modified().ok(),
            expanded: self.should_auto_expand(&name) || self.expanded_paths.contains(dir_path),
            children: Vec::new(),
            extension: None,
            ignored: self.is_ignored_path(dir_path),
            metadata: FileNodeMetadata {
                is_rust_file: false,
                is_config_file: self.is_config_directory(&name),
                is_test_file: name == "tests" || name == "test",
                is_doc_file: name == "docs" || name == "doc",
                is_build_artifact: name == "target" || name == "build",
                git_status: None, // TODO: Implement git status
                line_count: None,
                is_binary: false,
            },
        };

        // Skip if ignored and not showing ignored files
        if node.ignored && !self.filter.show_ignored {
            return Ok(node);
        }

        // Check depth limit
        if let Some(max_depth) = self.config.max_depth {
            if depth >= max_depth {
                return Ok(node);
            }
        }

        // Check filter depth
        if let Some(filter_depth) = self.filter.max_depth {
            if depth >= filter_depth {
                return Ok(node);
            }
        }

        // Read directory contents
        let mut entries = fs::read_dir(dir_path).await.with_context(|| {
            format!("Failed to read directory {}", dir_path.display())
        })?;

        while let Some(entry) = entries.next_entry().await? {
            let entry_path = entry.path();
            let entry_name = entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            // Skip hidden files if not showing them
            if !self.filter.show_hidden && entry_name.starts_with('.') && entry_name != ".." {
                continue;
            }

            // Skip if matches ignore patterns
            if self.is_ignored_path(&entry_path) && !self.filter.show_ignored {
                continue;
            }

            let child_node = if entry_path.is_dir() {
                self.scan_directory(&entry_path, depth + 1).await?
            } else {
                self.scan_file(&entry_path).await?
            };

            // Apply filters
            if self.should_include_node(&child_node) {
                node.children.push(child_node);
            }
        }

        // Sort children
        self.sort_children(&mut node.children);

        Ok(node)
    }

    /// Scan a single file
    async fn scan_file(&self, file_path: &Path) -> Result<FileNode> {
        let metadata = fs::metadata(file_path).await.with_context(|| {
            format!("Failed to get metadata for {}", file_path.display())
        })?;

        let name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_string());

        let relative_path = self.path_utils.get_relative_path(&self.root_path, file_path)?;

        let node_type = if metadata.file_type().is_symlink() {
            FileNodeType::Symlink
        } else if metadata.file_type().is_file() {
            FileNodeType::File
        } else {
            FileNodeType::Special
        };

        let is_rust_file = extension.as_ref().map_or(false, |ext| ext == "rs");
        let is_config_file = self.is_config_file(&name);
        let is_test_file = self.is_test_file(file_path);
        let is_doc_file = self.is_doc_file(&name, &extension);
        let is_build_artifact = self.is_build_artifact(file_path);

        // Detect if file is binary
        let is_binary = if metadata.len() > 0 && metadata.len() < 8192 {
            // Sample first few bytes to detect binary files
            if let Ok(sample) = fs::read(file_path).await {
                let sample_len = sample.len().min(512);
                sample[..sample_len].contains(&0)
            } else {
                false
            }
        } else {
            // Assume large files or empty files are not binary text
            metadata.len() > 10_000_000 || metadata.len() == 0
        };

        // Count lines for text files
        let line_count = if !is_binary && is_rust_file {
            self.count_lines(file_path).await.ok()
        } else {
            None
        };

        Ok(FileNode {
            name,
            path: file_path.to_path_buf(),
            relative_path,
            node_type,
            size: Some(metadata.len()),
            modified: metadata.modified().ok(),
            expanded: false, // Files are never expanded
            children: Vec::new(), // Files have no children
            extension,
            ignored: self.is_ignored_path(file_path),
            metadata: FileNodeMetadata {
                is_rust_file,
                is_config_file,
                is_test_file,
                is_doc_file,
                is_build_artifact,
                git_status: None, // TODO: Implement git status
                line_count,
                is_binary,
            },
        })
    }

    /// Load ignore patterns from .gitignore and other sources
    async fn load_ignore_patterns(&mut self) -> Result<()> {
        let mut patterns = self.config.ignore_patterns.clone();

        // Load from .gitignore
        let gitignore_path = self.root_path.join(".gitignore");
        if gitignore_path.exists() {
            let content = fs::read_to_string(&gitignore_path).await?;
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    patterns.push(line.to_string());
                }
            }
        }

        // Load from .ignore
        let ignore_path = self.root_path.join(".ignore");
        if ignore_path.exists() {
            let content = fs::read_to_string(&ignore_path).await?;
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    patterns.push(line.to_string());
                }
            }
        }

        // Add default ignore patterns
        patterns.extend_from_slice(&[
            ".DS_Store".to_string(),
            "Thumbs.db".to_string(),
            "*.tmp".to_string(),
            "*.swp".to_string(),
            "*.bak".to_string(),
        ]);

        self.ignore_patterns = patterns;
        debug!("Loaded {} ignore patterns", self.ignore_patterns.len());
        Ok(())
    }

    /// Setup file watcher for real-time updates
    async fn setup_file_watcher(&mut self) -> Result<()> {
        let mut watcher = FileWatcher::new();
        
        // Watch the root directory
        watcher.watch_directory(&self.root_path, self.ignore_patterns.clone())?;
        
        self.file_watcher = Some(watcher);
        debug!("File watcher setup completed");
        Ok(())
    }

    /// Check if a path should be ignored
    fn is_ignored_path(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        
        for pattern in &self.ignore_patterns {
            if self.matches_pattern(&path_str, pattern) {
                return true;
            }
        }

        false
    }

    /// Check if a pattern matches a path (simplified glob matching)
    fn matches_pattern(&self, path: &str, pattern: &str) -> bool {
        if pattern.contains('*') {
            // Simple glob pattern matching
            if pattern.starts_with("*.") {
                let ext = &pattern[2..];
                return path.ends_with(ext);
            } else if pattern.ends_with("/*") {
                let prefix = &pattern[..pattern.len() - 2];
                return path.starts_with(prefix);
            }
        }
        
        path.contains(pattern)
    }

    /// Check if a directory should be auto-expanded
    fn should_auto_expand(&self, dir_name: &str) -> bool {
        self.config.auto_expand_patterns.iter().any(|pattern| {
            dir_name == pattern || dir_name.contains(pattern)
        })
    }

    /// Check if a file is a configuration file
    fn is_config_file(&self, name: &str) -> bool {
        matches!(
            name,
            "Cargo.toml" | "Cargo.lock" | ".gitignore" | ".gitattributes" |
            "README.md" | "LICENSE" | "LICENSE.txt" | "LICENSE.md" |
            ".rustfmt.toml" | "rustfmt.toml" | "clippy.toml" |
            ".cargo" | "rust-toolchain" | "rust-toolchain.toml"
        ) || name.ends_with(".toml") || name.ends_with(".json") || name.ends_with(".yaml") || name.ends_with(".yml")
    }

    /// Check if a directory is a configuration directory
    fn is_config_directory(&self, name: &str) -> bool {
        matches!(name, ".cargo" | ".vscode" | ".idea" | "config")
    }

    /// Check if a file is a test file
    fn is_test_file(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        
        // Check if it's in a test directory
        if path_str.contains("/tests/") || path_str.contains("/test/") {
            return true;
        }

        // Check if filename indicates it's a test
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            return name.contains("test") || name.starts_with("test_") || name.ends_with("_test.rs");
        }

        false
    }

    /// Check if a file is a documentation file
    fn is_doc_file(&self, name: &str, extension: &Option<String>) -> bool {
        matches!(
            name,
            "README.md" | "README.txt" | "README" | "CHANGELOG.md" |
            "CHANGELOG.txt" | "CHANGELOG" | "CONTRIBUTING.md" | "docs"
        ) || extension.as_ref().map_or(false, |ext| {
            matches!(ext.as_str(), "md" | "txt" | "rst" | "adoc")
        })
    }

    /// Check if a file is a build artifact
    fn is_build_artifact(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        
        path_str.contains("/target/") ||
        path_str.contains("/build/") ||
        path_str.contains("/dist/") ||
        path_str.contains("/.git/")
    }

    /// Count lines in a text file
    async fn count_lines(&self, path: &Path) -> Result<usize> {
        let content = fs::read_to_string(path).await?;
        Ok(content.lines().count())
    }

    /// Check if a node should be included based on current filter
    fn should_include_node(&self, node: &FileNode) -> bool {
        // Check file type filters
        if !self.filter.file_type_filters.is_empty() {
            if !self.filter.file_type_filters.contains(&node.node_type) {
                return false;
            }
        }

        // Check extension filters
        if !self.filter.extension_filters.is_empty() {
            if let Some(ext) = &node.extension {
                if !self.filter.extension_filters.contains(ext) {
                    return false;
                }
            } else if node.node_type == FileNodeType::File {
                // File without extension doesn't match extension filter
                return false;
            }
        }

        // Check name pattern filters
        if !self.filter.name_patterns.is_empty() {
            let matches_pattern = self.filter.name_patterns.iter().any(|pattern| {
                self.matches_pattern(&node.name, pattern)
            });
            if !matches_pattern {
                return false;
            }
        }

        true
    }

    /// Sort children nodes
    fn sort_children(&self, children: &mut Vec<FileNode>) {
        children.sort_by(|a, b| {
            // Directories first
            match (a.node_type, b.node_type) {
                (FileNodeType::Directory, FileNodeType::File) => std::cmp::Ordering::Less,
                (FileNodeType::File, FileNodeType::Directory) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });
    }

    /// Get the root node
    pub fn root_node(&self) -> Option<&FileNode> {
        self.root_node.as_ref()
    }

    /// Set current filter
    pub fn set_filter(&mut self, filter: FileTreeFilter) {
        self.filter = filter;
    }

    /// Get current filter
    pub fn filter(&self) -> &FileTreeFilter {
        &self.filter
    }

    /// Expand or collapse a node
    pub async fn toggle_expansion(&mut self, path: &Path) -> Result<()> {
        if self.expanded_paths.contains(path) {
            self.expanded_paths.remove(path);
        } else {
            self.expanded_paths.insert(path.to_path_buf());
        }

        // Update the node in the tree
        if let Some(root) = &mut self.root_node {
            self.update_node_expansion(root, path);
        }

        Ok(())
    }

    /// Update expansion state of a node
    fn update_node_expansion(&self, node: &mut FileNode, target_path: &Path) {
        if node.path == target_path {
            node.expanded = self.expanded_paths.contains(target_path);
            return;
        }

        for child in &mut node.children {
            self.update_node_expansion(child, target_path);
        }
    }

    /// Find a node by path
    pub fn find_node(&self, path: &Path) -> Option<&FileNode> {
        if let Some(root) = &self.root_node {
            self.find_node_recursive(root, path)
        } else {
            None
        }
    }

    /// Recursively find a node
    fn find_node_recursive(&self, node: &FileNode, target_path: &Path) -> Option<&FileNode> {
        if node.path == target_path {
            return Some(node);
        }

        for child in &node.children {
            if let Some(found) = self.find_node_recursive(child, target_path) {
                return Some(found);
            }
        }

        None
    }

    /// Get all files in the tree
    pub fn all_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Some(root) = &self.root_node {
            self.collect_files(root, &mut files);
        }
        files
    }

    /// Recursively collect all file paths
    fn collect_files(&self, node: &FileNode, files: &mut Vec<PathBuf>) {
        if node.node_type == FileNodeType::File {
            files.push(node.path.clone());
        }

        for child in &node.children {
            self.collect_files(child, files);
        }
    }

    /// Get file tree statistics
    pub fn statistics(&self) -> FileTreeStats {
        let mut stats = FileTreeStats {
            total_files: 0,
            total_directories: 0,
            rust_files: 0,
            test_files: 0,
            hidden_files: 0,
            ignored_files: 0,
            total_size: 0,
            symlinks: 0,
        };

        if let Some(root) = &self.root_node {
            self.collect_stats(root, &mut stats);
        }

        stats
    }

    /// Recursively collect statistics
    fn collect_stats(&self, node: &FileNode, stats: &mut FileTreeStats) {
        match node.node_type {
            FileNodeType::File => {
                stats.total_files += 1;
                if let Some(size) = node.size {
                    stats.total_size += size;
                }
                if node.metadata.is_rust_file {
                    stats.rust_files += 1;
                }
                if node.metadata.is_test_file {
                    stats.test_files += 1;
                }
            }
            FileNodeType::Directory => {
                stats.total_directories += 1;
            }
            FileNodeType::Symlink => {
                stats.symlinks += 1;
            }
            _ => {}
        }

        if node.name.starts_with('.') {
            stats.hidden_files += 1;
        }

        if node.ignored {
            stats.ignored_files += 1;
        }

        for child in &node.children {
            self.collect_stats(child, stats);
        }
    }

    /// Get file count
    pub fn file_count(&self) -> usize {
        self.statistics().total_files
    }

    /// Get directory count
    pub fn directory_count(&self) -> usize {
        self.statistics().total_directories
    }

    /// Refresh a specific path
    pub async fn refresh_path(&mut self, path: &Path) -> Result<()> {
        info!("Refreshing path: {}", path.display());

        // Check if path is under our root
        if !path.starts_with(&self.root_path) {
            return Ok(());
        }

        // If it's the root path, do a full rescan
        if path == self.root_path {
            return self.scan().await;
        }

        // Otherwise, rescan just this subtree
        if path.is_dir() {
            // Find the parent node and update this directory
            if let Some(root) = &mut self.root_node {
                self.refresh_node_recursive(root, path).await?;
            }
        } else {
            // For files, refresh the parent directory
            if let Some(parent) = path.parent() {
                self.refresh_path(parent).await?;
            }
        }

        Ok(())
    }

    /// Recursively refresh a node
    async fn refresh_node_recursive(&mut self, node: &mut FileNode, target_path: &Path) -> Result<()> {
        if node.path == target_path && node.node_type == FileNodeType::Directory {
            // Refresh this directory
            let relative_path = self.path_utils.get_relative_path(&self.root_path, &node.path)?;
            let depth = relative_path.components().count();
            let refreshed_node = self.scan_directory(&node.path, depth).await?;
            
            // Preserve expansion state
            let was_expanded = node.expanded;
            *node = refreshed_node;
            node.expanded = was_expanded;
            
            return Ok(());
        }

        // Check if target is under this node
        if target_path.starts_with(&node.path) {
            for child in &mut node.children {
                self.refresh_node_recursive(child, target_path).await?;
            }
        }

        Ok(())
    }

    /// Add event listener
    pub fn add_event_listener<F>(&mut self, listener: F)
    where
        F: Fn(&TreeUpdateEvent) + Send + Sync + 'static,
    {
        self.event_listeners.push(Box::new(listener));
    }

    /// Emit an event to all listeners
    fn emit_event(&self, event: TreeUpdateEvent) {
        for listener in &self.event_listeners {
            listener(&event);
        }
    }

    /// Handle file system events from the watcher
    pub async fn handle_fs_event(&mut self, event: crate::utils::file_watcher::FileEvent) -> Result<()> {
        use crate::utils::file_watcher::FileEvent;

        match event {
            FileEvent::Created { path } => {
                self.refresh_path(&path).await?;
                self.emit_event(TreeUpdateEvent::Created { path });
            }
            FileEvent::Modified { path } => {
                self.refresh_path(&path).await?;
                self.emit_event(TreeUpdateEvent::Modified { path });
            }
            FileEvent::Deleted { path } => {
                self.refresh_path(&path).await?;
                self.emit_event(TreeUpdateEvent::Deleted { path });
            }
            FileEvent::Renamed { old_path, new_path } => {
                self.refresh_path(&old_path).await?;
                self.refresh_path(&new_path).await?;
                self.emit_event(TreeUpdateEvent::Renamed { from: old_path, to: new_path });
            }
            FileEvent::MetadataChanged { path } => {
                self.refresh_path(&path).await?;
                // Optionally emit a specific event, or just refresh
                self.emit_event(TreeUpdateEvent::Modified { path });
            }
        }

        Ok(())
    }

    /// Check if file watcher has pending changes
    pub fn has_pending_changes(&self) -> bool {
        self.file_watcher
            .as_ref()
            .map(|w| w.has_pending_changes())
            .unwrap_or(false)
    }

    /// Get the root path
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    /// Get configuration
    pub fn config(&self) -> &FileTreeConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: FileTreeConfig) {
        self.config = config;
    }

    /// Get last scan time
    pub fn last_scan_time(&self) -> Option<SystemTime> {
        self.last_scan
    }
}

impl Default for FileNodeMetadata {
    fn default() -> Self {
        Self {
            is_rust_file: false,
            is_config_file: false,
            is_test_file: false,
            is_doc_file: false,
            is_build_artifact: false,
            git_status: None,
            line_count: None,
            is_binary: false,
        }
    }
}

/// Utility functions for file tree operations
pub mod utils {
    use super::*;

    /// Convert file size to human readable format
    pub fn format_file_size(size: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = size as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} {}", size as u64, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size, UNITS[unit_index])
        }
    }

    /// Get file icon based on file type and extension
    pub fn get_file_icon(node: &FileNode) -> &'static str {
        match node.node_type {
            FileNodeType::Directory => {
                if node.expanded {
                    "📂"
                } else {
                    "📁"
                }
            }
            FileNodeType::File => {
                if let Some(ext) = &node.extension {
                    match ext.as_str() {
                        "rs" => "🦀",
                        "toml" => "⚙️",
                        "md" => "📝",
                        "json" => "📄",
                        "txt" => "📄",
                        "yaml" | "yml" => "📄",
                        "lock" => "🔒",
                        _ => "📄",
                    }
                } else {
                    match node.name.as_str() {
                        "Cargo.toml" => "📦",
                        "Cargo.lock" => "🔒",
                        "README.md" => "📖",
                        "LICENSE" => "📜",
                        _ => "📄",
                    }
                }
            }
            FileNodeType::Symlink => "🔗",
            FileNodeType::Special => "⚠️",
        }
    }

    /// Check if a file should be syntax highlighted
    pub fn should_highlight(node: &FileNode) -> bool {
        if node.metadata.is_binary {
            return false;
        }

        if let Some(ext) = &node.extension {
            matches!(
                ext.as_str(),
                "rs" | "toml" | "md" | "json" | "yaml" | "yml" | "txt"
            )
        } else {
            false
        }
    }

    /// Get relative path components for breadcrumb display
    pub fn get_breadcrumb_components(path: &Path, root: &Path) -> Vec<String> {
        if let Ok(relative) = path.strip_prefix(root) {
            relative
                .components()
                .filter_map(|comp| comp.as_os_str().to_str())
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Filter nodes based on search query
    pub fn filter_by_search(nodes: &[FileNode], query: &str) -> Vec<&FileNode> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for node in nodes {
            if node.name.to_lowercase().contains(&query_lower) {
                results.push(node);
            }
            
            // Recursively search children
            let child_results = filter_by_search(&node.children, query);
            results.extend(child_results);
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    async fn create_test_project(dir: &Path) -> Result<()> {
        // Create Cargo.toml
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"test\"\nversion = \"0.1.0\"").await?;

        // Create src directory with files
        let src_dir = dir.join("src");
        fs::create_dir_all(&src_dir).await?;
        fs::write(src_dir.join("main.rs"), "fn main() {}").await?;
        fs::write(src_dir.join("lib.rs"), "pub fn hello() {}").await?;

        // Create tests directory
        let tests_dir = dir.join("tests");
        fs::create_dir_all(&tests_dir).await?;
        fs::write(tests_dir.join("integration_test.rs"), "#[test]\nfn test() {}").await?;

        // Create docs
        fs::write(dir.join("README.md"), "# Test Project").await?;

        // Create target directory (should be ignored)
        let target_dir = dir.join("target");
        fs::create_dir_all(&target_dir).await?;
        fs::write(target_dir.join("some_artifact"), "binary data").await?;

        // Create hidden file
        fs::write(dir.join(".gitignore"), "target/\n*.tmp").await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_file_tree_creation() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = FileTreeConfig::default();
        let mut tree = FileTree::new(temp_dir.path().to_path_buf(), config);

        tree.scan().await.unwrap();

        let root = tree.root_node().unwrap();
        assert!(root.children.len() > 0);

        // Check that we have src directory
        let src_node = root.children.iter().find(|n| n.name == "src").unwrap();
        assert_eq!(src_node.node_type, FileNodeType::Directory);
        assert!(src_node.children.len() > 0);
    }

    #[tokio::test]
    async fn test_file_metadata() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = FileTreeConfig::default();
        let mut tree = FileTree::new(temp_dir.path().to_path_buf(), config);

        tree.scan().await.unwrap();

        let root = tree.root_node().unwrap();

        // Check Cargo.toml metadata
        let cargo_toml = root.children.iter().find(|n| n.name == "Cargo.toml").unwrap();
        assert!(cargo_toml.metadata.is_config_file);
        assert_eq!(cargo_toml.extension, Some("toml".to_string()));

        // Check main.rs metadata
        let src_node = root.children.iter().find(|n| n.name == "src").unwrap();
        let main_rs = src_node.children.iter().find(|n| n.name == "main.rs").unwrap();
        assert!(main_rs.metadata.is_rust_file);
        assert_eq!(main_rs.extension, Some("rs".to_string()));
    }

    #[tokio::test]
    async fn test_file_filtering() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = FileTreeConfig::default();
        let mut tree = FileTree::new(temp_dir.path().to_path_buf(), config);

        // Set filter to only show Rust files
        let filter = FileTreeFilter {
            show_hidden: false,
            show_ignored: false,
            extension_filters: vec!["rs".to_string()],
            name_patterns: Vec::new(),
            max_depth: None,
            file_type_filters: vec![FileNodeType::File],
        };
        tree.set_filter(filter);

        tree.scan().await.unwrap();

        // Count all files to verify filter works
        let all_files = tree.all_files();
        let rust_files: Vec<_> = all_files
            .iter()
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
            .collect();

        // We should have some Rust files
        assert!(rust_files.len() > 0);
    }

    #[tokio::test]
    async fn test_ignore_patterns() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = FileTreeConfig {
            ignore_patterns: vec!["target".to_string()],
            ..FileTreeConfig::default()
        };
        let mut tree = FileTree::new(temp_dir.path().to_path_buf(), config);

        tree.scan().await.unwrap();

        let root = tree.root_node().unwrap();

        // Target directory should be marked as ignored
        if let Some(target_node) = root.children.iter().find(|n| n.name == "target") {
            assert!(target_node.ignored);
        }
    }

    #[tokio::test]
    async fn test_statistics() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = FileTreeConfig::default();
        let mut tree = FileTree::new(temp_dir.path().to_path_buf(), config);

        tree.scan().await.unwrap();

        let stats = tree.statistics();
        assert!(stats.total_files > 0);
        assert!(stats.total_directories > 0);
        assert!(stats.rust_files > 0);
        assert!(stats.total_size > 0);
    }

    #[tokio::test]
    async fn test_node_expansion() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = FileTreeConfig::default();
        let mut tree = FileTree::new(temp_dir.path().to_path_buf(), config);

        tree.scan().await.unwrap();

        let src_path = temp_dir.path().join("src");
        
        // Toggle expansion
        tree.toggle_expansion(&src_path).await.unwrap();
        
        // Check that the node is now expanded
        let root = tree.root_node().unwrap();
        let src_node = root.children.iter().find(|n| n.name == "src").unwrap();
        assert!(src_node.expanded);
    }

    #[tokio::test]
    async fn test_find_node() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = FileTreeConfig::default();
        let mut tree = FileTree::new(temp_dir.path().to_path_buf(), config);

        tree.scan().await.unwrap();

        let main_rs_path = temp_dir.path().join("src").join("main.rs");
        let found_node = tree.find_node(&main_rs_path);

        assert!(found_node.is_some());
        let node = found_node.unwrap();
        assert_eq!(node.name, "main.rs");
        assert!(node.metadata.is_rust_file);
    }

    #[test]
    fn test_utility_functions() {
        // Test file size formatting
        assert_eq!(utils::format_file_size(512), "512 B");
        assert_eq!(utils::format_file_size(1024), "1.0 KB");
        assert_eq!(utils::format_file_size(1048576), "1.0 MB");

        // Test breadcrumb components
        let root = Path::new("/project");
        let path = Path::new("/project/src/main.rs");
        let components = utils::get_breadcrumb_components(path, root);
        assert_eq!(components, vec!["src", "main.rs"]);
    }

    #[test]
    fn test_file_icon_selection() {
        let rust_file = FileNode {
            name: "main.rs".to_string(),
            path: PathBuf::from("main.rs"),
            relative_path: PathBuf::from("main.rs"),
            node_type: FileNodeType::File,
            size: Some(100),
            modified: None,
            expanded: false,
            children: Vec::new(),
            extension: Some("rs".to_string()),
            ignored: false,
            metadata: FileNodeMetadata {
                is_rust_file: true,
                ..FileNodeMetadata::default()
            },
        };

        assert_eq!(utils::get_file_icon(&rust_file), "🦀");

        let directory = FileNode {
            name: "src".to_string(),
            path: PathBuf::from("src"),
            relative_path: PathBuf::from("src"),
            node_type: FileNodeType::Directory,
            size: None,
            modified: None,
            expanded: false,
            children: Vec::new(),
            extension: None,
            ignored: false,
            metadata: FileNodeMetadata::default(),
        };

        assert_eq!(utils::get_file_icon(&directory), "📁");
    }
}