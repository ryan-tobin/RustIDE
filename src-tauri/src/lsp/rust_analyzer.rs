use crate::lsp::{LspResult, LspError, LspServerInfo, LspServerStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tower_lsp::lsp_types::*;
use uuid::Uuid;

/// Rust-analyzer specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustAnalyzerConfig {
    /// Enable cargo check on save
    pub check_on_save: bool,
    
    /// Cargo check command (check, clippy, test)
    pub check_command: String,
    
    /// Additional cargo check arguments
    pub check_args: Vec<String>,
    
    /// Enable proc macro support
    pub proc_macro_enable: bool,
    
    /// Proc macro server path
    pub proc_macro_server: Option<String>,
    
    /// Enable inlay hints
    pub inlay_hints: InlayHintsConfig,
    
    /// Completion settings
    pub completion: CompletionConfig,
    
    /// Diagnostics configuration
    pub diagnostics: DiagnosticsConfig,
    
    /// Workspace symbol search settings
    pub workspace_symbol: WorkspaceSymbolConfig,
    
    /// Import settings
    pub imports: ImportsConfig,
    
    /// Assist (code actions) settings
    pub assist: AssistConfig,
    
    /// Hover settings
    pub hover: HoverConfig,
    
    /// Lens settings
    pub lens: LensConfig,
    
    /// Semantic tokens settings
    pub semantic_tokens: SemanticTokensConfig,
}

impl Default for RustAnalyzerConfig {
    fn default() -> Self {
        Self {
            check_on_save: true,
            check_command: "check".to_string(),
            check_args: vec!["--all-targets".to_string()],
            proc_macro_enable: true,
            proc_macro_server: None,
            inlay_hints: InlayHintsConfig::default(),
            completion: CompletionConfig::default(),
            diagnostics: DiagnosticsConfig::default(),
            workspace_symbol: WorkspaceSymbolConfig::default(),
            imports: ImportsConfig::default(),
            assist: AssistConfig::default(),
            hover: HoverConfig::default(),
            lens: LensConfig::default(),
            semantic_tokens: SemanticTokensConfig::default(),
        }
    }
}

/// Inlay hints configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlayHintsConfig {
    pub enable: bool,
    pub type_hints: bool,
    pub parameter_hints: bool,
    pub chaining_hints: bool,
    pub closure_return_type_hints: bool,
    pub lifetime_elision_hints: LifetimeElisionHints,
    pub max_length: Option<u32>,
    pub hide_named_constructor_hints: bool,
}

impl Default for InlayHintsConfig {
    fn default() -> Self {
        Self {
            enable: true,
            type_hints: true,
            parameter_hints: true,
            chaining_hints: true,
            closure_return_type_hints: true,
            lifetime_elision_hints: LifetimeElisionHints::SkipTrivial,
            max_length: Some(25),
            hide_named_constructor_hints: false,
        }
    }
}

/// Lifetime elision hints configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LifetimeElisionHints {
    Never,
    Always,
    SkipTrivial,
}

/// Completion configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionConfig {
    pub enable: bool,
    pub add_call_parenthesis: bool,
    pub add_call_argument_snippets: bool,
    pub snippet_cap: bool,
    pub postfix_enable: bool,
    pub imports_merge: ImportMergeMode,
    pub imports_prefix: ImportPrefixMode,
    pub private_editable: bool,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            enable: true,
            add_call_parenthesis: true,
            add_call_argument_snippets: true,
            snippet_cap: true,
            postfix_enable: true,
            imports_merge: ImportMergeMode::Full,
            imports_prefix: ImportPrefixMode::Plain,
            private_editable: false,
        }
    }
}

/// Import merge modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImportMergeMode {
    None,
    Full,
    Last,
}

/// Import prefix modes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImportPrefixMode {
    Plain,
    ByCrate,
    ByModule,
}

/// Diagnostics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsConfig {
    pub enable: bool,
    pub enable_experimental: bool,
    pub disabled: Vec<String>,
    pub remap_prefix: HashMap<String, String>,
    pub warnings_as_hint: Vec<String>,
    pub warnings_as_info: Vec<String>,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            enable: true,
            enable_experimental: false,
            disabled: Vec::new(),
            remap_prefix: HashMap::new(),
            warnings_as_hint: Vec::new(),
            warnings_as_info: Vec::new(),
        }
    }
}

