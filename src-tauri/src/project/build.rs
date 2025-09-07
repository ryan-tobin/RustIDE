use crate::project::{ProjectError, ProjectResult};
use crate::utils::process_manager::{ProcessManager, ProcessOutput};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime};
use tokio::process::Command;
use tracing::{debug, info, instrument, warn};

/// Build configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Cargo executable path
    pub cargo_path: String,
    /// Default cargo profile
    pub profile: String,
    /// Target triple
    pub target: Option<String>,
    /// Additional features to enable
    pub features: Vec<String>,
    /// Whether to build all features
    pub all_features: bool,
    /// Whether to disable default features
    pub no_default_features: bool,
    /// Number of parallel jobs
    pub jobs: Option<usize>,
    /// Verbose output
    pub verbose: bool,
    /// Environment variables
    pub env_vars: HashMap<String, String>,
    /// Working directory override
    pub working_dir: Option<PathBuf>,
    /// Timeout for build operations
    pub timeout: Duration,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            cargo_path: "cargo".to_string(),
            profile: "dev".to_string(),
            target: None,
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            jobs: None,
            verbose: false,
            env_vars: HashMap::new(),
            working_dir: None,
            timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Build target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildTarget {
    /// Target name
    pub name: String,
    /// Target kind (bin, lib, example, test, bench)
    pub kind: BuildTargetKind,
    /// Source path
    pub src_path: PathBuf,
    /// Required features
    pub required_features: Vec<String>,
    /// Whether this target is enabled
    pub enabled: bool,
}

/// Types of build targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildTargetKind {
    /// Binary executable
    Binary,
    /// Library
    Library,
    /// Example
    Example,
    /// Test
    Test,
    /// Benchmark
    Benchmark,
    /// Custom target
    Custom,
}

impl BuildTargetKind {
    /// Get the cargo command argument for this target kind
    pub fn cargo_arg(&self) -> &'static str {
        match self {
            BuildTargetKind::Binary => "--bin",
            BuildTargetKind::Library => "--lib",
            BuildTargetKind::Example => "--example",
            BuildTargetKind::Test => "--test",
            BuildTargetKind::Benchmark => "--bench",
            BuildTargetKind::Custom => "",
        }
    }
}

/// Build status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BuildStatus {
    /// Build not started
    NotStarted,
    /// Build in progress
    InProgress,
    /// Build completed successfully
    Success,
    /// Build failed
    Failed,
    /// Build was cancelled
    Cancelled,
}

/// Build output containing results and diagnostics
#[derive(Debug, Clone, Serialize)]
pub struct BuildOutput {
    /// Build status
    pub status: BuildStatus,
    /// Exit code
    pub exit_code: Option<i32>,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Build duration
    pub duration: Duration,
    /// Parsed diagnostics
    pub diagnostics: Vec<Diagnostic>,
    /// Build artifacts produced
    pub artifacts: Vec<BuildArtifact>,
    /// Timestamp when build started
    pub started_at: SystemTime,
    /// Timestamp when build finished
    pub finished_at: Option<SystemTime>,
}

/// Build diagnostic (error, warning, etc.)
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    /// Diagnostic level
    pub level: DiagnosticLevel,
    /// Error/warning message
    pub message: String,
    /// Source file path
    pub file_path: Option<PathBuf>,
    /// Line number
    pub line: Option<usize>,
    /// Column number
    pub column: Option<usize>,
    /// Span information
    pub span: Option<DiagnosticSpan>,
    /// Error code (e.g., E0308)
    pub code: Option<String>,
    /// Suggested fixes
    pub suggestions: Vec<String>,
}

/// Diagnostic severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticLevel {
    /// Error that prevents compilation
    Error,
    /// Warning that doesn't prevent compilation
    Warning,
    /// Informational note
    Note,
    /// Help message
    Help,
}

/// Diagnostic span information
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticSpan {
    /// Start line (1-indexed)
    pub start_line: usize,
    /// Start column (1-indexed)
    pub start_column: usize,
    /// End line (1-indexed)
    pub end_line: usize,
    /// End column (1-indexed)
    pub end_column: usize,
    /// Highlighted text
    pub text: String,
}

/// Build artifact information
#[derive(Debug, Clone, Serialize)]
pub struct BuildArtifact {
    /// Artifact name
    pub name: String,
    /// Path to the artifact
    pub path: PathBuf,
    /// Artifact type
    pub artifact_type: ArtifactType,
    /// File size
    pub size: u64,
    /// Whether this is the main target
    pub is_main: bool,
}

