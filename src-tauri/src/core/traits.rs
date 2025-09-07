use crate::core::{Position, Range};
use std::path::Path;

/// Trait for listening to editor events
pub trait EditorEventListener: Send + Sync {
    /// Called when the cursor position changes
    fn on_cursor_moved(&self, positions: &[Position]) {
        let _ = positions; // Default implementation does nothing
    }

    /// Called when the selection changes
    fn on_selection_changed(&self, has_selection: bool) {
        let _ = has_selection; // Default implementation does nothing
    }

    /// Called when a file is saved
    fn on_file_saved(&self, path: &Path) {
        let _ = path; // Default implementation does nothing
    }

    /// Called when a file is loaded
    fn on_file_loaded(&self, path: &Path) {
        let _ = path; // Default implementation does nothing
    }

    /// Called when text content changes
    fn on_text_changed(&self, version: u64) {
        let _ = version; // Default implementation does nothing
    }
}

/// Trait for text processing and analysis
pub trait TextProcessor: Send + Sync {
    /// Process text and return analysis results
    fn process_text(&self, text: &str) -> Result<TextAnalysis, String>;

    /// Get supported file extensions
    fn supported_extensions(&self) -> &[&str];

    /// Check if this processor can handle the given file
    fn can_process(&self, path: &Path) -> bool {
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            self.supported_extensions().contains(&extension)
        } else {
            false
        }
    }
}

/// Results of text analysis
#[derive(Debug, Clone)]
pub struct TextAnalysis {
    /// Number of lines
    pub line_count: usize,
    /// Number of characters
    pub char_count: usize,
    /// Number of words
    pub word_count: usize,
    /// Language detected
    pub language: Option<String>,
    /// Issues found (warnings, errors)
    pub issues: Vec<TextIssue>,
}

/// Text issue found during analysis
#[derive(Debug, Clone)]
pub struct TextIssue {
    /// Issue severity
    pub severity: IssueSeverity,
    /// Issue message
    pub message: String,
    /// Location in text
    pub range: Option<Range>,
    /// Suggested fix
    pub suggestion: Option<String>,
}

/// Severity levels for text issues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// Error that should be fixed
    Error,
    /// Warning that should be addressed
    Warning,
    /// Informational note
    Info,
    /// Hint for improvement
    Hint,
}

/// Trait for providing completions and suggestions
pub trait CompletionProvider: Send + Sync {
    /// Get completions at the given position
    fn get_completions(&self, text: &str, position: Position) -> Vec<Completion>;

    /// Get signature help for function calls
    fn get_signature_help(&self, text: &str, position: Position) -> Option<SignatureHelp>;

    /// Get hover information for the element at position
    fn get_hover_info(&self, text: &str, position: Position) -> Option<HoverInfo>;
}

/// A completion suggestion
#[derive(Debug, Clone)]
pub struct Completion {
    /// Label shown to user
    pub label: String,
    /// Text to insert
    pub insert_text: String,
    /// Completion kind
    pub kind: CompletionKind,
    /// Additional detail
    pub detail: Option<String>,
    /// Documentation
    pub documentation: Option<String>,
    /// Sort priority (lower = higher priority)
    pub sort_text: Option<String>,
}

/// Types of completions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// Variable
    Variable,
    /// Function
    Function,
    /// Method
    Method,
    /// Type/Class
    Type,
    /// Module
    Module,
    /// Keyword
    Keyword,
    /// Snippet
    Snippet,
    /// Other
    Other,
}

/// Signature help information
#[derive(Debug, Clone)]
pub struct SignatureHelp {
    /// Available signatures
    pub signatures: Vec<SignatureInfo>,
    /// Active signature index
    pub active_signature: usize,
    /// Active parameter index
    pub active_parameter: usize,
}

/// Information about a function signature
#[derive(Debug, Clone)]
pub struct SignatureInfo {
    /// Function signature label
    pub label: String,
    /// Function documentation
    pub documentation: Option<String>,
    /// Parameters
    pub parameters: Vec<ParameterInfo>,
}

/// Information about a function parameter
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    /// Parameter label
    pub label: String,
    /// Parameter documentation
    pub documentation: Option<String>,
}

/// Hover information
#[derive(Debug, Clone)]
pub struct HoverInfo {
    /// Content to display
    pub contents: String,
    /// Range this hover applies to
    pub range: Option<Range>,
}

/// Trait for document formatting
pub trait DocumentFormatter: Send + Sync {
    /// Format the entire document
    fn format_document(&self, text: &str) -> Result<String, String>;

    /// Format a range within the document
    fn format_range(&self, text: &str, range: Range) -> Result<String, String>;

    /// Get formatting options
    fn get_options(&self) -> FormattingOptions;

    /// Set formatting options
    fn set_options(&mut self, options: FormattingOptions);
}

/// Formatting options
#[derive(Debug, Clone)]
pub struct FormattingOptions {
    /// Tab size
    pub tab_size: usize,
    /// Use spaces instead of tabs
    pub insert_spaces: bool,
    /// Trim trailing whitespace
    pub trim_trailing_whitespace: bool,
    /// Insert final newline
    pub insert_final_newline: bool,
    /// Maximum line length
    pub max_line_length: Option<usize>,
}

