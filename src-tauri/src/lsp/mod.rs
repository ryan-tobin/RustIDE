use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tower_lsp::lsp_types::*;
use uuid::Uuid;

pub mod client;
pub mod ownership;
pub mod rust_analyzer;

pub use client::*;
pub use ownership::*;
pub use rust_analyzer::*;
/// Result type for LSP operations
pub type LspResult<T> = Result<T, LspError>;

/// Errors that can occur in LSP operations
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum LspError {
    #[error("Language server not found: {server_name}")]
    ServerNotFound { server_name: String },

    #[error("Language server initialization failed: {reason}")]
    InitializationFailed { reason: String },

    #[error("LSP request failed: {method} - {message}")]
    RequestFailed { method: String, message: String },

    #[error("Invalid LSP response: {expected}")]
    InvalidResponse { expected: String },

    #[error("Server communication error: {source}")]
    CommunicationError { source: String },

    #[error("Document not found: {uri}")]
    DocumentNotFound { uri: String },

    #[error("Ownership analysis error: {message}")]
    OwnershipError { message: String },

    #[error("Capability not supported: {capability}")]
    UnsupportedCapability { capability: String },
}

impl From<tower_lsp::jsonrpc::Error> for LspError {
    fn from(err: tower_lsp::jsonrpc::Error) -> Self {
        LspError::RequestFailed {
            method: "unknown".to_string(),
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for LspError {
    fn from(err: serde_json::Error) -> Self {
        LspError::InvalidResponse {
            expected: err.to_string(),
        }
    }
}

/// LSP server information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerInfo {
    pub id: String,
    pub name: String,
    pub language_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub initialization_options: Option<serde_json::Value>,
    pub capabilities: ServerCapabilities,
    pub status: LspServerStatus,
}

/// Status of an LSP server
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LspServerStatus {
    NotStarted,
    Initializing,
    Running,
    Failed { reason: String },
    Stopped,
}

/// Document state tracking for LSP
#[derive(Debug, Clone)]
pub struct LspDocument {
    pub uri: Url,
    pub language_id: String,
    pub version: i32,
    pub content: String,
    pub diagnostics: Vec<Diagnostic>,
    pub last_modified: std::time::SystemTime,
}

/// LSP feature capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspCapabilities {
    pub completion: bool,
    pub hover: bool,
    pub signature_help: bool,
    pub definition: bool,
    pub type_definition: bool,
    pub implementation: bool,
    pub references: bool,
    pub document_highlight: bool,
    pub document_symbol: bool,
    pub workspace_symbol: bool,
    pub code_action: bool,
    pub code_lens: bool,
    pub formatting: bool,
    pub range_formatting: bool,
    pub rename: bool,
    pub folding_range: bool,
    pub selection_range: bool,
    pub call_hierarchy: bool,
    pub semantic_tokens: bool,
    pub inlay_hints: bool,
}

impl From<&ServerCapabilities> for LspCapabilities {
    fn from(caps: &ServerCapabilities) -> Self {
        Self {
            completion: caps.completion_provider.is_some(),
            hover: caps.hover_provider.is_some(),
            signature_help: caps.signature_help_provider.is_some(),
            definition: caps.definition_provider.is_some(),
            type_definition: caps.type_definition_provider.is_some(),
            implementation: caps.implementation_provider.is_some(),
            references: caps.references_provider.is_some(),
            document_highlight: caps.document_highlight_provider.is_some(),
            document_symbol: caps.document_symbol_provider.is_some(),
            workspace_symbol: caps.workspace_symbol_provider.is_some(),
            code_action: caps.code_action_provider.is_some(),
            code_lens: caps.code_lens_provider.is_some(),
            formatting: caps.document_formatting_provider.is_some(),
            range_formatting: caps.document_range_formatting_provider.is_some(),
            rename: caps.rename_provider.is_some(),
            folding_range: caps.folding_range_provider.is_some(),
            selection_range: caps.selection_range_provider.is_some(),
            call_hierarchy: caps.call_hierarchy_provider.is_some(),
            semantic_tokens: caps.semantic_tokens_provider.is_some(),
            inlay_hints: caps.inlay_hint_provider.is_some(),
        }
    }
}

/// LSP Request types that we support
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum LspRequest {
    /// Completion request
    Completion {
        text_document: TextDocumentIdentifier,
        position: Position,
        context: Option<CompletionContext>,
    },

    /// Hover information request
    Hover {
        text_document: TextDocumentIdentifier,
        position: Position,
    },

    /// Go to definition request
    GotoDefinition {
        text_document: TextDocumentIdentifier,
        position: Position,
    },
    
    /// Find references request
    FindReferences {
        text_document: TextDocumentIdentifier,
        position: Position,
        include_declaration: bool,
    },
    
    /// Document symbols request
    DocumentSymbols {
        text_document: TextDocumentIdentifier,
    },
    
    /// Workspace symbols request
    WorkspaceSymbols {
        query: String,
    },
    
    /// Code actions request
    CodeAction {
        text_document: TextDocumentIdentifier,
        range: Range,
        context: CodeActionContext,
    },
    
    /// Formatting request
    Formatting {
        text_document: TextDocumentIdentifier,
        options: FormattingOptions,
    },
    
    /// Rename request
    Rename {
        text_document: TextDocumentIdentifier,
        position: Position,
        new_name: String,
    },
    
    /// Inlay hints request
    InlayHints {
        text_document: TextDocumentIdentifier,
        range: Range,
    },
}

/// LSP response types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum LspResponse {
    Completion {
        items: Vec<CompletionItem>,
    },
    