/// Types of build artifacts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ArtifactType {
    /// Executable binary
    Executable,
    /// Static library
    StaticLib,
    /// Dynamic library
    DynamicLib,
    /// Rust library (rlib)
    RustLib,
    /// C dynamic library
    CDylib,
    /// Procedural macro library
    ProcMacro,
    /// Test executable
    TestExecutable,
    /// Benchmark executable
    BenchExecutable,
    /// Example executable
    ExampleExecutable,
}

/// Build operation types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BuildOperation {
    /// Standard build
    Build,
    /// Clean build (remove artifacts first)
    Clean,
    /// Run tests
    Test,
    /// Run benchmarks
    Bench,
    /// Check syntax without building
    Check,
    /// Build documentation
    Doc,
    /// Run clippy lints
    Clippy,
    /// Format code
    Fmt,
    /// Run the main binary
    Run,
}

impl BuildOperation {
    /// Get the cargo subcommand for this operation
    pub fn cargo_subcommand(&self) -> &'static str {
        match self {
            BuildOperation::Build => "build",
            BuildOperation::Clean => "clean",
            BuildOperation::Test => "test",
            BuildOperation::Bench => "bench",
            BuildOperation::Check => "check",
            BuildOperation::Doc => "doc",
            BuildOperation::Clippy => "clippy",
            BuildOperation::Fmt => "fmt",
            BuildOperation::Run => "run",
        }
    }
}

/// Build manager for handling cargo operations
pub struct BuildManager {
    /// Project root path
    project_path: PathBuf,
    /// Build configuration
    config: BuildConfig,
    /// Process manager for running cargo commands
    process_manager: ProcessManager,
    /// Current build status
    current_status: BuildStatus,
    /// Last build output
    last_build: Option<BuildOutput>,
    /// Build history
    build_history: Vec<BuildOutput>,
    /// Maximum history entries to keep
    max_history: usize,
}

impl BuildManager {
    /// Create a new build manager
    pub fn new(project_path: PathBuf, config: BuildConfig) -> Self {
        Self {
            project_path,
            config,
            process_manager: ProcessManager::new(),
            current_status: BuildStatus::NotStarted,
            last_build: None,
            build_history: Vec::new(),
            max_history: 50,
        }
    }

    /// Execute a build operation
    #[instrument(skip(self))]
    pub async fn execute_build(
        &mut self,
        operation: BuildOperation,
        targets: Option<Vec<String>>,
    ) -> ProjectResult<BuildOutput> {
        info!("Starting build operation: {:?}", operation);

        self.current_status = BuildStatus::InProgress;
        let started_at = SystemTime::now();

        // Build cargo command
        let mut cmd = self.build_cargo_command(operation, targets)?;

        // Execute the command
        let result = self.process_manager
            .execute_command_with_timeout(&mut cmd, self.config.timeout)
            .await;

        let finished_at = SystemTime::now();
        let duration = finished_at.duration_since(started_at).unwrap_or_default();

        let build_output = match result {
            Ok(output) => {
                let success = output.status.success();
                self.current_status = if success {
                    BuildStatus::Success
                } else {
                    BuildStatus::Failed
                };

                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                let diagnostics = self.parse_diagnostics(&stderr, &stdout)?;
                let artifacts = self.find_build_artifacts(operation).await?;

                BuildOutput {
                    status: self.current_status,
                    exit_code: output.status.code(),
                    stdout,
                    stderr,
                    duration,
                    diagnostics,
                    artifacts,
                    started_at,
                    finished_at: Some(finished_at),
                }
            }
            Err(e) => {
                self.current_status = BuildStatus::Failed;
                warn!("Build operation failed: {}", e);

                BuildOutput {
                    status: BuildStatus::Failed,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("Build failed: {}", e),
                    duration,
                    diagnostics: Vec::new(),
                    artifacts: Vec::new(),
                    started_at,
                    finished_at: Some(finished_at),
                }
            }
        };

        // Store the build output
        self.last_build = Some(build_output.clone());
        self.add_to_history(build_output.clone());

        info!(
            "Build operation completed with status: {:?} in {:?}",
            build_output.status, build_output.duration
        );

        Ok(build_output)
    }