/// Workspace symbol configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSymbolConfig {
    pub search_scope: WorkspaceSymbolSearchScope,
    pub search_kind: WorkspaceSymbolSearchKind,
    pub search_limit: u32,
}

impl Default for WorkspaceSymbolConfig {
    fn default() -> Self {
        Self {
            search_scope: WorkspaceSymbolSearchScope::Workspace,
            search_kind: WorkspaceSymbolSearchKind::OnlyTypes,
            search_limit: 128,
        }
    }
}

/// Workspace symbol search scope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceSymbolSearchScope {
    Workspace,
    WorkspaceAndDependencies,
}

/// Workspace symbol search kind
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceSymbolSearchKind {
    OnlyTypes,
    AllSymbols,
}

/// Imports configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportsConfig {
    pub granularity_enforce: bool,
    pub granularity_group: ImportGranularityGroup,
    pub group_enable: bool,
    pub merge_glob: bool,
    pub prefix_self: bool,
}

impl Default for ImportsConfig {
    fn default() -> Self {
        Self {
            granularity_enforce: false,
            granularity_group: ImportGranularityGroup::Crate,
            group_enable: true,
            merge_glob: true,
            prefix_self: true,
        }
    }
}

/// Import granularity group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImportGranularityGroup {
    Preserve,
    Item,
    Module,
    Crate,
}

/// Assist (code actions) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistConfig {
    pub emit_must_use: bool,
    pub expr_fill_default: ExprFillDefaultMode,
}

impl Default for AssistConfig {
    fn default() -> Self {
        Self {
            emit_must_use: false,
            expr_fill_default: ExprFillDefaultMode::Todo,
        }
    }
}

/// Expression fill default mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprFillDefaultMode {
    Todo,
    Default,
}

/// Hover configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverConfig {
    pub documentation: bool,
    pub keywords: bool,
    pub links_in_hover: bool,
    pub memory_layout: bool,
}

impl Default for HoverConfig {
    fn default() -> Self {
        Self {
            documentation: true,
            keywords: true,
            links_in_hover: true,
            memory_layout: false,
        }
    }
}

/// Lens configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LensConfig {
    pub enable: bool,
    pub debug_enable: bool,
    pub force_custom_commands: bool,
    pub implementations_enable: bool,
    pub references_adt_enable: bool,
    pub references_enum_variant_enable: bool,
    pub references_method_enable: bool,
    pub references_trait_enable: bool,
    pub run_enable: bool,
}

impl Default for LensConfig {
    fn default() -> Self {
        Self {
            enable: true,
            debug_enable: true,
            force_custom_commands: false,
            implementations_enable: true,
            references_adt_enable: false,
            references_enum_variant_enable: false,
            references_method_enable: false,
            references_trait_enable: false,
            run_enable: true,
        }
    }
}

/// Semantic tokens configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticTokensConfig {
    pub enable: bool,
    pub operator_enable: bool,
    pub operator_specialization_enable: bool,
    pub punctuation_enable: bool,
    pub punctuation_separate_macro_bang: bool,
    pub punctuation_specialization_enable: bool,
    pub string_enable: bool,
}

impl Default for SemanticTokensConfig {
    fn default() -> Self {
        Self {
            enable: true,
            operator_enable: true,
            operator_specialization_enable: false,
            punctuation_enable: false,
            punctuation_separate_macro_bang: false,
            punctuation_specialization_enable: false,
            string_enable: false,
        }
    }
}

/// Rust-analyzer specific LSP server manager
#[derive(Debug)]
pub struct RustAnalyzerManager {
    config: RustAnalyzerConfig,
    server_path: Option<PathBuf>,
}

impl RustAnalyzerManager {
    /// Create a new rust-analyzer manager
    pub fn new(config: RustAnalyzerConfig) -> Self {
        Self {
            config,
            server_path: None,
        }
    }
    
    /// Set the path to the rust-analyzer binary
    pub fn set_server_path(&mut self, path: PathBuf) {
        self.server_path = Some(path);
    }
    