    Hover {
        contents: HoverContents,
        range: Option<Range>,
    },
    
    Locations {
        locations: Vec<Location>,
    },
    
    DocumentSymbols {
        symbols: Vec<DocumentSymbol>,
    },
    
    WorkspaceSymbols {
        symbols: Vec<SymbolInformation>,
    },
    
    CodeActions {
        actions: Vec<CodeActionOrCommand>,
    },
    
    TextEdits {
        edits: Vec<TextEdit>,
    },
    
    WorkspaceEdit {
        edit: WorkspaceEdit,
    },
    
    InlayHints {
        hints: Vec<InlayHint>,
    },
    
    Error {
        message: String,
    },
}

/// Diagnostic severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Information,
    Hint,
}

impl From<DiagnosticSeverity> for DiagnosticLevel {
    fn from(severity: DiagnosticSeverity) -> Self {
        match severity {
            DiagnosticSeverity::ERROR => DiagnosticLevel::Error,
            DiagnosticSeverity::WARNING => DiagnosticLevel::Warning,
            DiagnosticSeverity::INFORMATION => DiagnosticLevel::Information,
            DiagnosticSeverity::HINT => DiagnosticLevel::Hint,
        }
    }
}

/// Enhanced diagnostic information with ownership insights
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedDiagnostic {
    pub diagnostic: Diagnostic,
    pub level: DiagnosticLevel,
    pub ownership_info: Option<OwnershipInfo>,
    pub quick_fixes: Vec<QuickFix>,
    pub related_information: Vec<DiagnosticRelatedInformation>,
}

/// Quick fix suggestions for diagnostics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFix {
    pub title: String,
    pub kind: CodeActionKind,
    pub edit: WorkspaceEdit,
    pub is_preferred: bool,
}

/// Symbol information with additional metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedSymbol {
    pub symbol: DocumentSymbol,
    pub ownership_info: Option<OwnershipInfo>,
    pub usage_count: u32,
    pub is_exported: bool,
    pub documentation: Option<String>,
}

/// Code completion item with enhanced information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedCompletion {
    pub item: CompletionItem,
    pub ownership_info: Option<OwnershipInfo>,
    pub snippet: Option<String>,
    pub import_info: Option<ImportInfo>,
}

/// Import information for completions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportInfo {
    pub module_path: String,
    pub is_external_crate: bool,
    pub additional_edits: Vec<TextEdit>,
}

/// LSP events that can be sent to the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum LspEvent {
    /// Server status changed
    ServerStatusChanged {
        server_id: String,
        status: LspServerStatus,
    },
    
    /// Diagnostics updated for a document
    DiagnosticsUpdated {
        uri: String,
        diagnostics: Vec<EnhancedDiagnostic>,
    },
    
    /// Progress notification
    Progress {
        token: String,
        title: String,
        message: Option<String>,
        percentage: Option<u32>,
    },
    
    /// Ownership visualization updated
    OwnershipUpdated {
        uri: String,
        ownership_map: OwnershipMap,
    },
    
    /// Server capabilities changed
    CapabilitiesChanged {
        server_id: String,
        capabilities: LspCapabilities,
    },
}

/// Configuration for LSP servers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspConfig {
    pub rust_analyzer: RustAnalyzerConfig,
    pub enable_inlay_hints: bool,
    pub enable_semantic_tokens: bool,
    pub completion_auto_import: bool,
    pub diagnostics_delay: u64, // milliseconds
    pub max_completion_items: u32,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            rust_analyzer: RustAnalyzerConfig::default(),
            enable_inlay_hints: true,
            enable_semantic_tokens: true,
            completion_auto_import: true,
            diagnostics_delay: 500,
            max_completion_items: 50,
        }
    }
}

/// Utility functions for LSP operations
pub mod utils {
    use super::*;
    use std::path::Path;

    /// Convert file path to LSP URI
    pub fn path_to_uri(path: &Path) -> LspResult<Url> {
        Url::from_file_path(path)
            .map_err(|_| LspError::InvalidResponse {
                expected: "valid file URI".to_string(),
            })
    }

