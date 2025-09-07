use crate::project::{ProjectError, ProjectResult};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, instrument, warn};

/// Parsed Cargo.toml manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoManifest {
    /// Package metadata
    pub package: PackageMetadata,
    /// Dependencies
    pub dependencies: HashMap<String, Dependency>,
    /// Development dependencies
    pub dev_dependencies: HashMap<String, Dependency>,
    /// Build dependencies
    pub build_dependencies: HashMap<String, Dependency>,
    /// Target-specific dependencies
    pub target_dependencies: HashMap<String, HashMap<String, Dependency>>,
    /// Binary targets
    pub bins: Vec<BinaryTarget>,
    /// Library configuration
    pub lib: Option<LibraryTarget>,
    /// Example targets
    pub examples: Vec<ExampleTarget>,
    /// Test targets
    pub tests: Vec<TestTarget>,
    /// Benchmark targets
    pub benches: Vec<BenchTarget>,
    /// Features
    pub features: HashMap<String, Vec<String>>,
    /// Workspace configuration (if present)
    pub workspace: Option<WorkspaceConfig>,
    /// Build script configuration
    pub build: Option<String>,
    /// Package metadata
    pub metadata: Option<toml::Value>,
    /// Raw TOML content for custom fields
    pub raw_toml: toml::Value,
}

/// Package metadata from [package] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// Package name
    pub name: String,
    /// Package version
    pub version: String,
    /// Rust edition
    pub edition: String,
    /// Package description
    pub description: Option<String>,
    /// Authors
    pub authors: Vec<String>,
    /// License
    pub license: Option<String>,
    /// License file
    pub license_file: Option<String>,
    /// Repository URL
    pub repository: Option<String>,
    /// Homepage URL
    pub homepage: Option<String>,
    /// Documentation URL
    pub documentation: Option<String>,
    /// Keywords
    pub keywords: Vec<String>,
    /// Categories
    pub categories: Vec<String>,
    /// Whether to publish to crates.io
    pub publish: bool,
    /// Minimum Rust version
    pub rust_version: Option<String>,
    /// Include/exclude patterns
    pub include: Vec<String>,
    /// Exclude patterns
    pub exclude: Vec<String>,
}

/// Dependency specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Dependency name
    pub name: String,
    /// Version requirement
    pub version: Option<String>,
    /// Git repository
    pub git: Option<String>,
    /// Git branch
    pub branch: Option<String>,
    /// Git tag
    pub tag: Option<String>,
    /// Git revision
    pub rev: Option<String>,
    /// Path dependency
    pub path: Option<PathBuf>,
    /// Registry
    pub registry: Option<String>,
    /// Features to enable
    pub features: Vec<String>,
    /// Whether it's optional
    pub optional: bool,
    /// Default features
    pub default_features: bool,
    /// Package name (if different from key)
    pub package: Option<String>,
}

/// Binary target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryTarget {
    /// Binary name
    pub name: String,
    /// Path to source file
    pub path: Option<PathBuf>,
    /// Required features
    pub required_features: Vec<String>,
    /// Edition
    pub edition: Option<String>,
}

/// Library target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryTarget {
    /// Library name
    pub name: Option<String>,
    /// Path to source file
    pub path: Option<PathBuf>,
    /// Crate types
    pub crate_type: Vec<String>,
    /// Required features
    pub required_features: Vec<String>,
    /// Edition
    pub edition: Option<String>,
    /// Procedural macro
    pub proc_macro: bool,
}

/// Example target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleTarget {
    /// Example name
    pub name: String,
    /// Path to source file
    pub path: Option<PathBuf>,
    /// Required features
    pub required_features: Vec<String>,
    /// Edition
    pub edition: Option<String>,
    /// Crate type
    pub crate_type: Vec<String>,
}

