// src-tauri/src/project/templates.rs
//! Project templates for creating new Rust projects

use crate::project::{ProjectError, ProjectResult, TemplateConfig};
use crate::utils::paths::PathUtils;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;
use tracing::{debug, info, instrument, warn};

/// Types of project templates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateType {
    /// Simple binary application
    Binary,
    /// Library crate
    Library,
    /// Mixed binary and library
    Mixed,
    /// Cargo workspace
    Workspace,
    /// CLI application with clap
    CliApp,
    /// Web application with axum
    WebApp,
    /// WASM application
    WasmApp,
    /// Game with bevy
    Game,
    /// Procedural macro crate
    ProcMacro,
    /// Custom template
    Custom,
}

impl TemplateType {
    /// Get the display name for this template type
    pub fn display_name(&self) -> &'static str {
        match self {
            TemplateType::Binary => "Binary Application",
            TemplateType::Library => "Library Crate",
            TemplateType::Mixed => "Mixed Binary/Library",
            TemplateType::Workspace => "Cargo Workspace",
            TemplateType::CliApp => "CLI Application",
            TemplateType::WebApp => "Web Application",
            TemplateType::WasmApp => "WebAssembly Application",
            TemplateType::Game => "Game (Bevy)",
            TemplateType::ProcMacro => "Procedural Macro",
            TemplateType::Custom => "Custom Template",
        }
    }

    /// Get the description for this template type
    pub fn description(&self) -> &'static str {
        match self {
            TemplateType::Binary => "A simple Rust binary application with main.rs",
            TemplateType::Library => "A Rust library crate with lib.rs",
            TemplateType::Mixed => "A project with both binary and library components",
            TemplateType::Workspace => "A Cargo workspace for multi-package projects",
            TemplateType::CliApp => "Command-line application with argument parsing",
            TemplateType::WebApp => "Web server application using modern frameworks",
            TemplateType::WasmApp => "WebAssembly application for browser deployment",
            TemplateType::Game => "Game development setup with Bevy engine",
            TemplateType::ProcMacro => "Procedural macro crate for code generation",
            TemplateType::Custom => "User-defined custom template",
        }
    }
}

/// Template metadata and configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTemplate {
    /// Template name
    pub name: String,
    /// Template type
    pub template_type: TemplateType,
    /// Template description
    pub description: String,
    /// Required Rust version
    pub rust_version: Option<String>,
    /// Default dependencies
    pub dependencies: HashMap<String, String>,
    /// Default dev dependencies
    pub dev_dependencies: HashMap<String, String>,
    /// Build dependencies
    pub build_dependencies: HashMap<String, String>,
    /// Features to include
    pub features: HashMap<String, Vec<String>>,
    /// Files to create
    pub files: Vec<TemplateFile>,
    /// Directories to create
    pub directories: Vec<String>,
    /// Post-creation commands to run
    pub post_create_commands: Vec<String>,
    /// Template variables
    pub variables: HashMap<String, TemplateVariable>,
    /// Whether to initialize git repository
    pub init_git: bool,
}

/// Template file definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateFile {
    /// Relative path from project root
    pub path: String,
    /// File content (can contain template variables)
    pub content: String,
    /// Whether this file is executable
    pub executable: bool,
}

/// Template variable definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateVariable {
    /// Variable name
    pub name: String,
    /// Variable description
    pub description: String,
    /// Default value
    pub default: Option<String>,
    /// Whether this variable is required
    pub required: bool,
    /// Variable type
    pub var_type: VariableType,
}

/// Types of template variables
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VariableType {
    /// String value
    String,
    /// Boolean value
    Boolean,
    /// Integer value
    Integer,
    /// Selection from predefined options
    Choice { options: Vec<String> },
}

/// Template engine for creating projects from templates
pub struct TemplateEngine {
    /// Configuration
    config: TemplateConfig,
    /// Built-in templates
    builtin_templates: HashMap<TemplateType, ProjectTemplate>,
    /// Custom templates loaded from disk
    custom_templates: HashMap<String, ProjectTemplate>,
    /// Path utilities
    path_utils: PathUtils,
}

impl TemplateEngine {
    /// Create a new template engine
    pub fn new(config: TemplateConfig) -> Self {
        let mut engine = Self {
            config,
            builtin_templates: HashMap::new(),
            custom_templates: HashMap::new(),
            path_utils: PathUtils::new(),
        };

        engine.initialize_builtin_templates();
        engine
    }