impl Default for FormattingOptions {
    fn default() -> Self {
        Self {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: true,
            insert_final_newline: true,
            max_line_length: Some(100),
        }
    }
}

/// Trait for code navigation and references
pub trait NavigationProvider: Send + Sync {
    /// Go to definition of symbol at position
    fn goto_definition(&self, text: &str, position: Position) -> Vec<Location>;

    /// Find all references to symbol at position
    fn find_references(&self, text: &str, position: Position) -> Vec<Location>;

    /// Find symbols in document
    fn document_symbols(&self, text: &str) -> Vec<DocumentSymbol>;

    /// Find symbols in workspace
    fn workspace_symbols(&self, query: &str) -> Vec<WorkspaceSymbol>;
}

/// A location in a file
#[derive(Debug, Clone)]
pub struct Location {
    /// File path
    pub path: std::path::PathBuf,
    /// Range in the file
    pub range: Range,
}

/// A symbol in a document
#[derive(Debug, Clone)]
pub struct DocumentSymbol {
    /// Symbol name
    pub name: String,
    /// Symbol kind
    pub kind: SymbolKind,
    /// Range of the symbol
    pub range: Range,
    /// Selection range (usually just the name)
    pub selection_range: Range,
    /// Child symbols
    pub children: Vec<DocumentSymbol>,
}

/// A symbol in the workspace
#[derive(Debug, Clone)]
pub struct WorkspaceSymbol {
    /// Symbol name
    pub name: String,
    /// Symbol kind
    pub kind: SymbolKind,
    /// Location of the symbol
    pub location: Location,
    /// Container name (e.g., class name for a method)
    pub container_name: Option<String>,
}

/// Types of symbols
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// File
    File,
    /// Module
    Module,
    /// Namespace
    Namespace,
    /// Package
    Package,
    /// Class
    Class,
    /// Method
    Method,
    /// Property
    Property,
    /// Field
    Field,
    /// Constructor
    Constructor,
    /// Enum
    Enum,
    /// Interface
    Interface,
    /// Function
    Function,
    /// Variable
    Variable,
    /// Constant
    Constant,
    /// String
    String,
    /// Number
    Number,
    /// Boolean
    Boolean,
    /// Array
    Array,
    /// Object
    Object,
    /// Key
    Key,
    /// Null
    Null,
    /// EnumMember
    EnumMember,
    /// Struct
    Struct,
    /// Event
    Event,
    /// Operator
    Operator,
    /// TypeParameter
    TypeParameter,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Position;

    struct TestEventListener {
        events: std::sync::Mutex<Vec<String>>,
    }

    impl TestEventListener {
        fn new() -> Self {
            Self {
                events: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn get_events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl EditorEventListener for TestEventListener {
        fn on_cursor_moved(&self, positions: &[Position]) {
            self.events
                .lock()
                .unwrap()
                .push(format!("cursor_moved: {} positions", positions.len()));
        }

        fn on_selection_changed(&self, has_selection: bool) {
            self.events
                .lock()
                .unwrap()
                .push(format!("selection_changed: {}", has_selection));
        }
    }

    #[test]
    fn test_event_listener() {
        let listener = TestEventListener::new();
        
        listener.on_cursor_moved(&[Position::new(0, 0), Position::new(1, 0)]);
        listener.on_selection_changed(true);

        let events = listener.get_events();
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("cursor_moved: 2 positions"));
        assert!(events[1].contains("selection_changed: true"));
    }

    #[test]
    fn test_completion_kind() {
        let completion = Completion {
            label: "test_func".to_string(),
            insert_text: "test_func()".to_string(),
            kind: CompletionKind::Function,
            detail: Some("fn test_func()".to_string()),
            documentation: None,
            sort_text: None,
        };

        assert_eq!(completion.kind, CompletionKind::Function);
        assert_eq!(completion.label, "test_func");
    }

    #[test]
    fn test_formatting_options_default() {
        let options = FormattingOptions::default();
        assert_eq!(options.tab_size, 4);
        assert!(options.insert_spaces);
        assert!(options.trim_trailing_whitespace);
        assert!(options.insert_final_newline);
        assert_eq!(options.max_line_length, Some(100));
    }

    #[test]
    fn test_text_analysis() {
        let analysis = TextAnalysis {
            line_count: 10,
            char_count: 500,
            word_count: 100,
            language: Some("rust".to_string()),
            issues: vec![TextIssue {
                severity: IssueSeverity::Warning,
                message: "Unused variable".to_string(),
                range: Some(Range::new(Position::new(5, 0), Position::new(5, 10))),
                suggestion: Some("Remove or use the variable".to_string()),
            }],
        };

        assert_eq!(analysis.line_count, 10);
        assert_eq!(analysis.language, Some("rust".to_string()));
        assert_eq!(analysis.issues.len(), 1);
        assert_eq!(analysis.issues[0].severity, IssueSeverity::Warning);
    }

    #[test]
    fn test_symbol_kind_variants() {
        assert_ne!(SymbolKind::Function, SymbolKind::Method);
        assert_ne!(SymbolKind::Class, SymbolKind::Struct);
        assert_eq!(SymbolKind::Variable, SymbolKind::Variable);
    }
}