    /// Auto-detect rust-analyzer binary
    pub async fn auto_detect_server(&mut self) -> LspResult<()> {
        // Try common locations for rust-analyzer
        let possible_paths = vec![
            "rust-analyzer",
            "rust-analyzer.exe",
            "~/.cargo/bin/rust-analyzer",
            "~/.cargo/bin/rust-analyzer.exe",
        ];
        
        for path_str in possible_paths {
            let path = PathBuf::from(path_str);
            if self.check_rust_analyzer_binary(&path).await? {
                self.server_path = Some(path);
                return Ok(());
            }
        }
        
        Err(LspError::ServerNotFound {
            server_name: "rust-analyzer".to_string(),
        })
    }
    
    /// Check if a path contains a valid rust-analyzer binary
    async fn check_rust_analyzer_binary(&self, path: &PathBuf) -> LspResult<bool> {
        if !path.exists() {
            return Ok(false);
        }
        
        // Try to run rust-analyzer --version
        let output = tokio::process::Command::new(path)
            .arg("--version")
            .output()
            .await
            .map_err(|e| LspError::InitializationFailed {
                reason: format!("Failed to execute rust-analyzer: {}", e),
            })?;
        
        if output.status.success() {
            let version_str = String::from_utf8_lossy(&output.stdout);
            return Ok(version_str.contains("rust-analyzer"));
        }
        
        Ok(false)
    }
    
    /// Create rust-analyzer server info
    pub fn create_server_info(&self, workspace_root: Option<PathBuf>) -> LspResult<LspServerInfo> {
        let server_path = self.server_path.as_ref()
            .ok_or_else(|| LspError::ServerNotFound {
                server_name: "rust-analyzer binary not found".to_string(),
            })?;
        
        let mut args = Vec::new();
        
        // Add workspace root if provided
        if let Some(root) = workspace_root {
            args.push("--workspace-root".to_string());
            args.push(root.to_string_lossy().to_string());
        }
        
        let initialization_options = self.create_initialization_options();
        
        Ok(LspServerInfo {
            id: format!("rust_analyzer_{}", Uuid::new_v4()),
            name: "rust-analyzer".to_string(),
            language_id: "rust".to_string(),
            command: server_path.to_string_lossy().to_string(),
            args,
            initialization_options: Some(initialization_options),
            capabilities: ServerCapabilities::default(),
            status: LspServerStatus::NotStarted,
        })
    }
    