    /// Initialize built-in templates
    fn initialize_builtin_templates(&mut self) {
        self.builtin_templates.insert(TemplateType::Binary, self.create_binary_template());
        self.builtin_templates.insert(TemplateType::Library, self.create_library_template());
        self.builtin_templates.insert(TemplateType::Mixed, self.create_mixed_template());
        self.builtin_templates.insert(TemplateType::Workspace, self.create_workspace_template());
        self.builtin_templates.insert(TemplateType::CliApp, self.create_cli_app_template());
        self.builtin_templates.insert(TemplateType::WebApp, self.create_web_app_template());
        self.builtin_templates.insert(TemplateType::WasmApp, self.create_wasm_app_template());
        self.builtin_templates.insert(TemplateType::Game, self.create_game_template());
        self.builtin_templates.insert(TemplateType::ProcMacro, self.create_proc_macro_template());
    }

    /// Create a new project from template
    #[instrument(skip(self, options))]
    pub async fn create_project(
        &self,
        template_type: TemplateType,
        name: &str,
        target_dir: &Path,
        options: HashMap<String, String>,
    ) -> Result<()> {
        info!("Creating project '{}' from template: {:?}", name, template_type);

        // Get the template
        let template = self.get_template(template_type)?;

        // Create project directory
        let project_dir = target_dir.join(name);
        if project_dir.exists() {
            return Err(anyhow::anyhow!("Directory already exists: {}", project_dir.display()));
        }

        fs::create_dir_all(&project_dir).await.context("Failed to create project directory")?;

        // Prepare template context
        let mut context = self.build_template_context(name, &template, options)?;
        
        // Create directories
        for dir_path in &template.directories {
            let full_path = project_dir.join(self.substitute_variables(dir_path, &context)?);
            fs::create_dir_all(&full_path).await.context("Failed to create directory")?;
            debug!("Created directory: {}", full_path.display());
        }

        // Create files
        for template_file in &template.files {
            let file_path = project_dir.join(self.substitute_variables(&template_file.path, &context)?);
            
            // Ensure parent directory exists
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).await.context("Failed to create parent directory")?;
            }

            // Substitute variables in content
            let content = self.substitute_variables(&template_file.content, &context)?;
            
            // Write file
            fs::write(&file_path, content).await.context("Failed to write file")?;
            