/// Test target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestTarget {
    /// Test name
    pub name: String,
    /// Path to source file
    pub path: Option<PathBuf>,
    /// Required features
    pub required_features: Vec<String>,
    /// Edition
    pub edition: Option<String>,
    /// Harness
    pub harness: bool,
}

/// Benchmark target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchTarget {
    /// Benchmark name
    pub name: String,
    /// Path to source file
    pub path: Option<PathBuf>,
    /// Required features
    pub required_features: Vec<String>,
    /// Edition
    pub edition: Option<String>,
    /// Harness
    pub harness: bool,
}

/// Workspace configuration from [workspace] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Workspace members
    pub members: Vec<String>,
    /// Excluded members
    pub exclude: Vec<String>,
    /// Default members
    pub default_members: Vec<String>,
    /// Workspace dependencies
    pub dependencies: HashMap<String, Dependency>,
    /// Workspace metadata
    pub metadata: Option<toml::Value>,
    /// Resolver version
    pub resolver: Option<String>,
}

/// Manifest parser for Cargo.toml files
pub struct ManifestParser {
    /// Cache of parsed manifests
    cache: std::sync::RwLock<HashMap<PathBuf, (CargoManifest, std::time::SystemTime)>>,
}

impl ManifestParser {
    /// Create a new manifest parser
    pub fn new() -> Self {
        Self {
            cache: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Parse a Cargo.toml manifest from a project dir
    #[instrument(skip(self))]
    pub async fn parse_manifest(&self, project_path: &Path) -> ProjectResult<CargoManifest> {
        let manifest_path = project_path.join("Cargo.toml");

        if !manifest_path.exists() {
            return Err(ProjectError::ManifestNotFound {
                path: project_path.to_string_lossy().to_string(),
            });
        }

        if let Some(cached) = self.get_cached_manifest(&manifest_path).await? {
            debug!("Using cached manifest for {}", manifest_path.display());
            return Ok(cached);
        }

        let content =
            fs::read_to_string(&manifest_path)
                .await
                .map_err(|e| ProjectError::FileSystemError {
                    message: format!("Failed to read {}: {}", manifest_path.display(), e),
                });

        let manifest = self.parse_manifest_content(&content, &manifest_path)?;

        self.cache_manifest(&manifest_path, manifest.clone())
            .await?;

        debug!("Parsed manifest for {}", project_path.display());
        Ok(manifest)
    }

    /// Parse manifest content from string
    pub fn parse_manifest_content(
        &self,
        content: &str,
        manifest_path: &Path,
    ) -> ProjectResult<CargoManifest> {
        let raw_toml: toml::Value =
            toml::from_str(content).map_err(|e| ProjectError::InvalidManifest {
                message: format!("Failed to parse TOML: {}", e),
            })?;

        // Parse package section
        let package = self.parse_package_section(&raw_toml)?;

        // Parse dependencies
        let dependencies = self.parse_dependencies_section(&raw_toml, "dependencies")?;
        let dev_dependencies = self.parse_dependencies_section(&raw_toml, "dev-dependencies")?;
        let build_dependencies =
            self.parse_dependencies_section(&raw_toml, "build-dependencies")?;

        // Parse target-specific dependencies
        let target_dependencies = self.parse_target_dependencies(&raw_toml)?;

        // Parse targets
        let bins = self.parse_binary_targets(&raw_toml)?;
        let lib = self.parse_library_target(&raw_toml)?;
        let examples = self.parse_example_targets(&raw_toml)?;
        let tests = self.parse_test_targets(&raw_toml)?;
        let benches = self.parse_bench_targets(&raw_toml)?;

        // Parse features
        let features = self.parse_features(&raw_toml)?;

        // Parse workspace
        let workspace = self.parse_workspace_section(&raw_toml)?;

        // Parse build script
        let build = raw_toml
            .get("package")
            .and_then(|p| p.get("build"))
            .and_then(|b| b.as_str())
            .map(|s| s.to_string());

        // Extract metadata
        let metadata = raw_toml
            .get("package")
            .and_then(|p| p.get("metadata"))
            .cloned();

        Ok(CargoManifest {
            package,
            dependencies,
            dev_dependencies,
            build_dependencies,
            target_dependencies,
            bins,
            lib,
            examples,
            tests,
            benches,
            features,
            workspace,
            build,
            metadata,
            raw_toml,
        })
    }

    /// Get cached manifest if available and up to date
    async fn get_cached_manifest(&self, manifest_path: &Path) -> Result<Option<CargoManifest>> {
        let cache = self.cache.read().unwrap();

        if let some((manifest, cached_time)) = cache.get(manifest_path) {
            if let Ok(metadata) = fs::metadata(manifest_path).await {
                if let Ok(modified) = metadata.modified() {
                    if modified <= *cached_time {
                        return Ok(Some(manifest.clone()));
                    }
                }
            }
        }

        Ok(())
    }

    /// Cache a parsed manifest
    async fn cache_manifest(&self, manifest_path: &Path, manifest: CargoManifest) -> Result<()> {
        let mut cache = self.cache.write().unwrap();
        let current_time = std::time::SystemTime::now();
        cache.insert(manifest_path.to_path_buf(), (manifest, current_time));
        Ok(())
    }

    /// Parse the [package] section
    fn parse_package_section(&self, toml: &toml::Value) -> ProjectResult<PackageMetadata> {
        let package_section = toml
            .get("package")
            .ok_or_else(|| ProjectError::InvalidManifest {
                message: "No [package] section found".to_string(),
            })?;

        let name = package_section
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| ProjectError::InvalidManifest {
                message: "Package name is required".to_string(),
            })?
            .to_string();

        let version = package_section
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProjectError::InvalidManifest {
                message: "Package version is required".to_string(),
            })?
            .to_string();

        let edition = package_section
            .get("edition")
            .and_then(|e| e.as_str())
            .unwrap_or("2021")
            .to_string();

        let description = package_section
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        let authors = package_section
            .get("authors")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let license = package_section
            .get("license")
            .and_then(|l| l.as_str())
            .map(|s| s.to_string());

        let license_file = package_section
            .get("license-file")
            .and_then(|l| l.as_str())
            .map(|s| s.to_string());

        let repository = package_section
            .get("repository")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());