    /// Create initialization options for rust-analyzer
    fn create_initialization_options(&self) -> serde_json::Value {
        serde_json::json!({
            "checkOnSave": {
                "enable": self.config.check_on_save,
                "command": self.config.check_command,
                "extraArgs": self.config.check_args
            },
            "procMacro": {
                "enable": self.config.proc_macro_enable,
                "server": self.config.proc_macro_server
            },
            "inlayHints": {
                "enable": self.config.inlay_hints.enable,
                "typeHints": {
                    "enable": self.config.inlay_hints.type_hints,
                    "hideNamedConstructor": self.config.inlay_hints.hide_named_constructor_hints
                },
                "parameterHints": {
                    "enable": self.config.inlay_hints.parameter_hints
                },
                "chainingHints": {
                    "enable": self.config.inlay_hints.chaining_hints
                },
                "closureReturnTypeHints": {
                    "enable": self.config.inlay_hints.closure_return_type_hints
                },
                "lifetimeElisionHints": {
                    "enable": match self.config.inlay_hints.lifetime_elision_hints {
                        LifetimeElisionHints::Never => "never",
                        LifetimeElisionHints::Always => "always",
                        LifetimeElisionHints::SkipTrivial => "skip_trivial",
                    },
                    "useParameterNames": false
                },
                "maxLength": self.config.inlay_hints.max_length
            },
            "completion": {
                "addCallParenthesis": self.config.completion.add_call_parenthesis,
                "addCallArgumentSnippets": self.config.completion.add_call_argument_snippets,
                "snippets": {
                    "custom": {}
                },
                "postfix": {
                    "enable": self.config.completion.postfix_enable
                },
                "imports": {
                    "merge": {
                        "glob": match self.config.completion.imports_merge {
                            ImportMergeMode::None => false,
                            _ => true,
                        }
                    },
                    "prefix": match self.config.completion.imports_prefix {
                        ImportPrefixMode::Plain => "plain",
                        ImportPrefixMode::ByCrate => "by_crate",
                        ImportPrefixMode::ByModule => "by_module",
                    }
                },
                "privateEditable": {
                    "enable": self.config.completion.private_editable
                }
            },
            "diagnostics": {
                "enable": self.config.diagnostics.enable,
                "experimental": {
                    "enable": self.config.diagnostics.enable_experimental
                },
                "disabled": self.config.diagnostics.disabled,
                "remapPrefix": self.config.diagnostics.remap_prefix,
                "warningsAsHint": self.config.diagnostics.warnings_as_hint,
                "warningsAsInfo": self.config.diagnostics.warnings_as_info
            },
            "workspace": {
                "symbol": {
                    "search": {
                        "scope": match self.config.workspace_symbol.search_scope {
                            WorkspaceSymbolSearchScope::Workspace => "workspace",
                            WorkspaceSymbolSearchScope::WorkspaceAndDependencies => "workspace_and_dependencies",
                        },
                        "kind": match self.config.workspace_symbol.search_kind {
                            WorkspaceSymbolSearchKind::OnlyTypes => "only_types",
                            WorkspaceSymbolSearchKind::AllSymbols => "all_symbols",
                        },
                        "limit": self.config.workspace_symbol.search_limit
                    }
                }
            },
            "imports": {
                "granularity": {
                    "enforce": self.config.imports.granularity_enforce,
                    "group": match self.config.imports.granularity_group {
                        ImportGranularityGroup::Preserve => "preserve",
                        ImportGranularityGroup::Item => "item",
                        ImportGranularityGroup::Module => "module",
                        ImportGranularityGroup::Crate => "crate",
                    }
                },
                "group": {
                    "enable": self.config.imports.group_enable
                },
                "merge": {
                    "glob": self.config.imports.merge_glob
                },
                "prefix": {
                    "self": self.config.imports.prefix_self
                }
            },
            "assist": {
                "emitMustUse": self.config.assist.emit_must_use,
                "exprFillDefault": match self.config.assist.expr_fill_default {
                    ExprFillDefaultMode::Todo => "todo",
                    ExprFillDefaultMode::Default => "default",
                }
            },
            "hover": {
                "documentation": {
                    "enable": self.config.hover.documentation
                },
                "keywords": {
                    "enable": self.config.hover.keywords
                },
                "linksInHover": {
                    "enable": self.config.hover.links_in_hover
                },
                "memoryLayout": {
                    "enable": self.config.hover.memory_layout
                }
            },
            "lens": {
                "enable": self.config.lens.enable,
                "debug": {
                    "enable": self.config.lens.debug_enable
                },
                "forceCustomCommands": self.config.lens.force_custom_commands,
                "implementations": {
                    "enable": self.config.lens.implementations_enable
                },
                "references": {
                    "adt": {
                        "enable": self.config.lens.references_adt_enable
                    },
                    "enumVariant": {
                        "enable": self.config.lens.references_enum_variant_enable
                    },
                    "method": {
                        "enable": self.config.lens.references_method_enable
                    },
                    "trait": {
                        "enable": self.config.lens.references_trait_enable
                    }
                },
                "run": {
                    "enable": self.config.lens.run_enable
                }
            },
            "semanticTokens": {
                "enable": self.config.semantic_tokens.enable,
                "operator": {
                    "enable": self.config.semantic_tokens.operator_enable,
                    "specialization": {
                        "enable": self.config.semantic_tokens.operator_specialization_enable
                    }
                },
                "punctuation": {
                    "enable": self.config.semantic_tokens.punctuation_enable,
                    "separate": {
                        "macro": {
                            "bang": self.config.semantic_tokens.punctuation_separate_macro_bang
                        }
                    },
                    "specialization": {
                        "enable": self.config.semantic_tokens.punctuation_specialization_enable
                    }
                },
                "string": {
                    "enable": self.config.semantic_tokens.string_enable
                }
            }
        })
    }
    
    /// Update configuration
    pub fn update_config(&mut self, config: RustAnalyzerConfig) {
        self.config = config;
    }
    