            // Set executable permissions if needed
            #[cfg(unix)]
            if template_file.executable {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&file_path).await?.permissions();
                perms.set_mode(perms.mode() | 0o755);
                fs::set_permissions(&file_path, perms).await?;
            }

            debug!("Created file: {}", file_path.display());
        }

        // Create Cargo.toml
        self.create_cargo_toml(&project_dir, name, &template, &context).await?;

        // Initialize git repository if requested
        if template.init_git && self.config.init_git {
            self.initialize_git_repository(&project_dir).await?;
        }

        // Run post-creation commands
        for command in &template.post_create_commands {
            let substituted_command = self.substitute_variables(command, &context)?;
            self.run_post_create_command(&project_dir, &substituted_command).await?;
        }

        info!("Successfully created project: {}", project_dir.display());
        Ok(())
    }

    /// Get a template by type
    fn get_template(&self, template_type: TemplateType) -> Result<&ProjectTemplate> {
        self.builtin_templates
            .get(&template_type)
            .ok_or_else(|| anyhow::anyhow!("Template not found: {:?}", template_type))
    }

    /// Build template context with variables
    fn build_template_context(
        &self,
        project_name: &str,
        template: &ProjectTemplate,
        mut options: HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut context = HashMap::new();

        // Built-in variables
        context.insert("project_name".to_string(), project_name.to_string());
        context.insert("project_name_snake".to_string(), self.to_snake_case(project_name));
        context.insert("project_name_kebab".to_string(), self.to_kebab_case(project_name));
        context.insert("project_name_pascal".to_string(), self.to_pascal_case(project_name));
        
        // Add author from config
        if let Some(author) = &self.config.default_author {
            context.insert("author".to_string(), author.clone());
        } else {
            context.insert("author".to_string(), "Your Name <your.email@example.com>".to_string());
        }

        // Add license from config
        if let Some(license) = &self.config.default_license {
            context.insert("license".to_string(), license.clone());
        }

        // Add current year
        let current_year = chrono::Utc::now().year();
        context.insert("year".to_string(), current_year.to_string());

        // Process template variables
        for (var_name, var_def) in &template.variables {
            let value = if let Some(provided_value) = options.remove(var_name) {
                provided_value
            } else if let Some(default_value) = &var_def.default {
                default_value.clone()
            } else if var_def.required {
                return Err(anyhow::anyhow!("Required variable '{}' not provided", var_name));
            } else {
                String::new()
            };

            context.insert(var_name.clone(), value);
        }

        Ok(context)
    }

    /// Substitute template variables in a string
    fn substitute_variables(&self, template: &str, context: &HashMap<String, String>) -> Result<String> {
        let mut result = template.to_string();

        for (key, value) in context {
            let placeholder = format!("{{{{{}}}}}", key);
            result = result.replace(&placeholder, value);
        }

        // Check for unresolved placeholders
        if result.contains("{{") && result.contains("}}") {
            warn!("Template contains unresolved placeholders: {}", result);
        }

        Ok(result)
    }

    /// Create Cargo.toml file
    async fn create_cargo_toml(
        &self,
        project_dir: &Path,
        name: &str,
        template: &ProjectTemplate,
        context: &HashMap<String, String>,
    ) -> Result<()> {
        let mut cargo_toml = String::new();

        // Package section
        cargo_toml.push_str("[package]\n");
        cargo_toml.push_str(&format!("name = \"{}\"\n", name));
        cargo_toml.push_str("version = \"0.1.0\"\n");
        cargo_toml.push_str("edition = \"2021\"\n");

        if let Some(author) = context.get("author") {
            cargo_toml.push_str(&format!("authors = [\"{}\"]\n", author));
        }

        if let Some(license) = context.get("license") {
            cargo_toml.push_str(&format!("license = \"{}\"\n", license));
        }

        if let Some(rust_version) = &template.rust_version {
            cargo_toml.push_str(&format!("rust-version = \"{}\"\n", rust_version));
        }

        cargo_toml.push('\n');

        // Dependencies
        if !template.dependencies.is_empty() {
            cargo_toml.push_str("[dependencies]\n");
            for (name, version) in &template.dependencies {
                cargo_toml.push_str(&format!("{} = \"{}\"\n", name, version));
            }
            cargo_toml.push('\n');
        }

        // Dev dependencies
        if !template.dev_dependencies.is_empty() {
            cargo_toml.push_str("[dev-dependencies]\n");
            for (name, version) in &template.dev_dependencies {
                cargo_toml.push_str(&format!("{} = \"{}\"\n", name, version));
            }
            cargo_toml.push('\n');
        }

        // Build dependencies
        if !template.build_dependencies.is_empty() {
            cargo_toml.push_str("[build-dependencies]\n");
            for (name, version) in &template.build_dependencies {
                cargo_toml.push_str(&format!("{} = \"{}\"\n", name, version));
            }
            cargo_toml.push('\n');
        }

        // Features
        if !template.features.is_empty() {
            cargo_toml.push_str("[features]\n");
            for (feature_name, feature_deps) in &template.features {
                if feature_deps.is_empty() {
                    cargo_toml.push_str(&format!("{} = []\n", feature_name));
                } else {
                    let deps_str = feature_deps
                        .iter()
                        .map(|dep| format!("\"{}\"", dep))
                        .collect::<Vec<_>>()
                        .join(", ");
                    cargo_toml.push_str(&format!("{} = [{}]\n", feature_name, deps_str));
                }
            }
        }

        let cargo_path = project_dir.join("Cargo.toml");
        fs::write(cargo_path, cargo_toml).await?;

        Ok(())
    }

    /// Initialize git repository
    async fn initialize_git_repository(&self, project_dir: &Path) -> Result<()> {
        debug!("Initializing git repository in {}", project_dir.display());

        let output = Command::new("git")
            .arg("init")
            .current_dir(project_dir)
            .output()
            .await?;

        if !output.status.success() {
            warn!("Failed to initialize git repository: {}", String::from_utf8_lossy(&output.stderr));
        } else {
            // Create .gitignore
            let gitignore_content = r#"# Generated by Cargo
# will have compiled files and executables
debug/
target/

# Remove Cargo.lock from gitignore if creating an executable, leave it for libraries
# More information here https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html
Cargo.lock

# These are backup files generated by rustfmt
**/*.rs.bk

# MSVC Windows builds of rustc generate these, which store debugging information
*.pdb
"#;
            let gitignore_path = project_dir.join(".gitignore");
            fs::write(gitignore_path, gitignore_content).await?;
        }

        Ok(())
    }

    /// Run a post-creation command
    async fn run_post_create_command(&self, project_dir: &Path, command: &str) -> Result<()> {
        debug!("Running post-create command: {}", command);

        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        let mut cmd = Command::new(parts[0]);
        cmd.current_dir(project_dir);
        
        if parts.len() > 1 {
            cmd.args(&parts[1..]);
        }

        let output = cmd.output().await?;

        if !output.status.success() {
            warn!(
                "Post-create command failed: {} - {}",
                command,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Convert string to snake_case
    fn to_snake_case(&self, s: &str) -> String {
        s.chars()
            .enumerate()
            .map(|(i, c)| {
                if c.is_uppercase() && i > 0 {
                    format!("_{}", c.to_lowercase())
                } else {
                    c.to_lowercase().to_string()
                }
            })
            .collect::<String>()
            .replace('-', "_")
            .replace(' ', "_")
    }

    /// Convert string to kebab-case
    fn to_kebab_case(&self, s: &str) -> String {
        self.to_snake_case(s).replace('_', "-")
    }

    /// Convert string to PascalCase
    fn to_pascal_case(&self, s: &str) -> String {
        s.split(&['-', '_', ' '][..])
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().chain(chars.as_str().to_lowercase().chars()).collect(),
                }
            })
            .collect()
    }

    /// Get available templates
    pub fn available_templates(&self) -> Vec<(TemplateType, &ProjectTemplate)> {
        self.builtin_templates
            .iter()
            .map(|(t, template)| (*t, template))
            .collect()
    }

    /// Load custom templates from directory
    pub async fn load_custom_templates(&mut self) -> Result<()> {
        if let Some(templates_dir) = &self.config.templates_dir {
            if templates_dir.exists() {
                self.scan_custom_templates(templates_dir).await?;
            }
        }
        Ok(())
    }

    /// Scan directory for custom templates
    async fn scan_custom_templates(&mut self, dir: &Path) -> Result<()> {
        let mut entries = fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let template_file = path.join("template.toml");
                if template_file.exists() {
                    match self.load_custom_template(&template_file).await {
                        Ok(template) => {
                            self.custom_templates.insert(template.name.clone(), template);
                        }
                        Err(e) => {
                            warn!("Failed to load custom template from {}: {}", template_file.display(), e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a custom template from file
    async fn load_custom_template(&self, template_file: &Path) -> Result<ProjectTemplate> {
        let content = fs::read_to_string(template_file).await?;
        let template: ProjectTemplate = toml::from_str(&content)?;
        Ok(template)
    }

    // Template creation methods
    fn create_binary_template(&self) -> ProjectTemplate {
        ProjectTemplate {
            name: "Binary Application".to_string(),
            template_type: TemplateType::Binary,
            description: "A simple Rust binary application".to_string(),
            rust_version: Some("1.70".to_string()),
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            build_dependencies: HashMap::new(),
            features: HashMap::new(),
            files: vec![
                TemplateFile {
                    path: "src/main.rs".to_string(),
                    content: r#"fn main() {
    println!("Hello, {{project_name}}!");
}
"#.to_string(),
                    executable: false,
                },
                TemplateFile {
                    path: "README.md".to_string(),
                    content: r#"# {{project_name}}

A Rust binary application.

## Usage

```bash
cargo run
```

## License

{{license}}
"#.to_string(),
                    executable: false,
                },
            ],
            directories: vec!["src".to_string()],
            post_create_commands: vec![],
            variables: HashMap::new(),
            init_git: true,
        }
    }

    fn create_web_app_template(&self) -> ProjectTemplate {
        let mut deps = HashMap::new();
        deps.insert("axum".to_string(), "0.7".to_string());
        deps.insert("tokio".to_string(), "{ version = \"1.0\", features = [\"full\"] }".to_string());
        deps.insert("serde".to_string(), "{ version = \"1.0\", features = [\"derive\"] }".to_string());
        deps.insert("serde_json".to_string(), "1.0".to_string());

        ProjectTemplate {
            name: "Web Application".to_string(),
            template_type: TemplateType::WebApp,
            description: "Web server application using Axum".to_string(),
            rust_version: Some("1.70".to_string()),
            dependencies: deps,
            dev_dependencies: HashMap::new(),
            build_dependencies: HashMap::new(),
            features: HashMap::new(),
            files: vec![
                TemplateFile {
                    path: "src/main.rs".to_string(),
                    content: r#"use axum::{
    routing::get,
    http::StatusCode,
    Json, Router,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct HelloResponse {
    message: String,
}

async fn hello() -> Json<HelloResponse> {
    Json(HelloResponse {
        message: "Hello from {{project_name}}!".to_string(),
    })
}

async fn health() -> StatusCode {
    StatusCode::OK
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(hello))
        .route("/health", get(health));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server running on http://localhost:3000");
    
    axum::serve(listener, app).await.unwrap();
}
"#.to_string(),
                    executable: false,
                },
                TemplateFile {
                    path: "README.md".to_string(),
                    content: r#"# {{project_name}}

A web application built with Rust and Axum.

## Usage

```bash
# Run the server
cargo run

# The server will be available at http://localhost:3000
```

## Endpoints

- `GET /` - Hello message
- `GET /health` - Health check

## License

{{license}}
"#.to_string(),
                    executable: false,
                },
            ],
            directories: vec!["src".to_string()],
            post_create_commands: vec![],
            variables: HashMap::new(),
            init_git: true,
        }
    }

    fn create_wasm_app_template(&self) -> ProjectTemplate {
        let mut deps = HashMap::new();
        deps.insert("wasm-bindgen".to_string(), "0.2".to_string());
        deps.insert("web-sys".to_string(), "0.3".to_string());

        let mut dev_deps = HashMap::new();
        dev_deps.insert("wasm-pack".to_string(), "0.12".to_string());

        ProjectTemplate {
            name: "WebAssembly Application".to_string(),
            template_type: TemplateType::WasmApp,
            description: "WebAssembly application for browser deployment".to_string(),
            rust_version: Some("1.70".to_string()),
            dependencies: deps,
            dev_dependencies: dev_deps,
            build_dependencies: HashMap::new(),
            features: HashMap::new(),
            files: vec![
                TemplateFile {
                    path: "src/lib.rs".to_string(),
                    content: r#"use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    fn alert(s: &str);
    
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[wasm_bindgen]
pub fn greet(name: &str) {
    alert(&format!("Hello, {}! From {{project_name}}", name));
}

#[wasm_bindgen(start)]
pub fn main() {
    log("{{project_name}} WASM module loaded!");
}
"#.to_string(),
                    executable: false,
                },
                TemplateFile {
                    path: "index.html".to_string(),
                    content: r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>{{project_name}}</title>
</head>
<body>
    <h1>{{project_name}}</h1>
    <button id="greet-button">Greet</button>
    
    <script type="module">
        import init, { greet } from './pkg/{{project_name_snake}}.js';
        
        async function run() {
            await init();
            
            document.getElementById('greet-button').addEventListener('click', () => {
                greet('WebAssembly');
            });
        }
        
        run();
    </script>
</body>
</html>
"#.to_string(),
                    executable: false,
                },
                TemplateFile {
                    path: "README.md".to_string(),
                    content: r#"# {{project_name}}

A WebAssembly application built with Rust.

## Building

```bash
# Install wasm-pack if you haven't already
cargo install wasm-pack

# Build the WASM package
wasm-pack build --target web

# Serve the files (you'll need a local server)
python -m http.server 8000
# or
npx serve .
```

Then open http://localhost:8000 in your browser.

## License

{{license}}
"#.to_string(),
                    executable: false,
                },
            ],
            directories: vec!["src".to_string()],
            post_create_commands: vec!["wasm-pack build --target web".to_string()],
            variables: HashMap::new(),
            init_git: true,
        }
    }

    fn create_game_template(&self) -> ProjectTemplate {
        let mut deps = HashMap::new();
        deps.insert("bevy".to_string(), "0.12".to_string());

        ProjectTemplate {
            name: "Game (Bevy)".to_string(),
            template_type: TemplateType::Game,
            description: "Game development setup with Bevy engine".to_string(),
            rust_version: Some("1.70".to_string()),
            dependencies: deps,
            dev_dependencies: HashMap::new(),
            build_dependencies: HashMap::new(),
            features: HashMap::new(),
            files: vec![
                TemplateFile {
                    path: "src/main.rs".to_string(),
                    content: r#"use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "{{project_name}}".into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, rotate_cube)
        .run();
}

#[derive(Component)]
struct RotatingCube;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Spawn a cube
    commands.spawn((
        PbrBundle {
            mesh: meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
            material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
            transform: Transform::from_xyz(0.0, 0.5, 0.0),
            ..default()
        },
        RotatingCube,
    ));

    // Light
    commands.spawn(PointLightBundle {
        point_light: PointLight {
            intensity: 1500.0,
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..default()
    });

    // Camera
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(-2.0, 2.5, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });
}

fn rotate_cube(mut query: Query<&mut Transform, With<RotatingCube>>, time: Res<Time>) {
    for mut transform in &mut query {
        transform.rotate_y(time.delta_seconds());
    }
}
"#.to_string(),
                    executable: false,
                },
                TemplateFile {
                    path: "README.md".to_string(),
                    content: r#"# {{project_name}}

A game built with Rust and Bevy.

## Usage

```bash
# Run the game
cargo run
```

## Controls

- Use your mouse to look around
- WASD to move (if you add movement controls)

## License

{{license}}
"#.to_string(),
                    executable: false,
                },
            ],
            directories: vec!["src".to_string(), "assets".to_string()],
            post_create_commands: vec![],
            variables: HashMap::new(),
            init_git: true,
        }
    }

    fn create_proc_macro_template(&self) -> ProjectTemplate {
        let mut deps = HashMap::new();
        deps.insert("proc-macro2".to_string(), "1.0".to_string());
        deps.insert("quote".to_string(), "1.0".to_string());
        deps.insert("syn".to_string(), "{ version = \"2.0\", features = [\"full\"] }".to_string());

        ProjectTemplate {
            name: "Procedural Macro".to_string(),
            template_type: TemplateType::ProcMacro,
            description: "Procedural macro crate for code generation".to_string(),
            rust_version: Some("1.70".to_string()),
            dependencies: deps,
            dev_dependencies: HashMap::new(),
            build_dependencies: HashMap::new(),
            features: HashMap::new(),
            files: vec![
                TemplateFile {
                    path: "src/lib.rs".to_string(),
                    content: r#"//! {{project_name}} procedural macros

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// A derive macro that generates a `hello` method
#[proc_macro_derive(Hello)]
pub fn hello_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        impl #name {
            pub fn hello(&self) -> String {
                format!("Hello from {}!", stringify!(#name))
            }
        }
    };

    TokenStream::from(expanded)
}

/// A function-like macro that creates a greeting
#[proc_macro]
pub fn make_greeting(input: TokenStream) -> TokenStream {
    let input = input.to_string();
    let name = input.trim_matches('"');
    
    let expanded = quote! {
        format!("Hello, {}! This greeting was generated by {{project_name}}", #name)
    };

    TokenStream::from(expanded)
}
"#.to_string(),
                    executable: false,
                },
                TemplateFile {
                    path: "tests/test.rs".to_string(),
                    content: r#"use {{project_name_snake}}::{Hello, make_greeting};

#[derive(Hello)]
struct TestStruct;

#[test]
fn test_hello_derive() {
    let test = TestStruct;
    assert_eq!(test.hello(), "Hello from TestStruct!");
}

#[test]
fn test_make_greeting() {
    let greeting = make_greeting!("World");
    assert!(greeting.contains("Hello, World!"));
}
"#.to_string(),
                    executable: false,
                },
                TemplateFile {
                    path: "README.md".to_string(),
                    content: r#"# {{project_name}}

A procedural macro crate for Rust.

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
{{project_name}} = "0.1.0"
```

Then use the macros:

```rust
use {{project_name_snake}}::{Hello, make_greeting};

#[derive(Hello)]
struct MyStruct;

fn main() {
    let my_struct = MyStruct;
    println!("{}", my_struct.hello());
    
    let greeting = make_greeting!("World");
    println!("{}", greeting);
}
```

## License

{{license}}
"#.to_string(),
                    executable: false,
                },
            ],
            directories: vec!["src".to_string(), "tests".to_string()],
            post_create_commands: vec![],
            variables: HashMap::new(),
            init_git: true,
        }
    }
}

/// Utility functions for template operations
pub mod utils {
    use super::*;

    /// Validate a project name
    pub fn validate_project_name(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(anyhow::anyhow!("Project name cannot be empty"));
        }

        if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(anyhow::anyhow!(
                "Project name can only contain alphanumeric characters, hyphens, and underscores"
            ));
        }

        if name.starts_with('-') || name.ends_with('-') {
            return Err(anyhow::anyhow!("Project name cannot start or end with a hyphen"));
        }

        if name.len() > 64 {
            return Err(anyhow::anyhow!("Project name is too long (max 64 characters)"));
        }

        // Check against Rust keywords
        const RUST_KEYWORDS: &[&str] = &[
            "as", "break", "const", "continue", "crate", "else", "enum", "extern",
            "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
            "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
            "static", "struct", "super", "trait", "true", "type", "unsafe",
            "use", "where", "while", "async", "await", "dyn",
        ];

        if RUST_KEYWORDS.contains(&name) {
            return Err(anyhow::anyhow!("Project name cannot be a Rust keyword"));
        }

        Ok(())
    }

    /// Get template by name
    pub fn find_template_by_name(
        engine: &TemplateEngine,
        name: &str,
    ) -> Option<(TemplateType, &ProjectTemplate)> {
        engine
            .available_templates()
            .into_iter()
            .find(|(_, template)| template.name.to_lowercase() == name.to_lowercase())
    }

    /// List all available template types
    pub fn list_template_types() -> Vec<TemplateType> {
        vec![
            TemplateType::Binary,
            TemplateType::Library,
            TemplateType::Mixed,
            TemplateType::Workspace,
            TemplateType::CliApp,
            TemplateType::WebApp,
            TemplateType::WasmApp,
            TemplateType::Game,
            TemplateType::ProcMacro,
        ]
    }

    /// Create a template context from user input
    pub fn create_template_context(
        project_name: &str,
        template: &ProjectTemplate,
        user_variables: HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut context = HashMap::new();

        // Add project name variants
        context.insert("project_name".to_string(), project_name.to_string());
        
        // Add current date/time
        let now = chrono::Utc::now();
        context.insert("year".to_string(), now.year().to_string());
        context.insert("date".to_string(), now.format("%Y-%m-%d").to_string());

        // Add template variables
        for (var_name, var_def) in &template.variables {
            if let Some(user_value) = user_variables.get(var_name) {
                context.insert(var_name.clone(), user_value.clone());
            } else if let Some(default_value) = &var_def.default {
                context.insert(var_name.clone(), default_value.clone());
            } else if var_def.required {
                return Err(anyhow::anyhow!("Required variable '{}' not provided", var_name));
            }
        }

        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_template_types() {
        assert_eq!(TemplateType::Binary.display_name(), "Binary Application");
        assert_eq!(TemplateType::Library.display_name(), "Library Crate");
        assert_eq!(TemplateType::WebApp.display_name(), "Web Application");
    }

    #[test]
    fn test_case_conversions() {
        let engine = TemplateEngine::new(TemplateConfig::default());
        
        assert_eq!(engine.to_snake_case("MyProject"), "my_project");
        assert_eq!(engine.to_kebab_case("MyProject"), "my-project");
        assert_eq!(engine.to_pascal_case("my-project"), "MyProject");
        assert_eq!(engine.to_pascal_case("my_project"), "MyProject");
    }

    #[test]
    fn test_template_variable_substitution() {
        let engine = TemplateEngine::new(TemplateConfig::default());
        let mut context = HashMap::new();
        context.insert("project_name".to_string(), "test-project".to_string());
        context.insert("author".to_string(), "Test Author".to_string());

        let template = "Hello {{project_name}} by {{author}}!";
        let result = engine.substitute_variables(template, &context).unwrap();
        
        assert_eq!(result, "Hello test-project by Test Author!");
    }

    #[tokio::test]
    async fn test_create_binary_project() {
        let temp_dir = TempDir::new().unwrap();
        let config = TemplateConfig::default();
        let engine = TemplateEngine::new(config);

        let result = engine
            .create_project(
                TemplateType::Binary,
                "test-project",
                temp_dir.path(),
                HashMap::new(),
            )
            .await;

        assert!(result.is_ok());

        let project_dir = temp_dir.path().join("test-project");
        assert!(project_dir.exists());
        assert!(project_dir.join("Cargo.toml").exists());
        assert!(project_dir.join("src").join("main.rs").exists());
        assert!(project_dir.join("README.md").exists());
    }

    #[tokio::test]
    async fn test_create_library_project() {
        let temp_dir = TempDir::new().unwrap();
        let config = TemplateConfig::default();
        let engine = TemplateEngine::new(config);

        let result = engine
            .create_project(
                TemplateType::Library,
                "test-lib",
                temp_dir.path(),
                HashMap::new(),
            )
            .await;

        assert!(result.is_ok());

        let project_dir = temp_dir.path().join("test-lib");
        assert!(project_dir.exists());
        assert!(project_dir.join("Cargo.toml").exists());
        assert!(project_dir.join("src").join("lib.rs").exists());

        // Check that lib.rs contains the expected content
        let lib_content = fs::read_to_string(project_dir.join("src").join("lib.rs")).await.unwrap();
        assert!(lib_content.contains("test-lib"));
    }

    #[tokio::test]
    async fn test_create_workspace_project() {
        let temp_dir = TempDir::new().unwrap();
        let config = TemplateConfig::default();
        let engine = TemplateEngine::new(config);

        let result = engine
            .create_project(
                TemplateType::Workspace,
                "test-workspace",
                temp_dir.path(),
                HashMap::new(),
            )
            .await;

        assert!(result.is_ok());

        let project_dir = temp_dir.path().join("test-workspace");
        assert!(project_dir.exists());
        assert!(project_dir.join("Cargo.toml").exists());
        assert!(project_dir.join("test_workspace-lib").exists());
        assert!(project_dir.join("test_workspace-cli").exists());
    }

    #[test]
    fn test_project_name_validation() {
        assert!(utils::validate_project_name("valid-name").is_ok());
        assert!(utils::validate_project_name("valid_name").is_ok());
        assert!(utils::validate_project_name("ValidName123").is_ok());

        assert!(utils::validate_project_name("").is_err());
        assert!(utils::validate_project_name("invalid name").is_err());
        assert!(utils::validate_project_name("-invalid").is_err());
        assert!(utils::validate_project_name("invalid-").is_err());
        assert!(utils::validate_project_name("fn").is_err()); // Rust keyword
    }

    #[test]
    fn test_available_templates() {
        let config = TemplateConfig::default();
        let engine = TemplateEngine::new(config);
        let templates = engine.available_templates();

        assert!(!templates.is_empty());
        assert!(templates.iter().any(|(t, _)| *t == TemplateType::Binary));
        assert!(templates.iter().any(|(t, _)| *t == TemplateType::Library));
        assert!(templates.iter().any(|(t, _)| *t == TemplateType::WebApp));
    }

    #[test]
    fn test_template_context_building() {
        let config = TemplateConfig {
            default_author: Some("Test Author".to_string()),
            default_license: Some("MIT".to_string()),
            ..TemplateConfig::default()
        };
        let engine = TemplateEngine::new(config);
        let template = engine.create_binary_template();

        let context = engine
            .build_template_context("my-project", &template, HashMap::new())
            .unwrap();

        assert_eq!(context.get("project_name"), Some(&"my-project".to_string()));
        assert_eq!(context.get("project_name_snake"), Some(&"my_project".to_string()));
        assert_eq!(context.get("project_name_kebab"), Some(&"my-project".to_string()));
        assert_eq!(context.get("project_name_pascal"), Some(&"MyProject".to_string()));
        assert_eq!(context.get("author"), Some(&"Test Author".to_string()));
        assert_eq!(context.get("license"), Some(&"MIT".to_string()));
    }

    #[test]
    fn test_template_serialization() {
        let template = ProjectTemplate {
            name: "Test Template".to_string(),
            template_type: TemplateType::Binary,
            description: "A test template".to_string(),
            rust_version: Some("1.70".to_string()),
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            build_dependencies: HashMap::new(),
            features: HashMap::new(),
            files: Vec::new(),
            directories: Vec::new(),
            post_create_commands: Vec::new(),
            variables: HashMap::new(),
            init_git: true,
        };

        let serialized = serde_json::to_string(&template).unwrap();
        let deserialized: ProjectTemplate = serde_json::from_str(&serialized).unwrap();

        assert_eq!(template.name, deserialized.name);
        assert_eq!(template.template_type, deserialized.template_type);
    }
}