    /// Build a cargo command for the given operation
    fn build_cargo_command(
        &self,
        operation: BuildOperation,
        targets: Option<Vec<String>>,
    ) -> ProjectResult<Command> {
        let mut cmd = Command::new(&self.config.cargo_path);

        // Set working directory
        cmd.current_dir(&self.project_path);

        // Add subcommand
        cmd.arg(operation.cargo_subcommand());

        // Add profile
        if operation != BuildOperation::Clean && operation != BuildOperation::Fmt {
            if self.config.profile == "release" {
                cmd.arg("--release");
            }
        }

        // Add target
        if let Some(target) = &self.config.target {
            cmd.args(&["--target", target]);
        }

        // Add features
        if !self.config.features.is_empty() {
            cmd.arg("--features");
            cmd.arg(self.config.features.join(","));
        }

        if self.config.all_features {
            cmd.arg("--all-features");
        }

        if self.config.no_default_features {
            cmd.arg("--no-default-features");
        }

        // Add parallel jobs
        if let Some(jobs) = self.config.jobs {
            cmd.args(&["--jobs", &jobs.to_string()]);
        }

        // Add verbosity
        if self.config.verbose {
            cmd.arg("--verbose");
        }

        // Add specific targets
        if let Some(targets) = targets {
            for target in targets {
                cmd.args(&["--bin", &target]);
            }
        }

        // Add operation-specific arguments
        match operation {
            BuildOperation::Doc => {
                cmd.arg("--no-deps");
            }
            BuildOperation::Test => {
                cmd.arg("--");
                cmd.arg("--nocapture");
            }
            BuildOperation::Clippy => {
                cmd.arg("--");
                cmd.arg("-D");
                cmd.arg("warnings");
            }
            _ => {}
        }

        // Set environment variables
        for (key, value) in &self.config.env_vars {
            cmd.env(key, value);
        }

        // Configure stdio
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!("Built cargo command: {:?}", cmd);
        Ok(cmd)
    }

    /// Parse diagnostics from cargo output
    fn parse_diagnostics(&self, stderr: &str, stdout: &str) -> ProjectResult<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();

        // Parse stderr for compiler diagnostics
        for line in stderr.lines() {
            if let Some(diagnostic) = self.parse_diagnostic_line(line) {
                diagnostics.push(diagnostic);
            }
        }

        // Parse stdout for additional information
        for line in stdout.lines() {
            if let Some(diagnostic) = self.parse_diagnostic_line(line) {
                diagnostics.push(diagnostic);
            }
        }

        // Sort diagnostics by severity and location
        diagnostics.sort_by(|a, b| {
            use std::cmp::Ordering;
            
            // First by severity (errors first)
            let severity_order = |level: DiagnosticLevel| match level {
                DiagnosticLevel::Error => 0,
                DiagnosticLevel::Warning => 1,
                DiagnosticLevel::Note => 2,
                DiagnosticLevel::Help => 3,
            };

            match severity_order(a.level).cmp(&severity_order(b.level)) {
                Ordering::Equal => {
                    // Then by file path and line number
                    match (&a.file_path, &b.file_path) {
                        (Some(a_path), Some(b_path)) => {
                            match a_path.cmp(b_path) {
                                Ordering::Equal => a.line.cmp(&b.line),
                                other => other,
                            }
                        }
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => Ordering::Equal,
                    }
                }
                other => other,
            }
        });