        let homepage = package_section
            .get("homepage")
            .and_then(|h| h.as_str())
            .map(|s| s.to_string());

        let documentation = package_section
            .get("documentation")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());

        let keywords = package_section
            .get("keywords")
            .and_then(|k| k.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let categories = package_section
            .get("categories")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let publish = package_section
            .get("publish")
            .and_then(|p| p.as_bool())
            .unwrap_or(true);

        let rust_version = package_section
            .get("rust-version")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());

        let include = package_section
            .get("include")
            .and_then(|i| i.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let exclude = package_section
            .get("exclude")
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        Ok(PackageMetadata {
            name,
            version,
            edition,
            description,
            authors,
            license,
            license_file,
            repository,
            homepage,
            documentation,
            keywords,
            categories,
            publish,
            rust_version,
            include,
            exclude,
        })
    }

    /// Parse the dependencies section
    fn parse_dependencies_section(
        &self,
        toml: &toml::Value,
        section_name: &str,
    ) -> ProjectResult<HashMap<String, Dependency>> {
        let mut dependencies = HashMap::new();

        if let Some(deps_section) = toml.get(section_name) {
            if let Some(deps_table) = deps_section.as_table() {
                for (name, value) in deps_table {
                    let dependency = self.parse_dependency(name, value)?;
                    dependencies.insert(name.clone(), dependency);
                }
            }
        }

        Ok(())
    }

    /// Parse a single dependency
    fn parse_dependency(&self, name: &str, value: &toml::Value) -> ProjectResult<Dependency> {
        let mut dependency = Dependency {
            name: name.to_string(),
            version: None,
            git: None,
            branch: None,
            tag: None,
            rev: None,
            path: None,
            registry: None,
            features: Vec::new(),
            optional: false,
            default_features: true,
            package: None,
        };

        match value {
            toml::Value::String(version) => {
                dependency.version = Some(version.clone());
            }
            toml::Value::Table(table) => {
                if let Some(version) = table.get("version").and_them(|v| v.as_str()) {
                    dependency.version = Some(version.to_string());
                }

                if let Some(git) = table.get("git").and_then(|g| g.as_str()) {
                    dependency.git = Some(git.to_string());
                }

                if let Some(branch) = table.get("branch").and_then(|b| b.as_str()) {
                    dependency.branch = Some(branch.to_string());
                }

                if let Some(tag) = table.get("tag").and_then(|t| t.as_str()) {
                    dependency.tag = Some(tag.to_string());
                }

                if let Some(rev) = table.get("rev").and_then(|r| r.as_str()) {
                    dependency.rev = Some(rev.to_string());
                }

                if let Some(path) = table.get("path").and_then(|p| p.as_str()) {
                    dependency.path = Some(PathBuf::from(path));
                }

                if let Some(registry) = table.get("registry").and_then(|r| r.as_str()) {
                    dependency.registry = Some(registry.to_string());
                }

                if let Some(features) = table.get("features").and_then(|f| f.as_array()) {
                    dependency.features = features
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect();
                }

                if let Some(optional) = table.get("optional").and_then(|o| o.as_bool()) {
                    dependency.optional = optional;
                }

                if let Some(default_features) =
                    table.get("default-features").and_then(|d| d.as_bool())
                {
                    dependency.default_features = default_features;
                }

                if let Some(package) = table.get("package").and_then(|p| p.as_str()) {
                    dependency.package = Some(package.to_string());
                }
            }
            _ => {
                return Err(ProjectError::InvalidManifest {
                    message: format!("Invalid dependency format for {}", name),
                });
            }
        }

        Ok(dependency)
    }

    /// Parse target-specific dependencies
    fn parse_target_dependencies(
        &self,
        toml: &toml::Value,
    ) -> ProjectResult<HashMap<String, HashMap<String, Dependency>>> {
        let mut target_dependencies = HashMap::new();

        if let Some(target_section) = toml.get("target") {
            if let Some(target_table) = target_section.as_table() {
                for (target_name, target_value) in target_table {
                    if let Some(target_deps) = target_value.get("dependencies") {
                        if let Some(deps_table) = target_deps.as_table() {
                            let mut deps = HashMap::new();
                            for (name, value) in deps_table {
                                let dependency = self.parse_dependency(name, value)?;
                                deps.insert(name.clone(), dependency);
                            }
                            target_dependencies.insert(target_name.clone(), deps);
                        }
                    }
                }
            }
        }

        Ok(target_dependencies)
    }

    /// Parse binary targets
    fn parse_binary_targets(&self, toml: &toml::Value) -> ProjectResult<Vec<BinaryTarget>> {
        let mut targets = Vec::new();

        if let Some(bin_section) = toml.get("bin") {
            if let Some(bin_array) = bin_section.as_array() {
                for bin_value in bin_array {
                    if let Some(bin_table) = bin_value.as_table() {
                        let target = self.parse_binary_targets(bin_table)?;
                        targets.push(target);
                    }
                }
            }
        }

        Ok(targets)
    }

    /// Parse a single binary target
    fn parse_binary_target(
        &self,
        table: &toml::map::Map<String, toml::Value>,
    ) -> ProjectResult<BinaryTarget> {
        let name = table
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| ProjectError::InvalidManifest {
                message: "Binary target name is required".to_string(),
            })?
            .to_string();

        let path = table
            .get("path")
            .and_then(|p| p.as_str())
            .map(PathBuf::from);

        let required_features = table
            .get("required-features")
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let edition = table
            .get("edition")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string());

        Ok(BinaryTarget {
            name,
            path,
            required_features,
            edition,
        })
    }

    /// Parse library target
    fn parse_library_target(&self, toml: &toml::Value) -> ProjectResult<Option<LibraryTarget>> {
        if let Some(lib_section) = toml.get("lib") {
            if let Some(lib_table) = lib_section.as_table() {
                let name = lib_table
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string());

                let path = lib_table
                    .get("path")
                    .and_then(|p| p.as_str())
                    .map(PathBuf::from);

                let crate_type = lib_table
                    .get("crate-type")
                    .and_then(|c| c.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["rlib".to_string()]);

                let required_features = lib_table
                    .get("required-features")
                    .and_then(|f| f.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                let edition = lib_table
                    .get("edition")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string());

                let proc_macro = lib_table
                    .get("proc-macro")
                    .and_then(|p| p.as_bool())
                    .unwrap_or(false);

                return Ok(Some(LibraryTarget {
                    name,
                    path,
                    crate_type,
                    required_features,
                    edition,
                    proc_macro,
                }));
            }
        }

        Ok(None)
    }

    /// Parse example targets
    fn parse_example_targets(&self, toml: &toml::Value) -> ProjectResult<Vec<ExampleTarget>> {
        let mut targets = Vec::new();

        if let Some(example_section) = toml.get("example") {
            if let Some(example_array) = example_section.as_array() {
                for example_value in example_array {
                    if let Some(example_table) = example_value.as_table() {
                        let target = self.parse_example_target(example_table)?;
                        targets.push(target);
                    }
                }
            }
        }

        Ok(targets)
    }

    /// Parse a single example target
    fn parse_example_target(
        &self,
        table: &toml::map::Map<String, toml::Value>,
    ) -> ProjectResult<ExampleTarget> {
        let name = table
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| ProjectError::InvalidManifest {
                message: "Example target name is required".to_string(),
            })?
            .to_string();

        let path = table
            .get("path")
            .and_then(|p| p.as_str())
            .map(PathBuf::from);

        let required_features = table
            .get("required-features")
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let edition = table
            .get("edition")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string());

        let crate_type = table
            .get("crate-type")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_else(|| vec!["bin".to_string()]);

        Ok(ExampleTarget {
            name,
            path,
            required_features,
            edition,
            crate_type,
        })
    }

    /// Parse test targets
    fn parse_test_targets(&self, toml: &toml::Value) -> ProjectResult<Vec<TestTarget>> {
        let mut targets = Vec::new();

        if let Some(test_section) = toml.get("test") {
            if let Some(test_array) = test_section.as_array() {
                for test_value in test_array {
                    if let Some(test_table) = test_value.as_table() {
                        let target = self.parse_test_target(test_table)?;
                        targets.push(target);
                    }
                }
            }
        }

        Ok(targets)
    }

    /// Parse a single test target
    fn parse_test_target(
        &self,
        table: &toml::map::Map<String, toml::Value>,
    ) -> ProjectResult<TestTarget> {
        let name = table
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| ProjectError::InvalidManifest {
                message: "Test target name is required".to_string(),
            })?
            .to_string();

        let path = table
            .get("path")
            .and_then(|p| p.as_str())
            .map(PathBuf::from);

        let required_features = table
            .get("required-features")
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let edition = table
            .get("edition")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string());

        let harness = table
            .get("harness")
            .and_then(|h| h.as_bool())
            .unwrap_or(true);

        Ok(TestTarget {
            name,
            path,
            required_features,
            edition,
            harness,
        })
    }

    /// Parse benchmark targets
    fn parse_bench_targets(&self, toml: &toml::Value) -> ProjectResult<Vec<BenchTarget>> {
        let mut targets = Vec::new();

        if let Some(bench_section) = toml.get("bench") {
            if let Some(bench_array) = bench_section.as_array() {
                for bench_value in bench_array {
                    if let Some(bench_table) = bench_value.as_table() {
                        let target = self.parse_bench_target(bench_table)?;
                        targets.push(target);
                    }
                }
            }
        }

        Ok(targets)
    }

    /// Parse a single benchmark target
    fn parse_bench_target(
        &self,
        table: &toml::map::Map<String, toml::Value>,
    ) -> ProjectResult<BenchTarget> {
        let name = table
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or_else(|| ProjectError::InvalidManifest {
                message: "Benchmark target name is required".to_string(),
            })?
            .to_string();

        let path = table
            .get("path")
            .and_then(|p| p.as_str())
            .map(PathBuf::from);

        let required_features = table
            .get("required-features")
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let edition = table
            .get("edition")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string());

        let harness = table
            .get("harness")
            .and_then(|h| h.as_bool())
            .unwrap_or(true);

        Ok(BenchTarget {
            name,
            path,
            required_features,
            edition,
            harness,
        })
    }

    /// Parse features section
    fn parse_features(&self, toml: &toml::Value) -> ProjectResult<HashMap<String, Vec<String>>> {
        let mut features = HashMap::new();

        if let Some(features_section) = toml.get("features") {
            if let Some(features_table) = features_section.as_table() {
                for (feature_name, feature_value) in features_table {
                    let dependencies = if let Some(deps_array) = feature_value.as_array() {
                        deps_array
                            .iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    } else {
                        Vec::new()
                    };

                    features.insert(feature_name.clone(), dependencies);
                }
            }
        }

        Ok(features)
    }

    /// Parse workspace section
    fn parse_workspace_section(
        &self,
        toml: &toml::Value,
    ) -> ProjectResult<Option<WorkspaceConfig>> {
        if let Some(workspace_section) = toml.get("workspace") {
            if let Some(workspace_table) = workspace_section.as_table() {
                let members = workspace_table
                    .get("members")
                    .and_then(|m| m.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                let exclude = workspace_table
                    .get("exclude")
                    .and_then(|e| e.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                let default_members = workspace_table
                    .get("default-members")
                    .and_then(|d| d.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                let dependencies = workspace_table
                    .get("dependencies")
                    .and_then(|d| d.as_table())
                    .map(|deps_table| {
                        let mut deps = HashMap::new();
                        for (name, value) in deps_table {
                            if let Ok(dependency) = self.parse_dependency(name, value) {
                                deps.insert(name.clone(), dependency);
                            }
                        }
                        deps
                    })
                    .unwrap_or_default();

                let metadata = workspace_table.get("metadata").cloned();

                let resolver = workspace_table
                    .get("resolver")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string());

                return Ok(Some(WorkspaceConfig {
                    members,
                    exclude,
                    default_members,
                    dependencies,
                    metadata,
                    resolver,
                }));
            }
        }

        Ok(None)
    }

    /// Clear the manifest cache
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
    }

    /// Get cache stats
    pub fn cache_stats(&self) -> (usize, usize) {
        let cache = self.cache.read().unwrap();
        (cache.len(), cache.capacity())
    }
}