    /// Get current configuration
    pub fn get_config(&self) -> &RustAnalyzerConfig {
        &self.config
    }
    
    /// Get server path
    pub fn get_server_path(&self) -> Option<&PathBuf> {
        self.server_path.as_ref()
    }
}

/// Rust-analyzer specific request handlers
pub struct RustAnalyzerRequests;

impl RustAnalyzerRequests {
    /// Request inlay hints for a document
    pub fn inlay_hints(
        text_document: TextDocumentIdentifier,
        range: Range,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "range": range
        })
    }
    
    /// Request syntax tree for a document
    pub fn syntax_tree(
        text_document: TextDocumentIdentifier,
        range: Option<Range>,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "range": range
        })
    }
    
    /// Request expanded macro
    pub fn expand_macro(
        text_document: TextDocumentIdentifier,
        position: Position,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "position": position
        })
    }
    
    /// Request parent module
    pub fn parent_module(
        text_document: TextDocumentIdentifier,
        position: Position,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "position": position
        })
    }
    
    /// Request join lines
    pub fn join_lines(
        text_document: TextDocumentIdentifier,
        ranges: Vec<Range>,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "ranges": ranges
        })
    }
    
    /// Request on enter
    pub fn on_enter(
        text_document: TextDocumentIdentifier,
        position: Position,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "position": position
        })
    }
    
    /// Request matching brace
    pub fn matching_brace(
        text_document: TextDocumentIdentifier,
        positions: Vec<Position>,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "positions": positions
        })
    }
    
    /// Request related tests
    pub fn related_tests(
        text_document: TextDocumentIdentifier,
        position: Position,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "position": position
        })
    }
    
    /// Request run single test
    pub fn run_single(
        text_document: TextDocumentIdentifier,
        position: Position,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "position": position
        })
    }
    
    /// Request debug single test
    pub fn debug_single(
        text_document: TextDocumentIdentifier,
        position: Position,
    ) -> serde_json::Value {
        serde_json::json!({
            "textDocument": text_document,
            "position": position
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_analyzer_config_default() {
        let config = RustAnalyzerConfig::default();
        assert!(config.check_on_save);
        assert_eq!(config.check_command, "check");
        assert!(config.proc_macro_enable);
        assert!(config.inlay_hints.enable);
    }

    #[test]
    fn test_inlay_hints_config() {
        let config = InlayHintsConfig::default();
        assert!(config.enable);
        assert!(config.type_hints);
        assert!(config.parameter_hints);
        assert_eq!(config.max_length, Some(25));
    }

    #[test]
    fn test_completion_config() {
        let config = CompletionConfig::default();
        assert!(config.enable);
        assert!(config.add_call_parenthesis);
        assert!(config.snippet_cap);
    }

    #[test]
    fn test_manager_creation() {
        let config = RustAnalyzerConfig::default();
        let manager = RustAnalyzerManager::new(config);
        assert!(manager.get_server_path().is_none());
    }

    #[test]
    fn test_initialization_options_creation() {
        let config = RustAnalyzerConfig::default();
        let manager = RustAnalyzerManager::new(config);
        let options = manager.create_initialization_options();
        
        assert!(options.is_object());
        assert!(options.get("checkOnSave").is_some());
        assert!(options.get("inlayHints").is_some());
        assert!(options.get("completion").is_some());
    }

    #[test]
    fn test_rust_analyzer_requests() {
        let text_doc = TextDocumentIdentifier {
            uri: Url::parse("file:///test.rs").unwrap(),
        };
        let position = Position { line: 0, character: 0 };
        let range = Range {
            start: position,
            end: position,
        };
        
        let inlay_request = RustAnalyzerRequests::inlay_hints(text_doc.clone(), range);
        assert!(inlay_request.get("textDocument").is_some());
        assert!(inlay_request.get("range").is_some());
        
        let syntax_request = RustAnalyzerRequests::syntax_tree(text_doc.clone(), Some(range));
        assert!(syntax_request.get("textDocument").is_some());
        
        let expand_request = RustAnalyzerRequests::expand_macro(text_doc, position);
        assert!(expand_request.get("textDocument").is_some());
        assert!(expand_request.get("position").is_some());
    }
}