        Ok(diagnostics)
    }

    /// Parse a single diagnostic line
    fn parse_diagnostic_line(&self, line: &str) -> Option<Diagnostic> {
        // This is a simplified parser. In a real implementation, you'd want to
        // parse JSON output from cargo with --message-format=json
        
        if line.contains("error:") {
            let message = line.replace("error:", "").trim().to_string();
            Some(Diagnostic {
                level: DiagnosticLevel::Error,
                message,
                file_path: None,
                line: None,
                column: None,
                span: None,
                code: None,
                suggestions: Vec::new(),
            })
        } else if line.contains("warning:") {
            let message = line.replace("warning:", "").trim().to_string();
            Some(Diagnostic {
                level: DiagnosticLevel::Warning,
                message,
                file_path: None,
                line: None,
                column: None,
                span: None,
                code: None,
                suggestions: Vec::new(),
            })
        } else {
            None
        }
    }

    /// Find build artifacts after a build
    async fn find_build_artifacts(&self, operation: BuildOperation) -> ProjectResult<Vec<BuildArtifact>> {
        let mut artifacts = Vec::new();

        // Only look for artifacts for build operations that produce them
        if !matches!(operation, BuildOperation::Build | BuildOperation::Test | BuildOperation::Bench) {
            return Ok(artifacts);
        }

        let target_dir = self.project_path.join("target");
        let profile_dir = target_dir.join(&self.config.profile);

        if !profile_dir.exists() {
            return Ok(artifacts);
        }

        // Look for executables
        if let Ok(mut entries) = tokio::fs::read_dir(&profile_dir).await {
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                
                if path.is_file() {
                    if let Some(artifact) = self.classify_artifact(&path).await? {
                        artifacts.push(artifact);
                    }
                }
            }
        }

        // Look in deps directory for libraries
        let deps_dir = profile_dir.join("deps");
        if deps_dir.exists() {
            if let Ok(mut entries) = tokio::fs::read_dir(&deps_dir).await {
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();
                    
                    if path.is_file() {
                        if let Some(artifact) = self.classify_artifact(&path).await? {
                            artifacts.push(artifact);
                        }
                    }
                }
            }
        }

        Ok(artifacts)
    }

    /// Classify a file as a build artifact
    async fn classify_artifact(&self, path: &Path) -> Result<Option<BuildArtifact>> {
        let metadata = tokio::fs::metadata(path).await?;
        
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let extension = path.extension().and_then(|ext| ext.to_str());

        let artifact_type = match extension {
            Some("exe") | None if cfg!(windows) => {
                // On Windows, executables have .exe extension or no extension
                if self.is_executable_file(path).await {
                    ArtifactType::Executable
                } else {
                    return Ok(None);
                }
            }
            None if cfg!(unix) => {
                // On Unix, executables typically have no extension
                if self.is_executable_file(path).await {
                    ArtifactType::Executable
                } else {
                    return Ok(None);
                }
            }
            Some("rlib") => ArtifactType::RustLib,
            Some("so") | Some("dylib") => ArtifactType::DynamicLib,
            Some("a") => ArtifactType::StaticLib,
            _ => return Ok(None),
        };

        Ok(Some(BuildArtifact {
            name,
            path: path.to_path_buf(),
            artifact_type,
            size: metadata.len(),
            is_main: false, // TODO: Determine if this is the main target
        }))
    }

    /// Check if a file is executable
    async fn is_executable_file(&self, path: &Path) -> bool {
        // Simple heuristic: check if file has execute permissions (Unix)
        // or if it's in the target directory with the right pattern
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = tokio::fs::metadata(path).await {
                let permissions = metadata.permissions();
                return permissions.mode() & 0o111 != 0;
            }
        }

        #[cfg(windows)]
        {
            return path.extension().and_then(|ext| ext.to_str()) == Some("exe");
        }

        #[cfg(not(any(unix, windows)))]
        {
            false
        }
    }

    /// Add build output to history
    fn add_to_history(&mut self, output: BuildOutput) {
        self.build_history.push(output);
        
        // Keep only the most recent builds
        while self.build_history.len() > self.max_history {
            self.build_history.remove(0);
        }
    }

    /// Cancel the current build operation
    pub async fn cancel_build(&mut self) -> ProjectResult<()> {
        if self.current_status == BuildStatus::InProgress {
            self.process_manager.kill_all_processes().await?;
            self.current_status = BuildStatus::Cancelled;
            info!("Build operation cancelled");
        }
        Ok(())
    }

    /// Get current build status
    pub fn current_status(&self) -> BuildStatus {
        self.current_status
    }

    /// Get last build output
    pub fn last_build(&self) -> Option<&BuildOutput> {
        self.last_build.as_ref()
    }

    /// Get build history
    pub fn build_history(&self) -> &[BuildOutput] {
        &self.build_history
    }

    /// Get last build time
    pub fn last_build_time(&self) -> Option<SystemTime> {
        self.last_build.as_ref().map(|b| b.started_at)
    }

    /// Clear build history
    pub fn clear_history(&mut self) {
        self.build_history.clear();
        self.last_build = None;
    }

    /// Update build configuration
    pub fn set_config(&mut self, config: BuildConfig) {
        self.config = config;
    }

    /// Get build configuration
    pub fn config(&self) -> &BuildConfig {
        &self.config
    }

    /// Get project path
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    /// Quick check operation (fast syntax check)
    pub async fn quick_check(&mut self) -> ProjectResult<BuildOutput> {
        self.execute_build(BuildOperation::Check, None).await
    }

    /// Run tests
    pub async fn run_tests(&mut self, test_names: Option<Vec<String>>) -> ProjectResult<BuildOutput> {
        self.execute_build(BuildOperation::Test, test_names).await
    }

    /// Run clippy lints
    pub async fn run_clippy(&mut self) -> ProjectResult<BuildOutput> {
        self.execute_build(BuildOperation::Clippy, None).await
    }

    /// Format code
    pub async fn format_code(&mut self) -> ProjectResult<BuildOutput> {
        self.execute_build(BuildOperation::Fmt, None).await
    }

    /// Clean build artifacts
    pub async fn clean(&mut self) -> ProjectResult<BuildOutput> {
        self.execute_build(BuildOperation::Clean, None).await
    }

    /// Build documentation
    pub async fn build_docs(&mut self) -> ProjectResult<BuildOutput> {
        self.execute_build(BuildOperation::Doc, None).await
    }

    /// Run the main binary
    pub async fn run_binary(&mut self, binary_name: Option<String>) -> ProjectResult<BuildOutput> {
        let targets = binary_name.map(|name| vec![name]);
        self.execute_build(BuildOperation::Run, targets).await
    }
}