impl Default for ManifestParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Utility functions for working with manifests
pub mod utils {
    use cargo_metadata::CargoOpt;

    use super::*;

    /// Check if a manifest represents a workspace
    pub fn is_workspace_manifest(manifest: &CargoManifest) -> bool {
        manifest.workspace.is_some()
    }

    /// Get all dependency names from a manifest
    pub fn get_all_dependency_names(manifest: &CargoManifest) -> Vec<String> {
        let mut names = Vec::new();

        names.extend(manifest.dependencies.keys().cloned());
        names.extend(manifest.dev_dependencies.keys().cloned());
        names.extend(manifest.build_dependencies.keys().cloned());

        for target_deps in manifest.target_dependencies.values() {
            names.extend(target_deps.keys().cloned());
        }

        names.sort();
        names.dedup();
        names
    }

    /// Get external dependencies
    pub fn get_external_dependencies(manifest: &CargoManifest) -> Vec<&Dependency> {
        manifest
            .dependencies
            .values()
            .filter(|dep| dep.path.is_none())
            .collect()
    }

    /// Get local dependencies
    pub fn get_local_dependencies(manifest: &CargoManifest) -> Vec<&Dependency> {
        manifest
            .dependencies
            .values()
            .filter(|dep| dep.path.is_some())
            .collect()
    }