    /// Convert LSP URI to file path
    pub fn uri_to_path(uri: &Url) -> LspResult<PathBuf> {
        uri.to_file_path()
            .map_err(|_| LspError::InvalidResponse {
                expected: "valid file path".to_string(),
            })
    }

    /// Convert position from editor coords to LSP positions
    pub fn editor_position_to_lsp(line: u32, column: u32) -> Position {
        Position {
            line,
            character: column,
        }
    }

    /// Convert LSP position to editor coords
    pub fn lsp_position_to_editor(position: &Position) -> (u32, u32) {
        (position.line, position.character)
    }

    /// Convert range from editor coords to LSP range
    pub fn editor_range_to_lsp(
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Range {
        Range {
            start: Position {
                line: start_line,
                character: start_column,
            },
            end: Position {
                line: end_line,
                character: end_column,
            },
        }
    }

    /// Check if a position is within a range
    pub fn position_in_range(position: &Position, range: &Range) -> bool {
        if position.line < range.start.line || position.line > range.end.line {
            return false;
        }

        if position.line == range.start.line && position.character < range.start.character {
            return false;
        }

        if position.line == range.end.line && position.character > range.end.character {
            return false;
        }

        true 
    }

    /// Get text in range from document content
    pub fn get_text_in_range(content: &str, range: &Range) -> Option<String> {
        let lines: Vec<&str> = content.lines().collect();

        if range.start.line as usize >= lines.len() || range.end.line as usize >= lines.len() {
            return None;
        }

        if range.start.line == range.end.line {
            let line = lines[range.start.line as usize];
            let start = range.start.character as usize;
            let end = range.end.character as usize;

            if start <= line.len() && end <= line.len() && start <= end {
                return Some(line[start..end].to_string());
            }
        } else {
            let mut result = String::new();

            for line_idx in range.start.line..=range.end.line {
                let line = lines[line_idx as usize];

                if line_idx == range.start.line {
                    let start = range.start.character as usize;
                    if start <= line.len() {
                        result.push_str(&line[start..]);
                        result.push('\n');
                    }
                } else if line_idx == range.end.line {
                    let end = range.end.character as usize;
                    if end <= line.len() {
                        result.push_str(&line[..end]);
                    }
                } else {
                    result.push_str(line);
                    result.push('\n');
                }
            }

            return Some(result);
        }

        None 
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::utils::*;

    #[test]
    fn test_position_conversion() {
        let pos = editor_position_to_lsp(10, 5);
        assert_eq!(pos.line, 10);
        assert_eq!(pos.character, 5);
        
        let (line, col) = lsp_position_to_editor(&pos);
        assert_eq!(line, 10);
        assert_eq!(col, 5);
    }

    #[test]
    fn test_range_conversion() {
        let range = editor_range_to_lsp(1, 2, 3, 4);
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 2);
        assert_eq!(range.end.line, 3);
        assert_eq!(range.end.character, 4);
    }

    #[test]
    fn test_position_in_range() {
        let range = Range {
            start: Position { line: 1, character: 5 },
            end: Position { line: 3, character: 10 },
        };
        
        // Position inside range
        let pos1 = Position { line: 2, character: 0 };
        assert!(position_in_range(&pos1, &range));
        
        // Position at start
        let pos2 = Position { line: 1, character: 5 };
        assert!(position_in_range(&pos2, &range));
        
        // Position at end
        let pos3 = Position { line: 3, character: 10 };
        assert!(position_in_range(&pos3, &range));
        
        // Position before range
        let pos4 = Position { line: 1, character: 4 };
        assert!(!position_in_range(&pos4, &range));
        
        // Position after range
        let pos5 = Position { line: 3, character: 11 };
        assert!(!position_in_range(&pos5, &range));
    }

    #[test]
    fn test_get_text_in_range() {
        let content = "line 0\nline 1\nline 2\nline 3";
        
        // Single line range
        let range1 = Range {
            start: Position { line: 1, character: 2 },
            end: Position { line: 1, character: 6 },
        };
        assert_eq!(get_text_in_range(content, &range1), Some("ne 1".to_string()));
        
        // Multi-line range
        let range2 = Range {
            start: Position { line: 1, character: 2 },
            end: Position { line: 2, character: 4 },
        };
        assert_eq!(get_text_in_range(content, &range2), Some("ne 1\nline".to_string()));
    }

    #[test]
    fn test_diagnostic_level_conversion() {
        assert_eq!(
            DiagnosticLevel::from(DiagnosticSeverity::ERROR),
            DiagnosticLevel::Error
        );
        assert_eq!(
            DiagnosticLevel::from(DiagnosticSeverity::WARNING),
            DiagnosticLevel::Warning
        );
    }

    #[test]
    fn test_lsp_config_default() {
        let config = LspConfig::default();
        assert!(config.enable_inlay_hints);
        assert!(config.enable_semantic_tokens);
        assert_eq!(config.diagnostics_delay, 500);
    }
}