/// Utility functions for build management
pub mod utils {
    use super::*;

    /// Parse cargo metadata to get available targets
    pub async fn get_available_targets(project_path: &Path) -> Result<Vec<BuildTarget>> {
        let mut cmd = Command::new("cargo");
        cmd.current_dir(project_path);
        cmd.args(&["metadata", "--format-version", "1", "--no-deps"]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to get cargo metadata"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let metadata: serde_json::Value = serde_json::from_str(&stdout)?;

        let mut targets = Vec::new();

        if let Some(packages) = metadata["packages"].as_array() {
            for package in packages {
                if let Some(package_targets) = package["targets"].as_array() {
                    for target in package_targets {
                        if let Some(target_info) = parse_target_info(target) {
                            targets.push(target_info);
                        }
                    }
                }
            }
        }

        Ok(targets)
    }

    /// Parse target information from cargo metadata
    fn parse_target_info(target: &serde_json::Value) -> Option<BuildTarget> {
        let name = target["name"].as_str()?.to_string();
        let src_path = PathBuf::from(target["src_path"].as_str()?);
        
        let kinds = target["kind"].as_array()?;
        let kind = if kinds.iter().any(|k| k.as_str() == Some("bin")) {
            BuildTargetKind::Binary
        } else if kinds.iter().any(|k| k.as_str() == Some("lib")) {
            BuildTargetKind::Library
        } else if kinds.iter().any(|k| k.as_str() == Some("example")) {
            BuildTargetKind::Example
        } else if kinds.iter().any(|k| k.as_str() == Some("test")) {
            BuildTargetKind::Test
        } else if kinds.iter().any(|k| k.as_str() == Some("bench")) {
            BuildTargetKind::Benchmark
        } else {
            BuildTargetKind::Custom
        };

        let required_features = target["required-features"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        Some(BuildTarget {
            name,
            kind,
            src_path,
            required_features,
            enabled: true,
        })
    }

    /// Get cargo version
    pub async fn get_cargo_version() -> Result<String> {
        let output = Command::new("cargo")
            .arg("--version")
            .output()
            .await?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(anyhow::anyhow!("Failed to get cargo version"))
        }
    }

    /// Check if cargo is available
    pub async fn is_cargo_available() -> bool {
        Command::new("cargo")
            .arg("--version")
            .output()
            .await
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Format build duration for display
    pub fn format_duration(duration: Duration) -> String {
        let seconds = duration.as_secs();
        let millis = duration.subsec_millis();

        if seconds >= 60 {
            let minutes = seconds / 60;
            let remaining_seconds = seconds % 60;
            format!("{}m {}s", minutes, remaining_seconds)
        } else if seconds > 0 {
            format!("{}.{}s", seconds, millis / 100)
        } else {
            format!("{}ms", millis)
        }
    }

    /// Get diagnostic count by level
    pub fn count_diagnostics_by_level(diagnostics: &[Diagnostic]) -> HashMap<DiagnosticLevel, usize> {
        let mut counts = HashMap::new();
        
        for diagnostic in diagnostics {
            *counts.entry(diagnostic.level).or_insert(0) += 1;
        }

        counts
    }
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
    async fn test_build_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = BuildConfig::default();
        let manager = BuildManager::new(temp_dir.path().to_path_buf(), config);

        assert_eq!(manager.current_status(), BuildStatus::NotStarted);
        assert!(manager.last_build().is_none());
        assert_eq!(manager.build_history().len(), 0);
    }

    #[tokio::test]
    #[ignore] // Requires cargo to be installed
    async fn test_cargo_check() {
        let temp_dir = TempDir::new().unwrap();
        create_test_project(temp_dir.path()).await.unwrap();

        let config = BuildConfig::default();
        let mut manager = BuildManager::new(temp_dir.path().to_path_buf(), config);

        let result = manager.quick_check().await;
        
        // This test might fail if cargo is not available
        if utils::is_cargo_available().await {
            assert!(result.is_ok());
            let output = result.unwrap();
            assert!(matches!(output.status, BuildStatus::Success | BuildStatus::Failed));
        }
    }

    #[test]
    fn test_build_config_defaults() {
        let config = BuildConfig::default();
        assert_eq!(config.cargo_path, "cargo");
        assert_eq!(config.profile, "dev");
        assert_eq!(config.timeout, Duration::from_secs(300));
        assert!(!config.verbose);
        assert!(!config.all_features);
    }

    #[test]
    fn test_diagnostic_parsing() {
        let config = BuildConfig::default();
        let manager = BuildManager::new(PathBuf::from("/tmp"), config);

        let stderr = "error: unused variable: `x`\nwarning: function is never used";
        let stdout = "";

        let diagnostics = manager.parse_diagnostics(stderr, stdout).unwrap();
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].level, DiagnosticLevel::Error);
        assert_eq!(diagnostics[1].level, DiagnosticLevel::Warning);
    }

    #[test]
    fn test_build_operation_cargo_subcommand() {
        assert_eq!(BuildOperation::Build.cargo_subcommand(), "build");
        assert_eq!(BuildOperation::Test.cargo_subcommand(), "test");
        assert_eq!(BuildOperation::Clean.cargo_subcommand(), "clean");
        assert_eq!(BuildOperation::Check.cargo_subcommand(), "check");
    }

    #[test]
    fn test_build_target_kind_cargo_arg() {
        assert_eq!(BuildTargetKind::Binary.cargo_arg(), "--bin");
        assert_eq!(BuildTargetKind::Library.cargo_arg(), "--lib");
        assert_eq!(BuildTargetKind::Example.cargo_arg(), "--example");
        assert_eq!(BuildTargetKind::Test.cargo_arg(), "--test");
    }

    #[test]
    fn test_utility_functions() {
        // Test duration formatting
        assert_eq!(utils::format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(utils::format_duration(Duration::from_secs(30)), "30.0s");
        assert_eq!(utils::format_duration(Duration::from_secs(90)), "1m 30s");

        // Test diagnostic counting
        let diagnostics = vec![
            Diagnostic {
                level: DiagnosticLevel::Error,
                message: "test error".to_string(),
                file_path: None,
                line: None,
                column: None,
                span: None,
                code: None,
                suggestions: Vec::new(),
            },
            Diagnostic {
                level: DiagnosticLevel::Warning,
                message: "test warning".to_string(),
                file_path: None,
                line: None,
                column: None,
                span: None,
                code: None,
                suggestions: Vec::new(),
            },
            Diagnostic {
                level: DiagnosticLevel::Error,
                message: "another error".to_string(),
                file_path: None,
                line: None,
                column: None,
                span: None,
                code: None,
                suggestions: Vec::new(),
            },
        ];

        let counts = utils::count_diagnostics_by_level(&diagnostics);
        assert_eq!(counts[&DiagnosticLevel::Error], 2);
        assert_eq!(counts[&DiagnosticLevel::Warning], 1);
    }

    #[tokio::test]
    async fn test_build_output_serialization() {
        let output = BuildOutput {
            status: BuildStatus::Success,
            exit_code: Some(0),
            stdout: "Build succeeded".to_string(),
            stderr: String::new(),
            duration: Duration::from_secs(10),
            diagnostics: Vec::new(),
            artifacts: Vec::new(),
            started_at: SystemTime::now(),
            finished_at: Some(SystemTime::now()),
        };

        let serialized = serde_json::to_string(&output).unwrap();
        assert!(serialized.contains("Success"));
        assert!(serialized.contains("Build succeeded"));
    }
}