    /// Get git dependencies
    pub fn get_git_dependencies(manifest: &CargoManifest) -> Vec<&Dependency> {
        manifest
            .dependencies
            .values()
            .filter(|dep| dep.git.is_some())
            .collect()
    }

    /// Check if a dependency is optional
    pub fn is_optional_dependency(manifest: &CargoManifest, name: &str) -> bool {
        manifest
            .dependencies
            .get(name)
            .map(|dep| dep.optional)
            .unwrap_or(false)
    }

    /// Get features that enable a specific dependency
    pub fn get_features_enabling_dependency(
        manifest: &CargoManifest,
        dep_name: &str,
    ) -> Vec<String> {
        manifest
            .features
            .iter()
            .filter_map(|(feature_name, feature_deps)| {
                if feature_deps.contains(&dep_name.to_string()) {
                    Some(feature_name.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_parse_simple_manifest() {
        let manifest_content = r#"
[package]
name = "test-package"
version = "0.1.0"
edition = "2021"
description = "A test package"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[dependencies]
serde = "1.0"
tokio = { version = "1.0", features = ["full"] }
"#;

        let parser = ManifestParser::new();
        let manifest = parser
            .parse_manifest_content(manifest_content, &PathBuf::from("Cargo.toml"))
            .unwrap();

        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "0.1.0");
        assert_eq!(manifest.package.edition, "2021");
        assert_eq!(manifest.dependencies.len(), 2);
        assert!(manifest.dependencies.contains_key("serde"));
        assert!(manifest.dependencies.contains_key("tokio"));
    }

    #[tokio::test]
    async fn test_parse_workspace_manifest() {
        let manifest_content = r#"
[workspace]
members = ["app", "lib"]
exclude = ["old-stuff"]

[workspace.dependencies]
serde = "1.0"
"#;

        let parser = ManifestParser::new();
        let manifest = parser
            .parse_manifest_content(manifest_content, &PathBuf::from("Cargo.toml"))
            .unwrap();

        assert!(manifest.workspace.is_some());
        let workspace = manifest.workspace.unwrap();
        assert_eq!(workspace.members, vec!["app", "lib"]);
        assert_eq!(workspace.exclude, vec!["old-stuff"]);
        assert!(workspace.dependencies.contains_key("serde"));
    }

    #[tokio::test]
    async fn test_parse_complex_dependencies() {
        let manifest_content = r#"
[package]
name = "test"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1.0", features = ["derive"], optional = true }
local-crate = { path = "../local" }
git-crate = { git = "https://github.com/example/repo", branch = "main" }

[dev-dependencies]
criterion = "0.3"

[build-dependencies]
cc = "1.0"
"#;

        let parser = ManifestParser::new();
        let manifest = parser
            .parse_manifest_content(manifest_content, &PathBuf::from("Cargo.toml"))
            .unwrap();

        // Check regular dependencies
        let serde_dep = manifest.dependencies.get("serde").unwrap();
        assert_eq!(serde_dep.version, Some("1.0".to_string()));
        assert!(serde_dep.features.contains(&"derive".to_string()));
        assert!(serde_dep.optional);

        let local_dep = manifest.dependencies.get("local-crate").unwrap();
        assert_eq!(local_dep.path, Some(PathBuf::from("../local")));

        let git_dep = manifest.dependencies.get("git-crate").unwrap();
        assert_eq!(
            git_dep.git,
            Some("https://github.com/example/repo".to_string())
        );
        assert_eq!(git_dep.branch, Some("main".to_string()));

        // Check dev dependencies
        assert!(manifest.dev_dependencies.contains_key("criterion"));

        // Check build dependencies
        assert!(manifest.build_dependencies.contains_key("cc"));
    }

    #[tokio::test]
    async fn test_parse_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("Cargo.toml");

        let manifest_content = r#"
[package]
name = "file-test"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1.0"
"#;

        fs::write(&manifest_path, manifest_content).await.unwrap();

        let parser = ManifestParser::new();
        let manifest = parser.parse_manifest(temp_dir.path()).await.unwrap();

        assert_eq!(manifest.package.name, "file-test");
        assert!(manifest.dependencies.contains_key("serde"));
    }

    #[test]
    fn test_utility_functions() {
        let manifest_content = r#"
[package]
name = "test"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1.0", optional = true }
local = { path = "../local" }
git-dep = { git = "https://github.com/example/repo" }

[features]
default = ["serde"]
full = ["serde", "extra"]
"#;

        let parser = ManifestParser::new();
        let manifest = parser
            .parse_manifest_content(manifest_content, &PathBuf::from("Cargo.toml"))
            .unwrap();

        // Test dependency utilities
        let all_deps = utils::get_all_dependency_names(&manifest);
        assert!(all_deps.contains(&"serde".to_string()));

        let external_deps = utils::get_external_dependencies(&manifest);
        assert_eq!(external_deps.len(), 2); // serde and git-dep

        let local_deps = utils::get_local_dependencies(&manifest);
        assert_eq!(local_deps.len(), 1); // local

        let git_deps = utils::get_git_dependencies(&manifest);
        assert_eq!(git_deps.len(), 1); // git-dep

        // Test optional dependency check
        assert!(utils::is_optional_dependency(&manifest, "serde"));

        // Test feature utilities
        let features = utils::get_features_enabling_dependency(&manifest, "serde");
        assert!(features.contains(&"default".to_string()));
        assert!(features.contains(&"full".to_string()));
    }

    #[tokio::test]
    async fn test_cache_functionality() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("Cargo.toml");

        let manifest_content = r#"
[package]
name = "cache-test"
version = "0.1.0"
edition = "2021"
"#;

        fs::write(&manifest_path, manifest_content).await.unwrap();

        let parser = ManifestParser::new();

        // First parse - should cache
        let manifest1 = parser.parse_manifest(temp_dir.path()).await.unwrap();
        assert_eq!(manifest1.package.name, "cache-test");

        let (cache_size, _) = parser.cache_stats();
        assert_eq!(cache_size, 1);

        // Second parse - should use cache
        let manifest2 = parser.parse_manifest(temp_dir.path()).await.unwrap();
        assert_eq!(manifest2.package.name, "cache-test");

        // Cache should still have one entry
        let (cache_size, _) = parser.cache_stats();
        assert_eq!(cache_size, 1);

        // Clear cache
        parser.clear_cache();
        let (cache_size, _) = parser.cache_stats();
        assert_eq!(cache_size, 0);
    }

    #[test]
    fn test_error_handling() {
        let parser = ManifestParser::new();

        // Test invalid TOML
        let invalid_toml = "invalid toml content [[[";
        let result = parser.parse_manifest_content(invalid_toml, &PathBuf::from("Cargo.toml"));
        assert!(result.is_err());

        // Test missing package section
        let no_package = r#"
[dependencies]
serde = "1.0"
"#;
        let result = parser.parse_manifest_content(no_package, &PathBuf::from("Cargo.toml"));
        assert!(result.is_err());

        // Test missing name
        let no_name = r#"
[package]
version = "0.1.0"
"#;
        let result = parser.parse_manifest_content(no_name, &PathBuf::from("Cargo.toml"));
        assert!(result.is_err());
    }
}
