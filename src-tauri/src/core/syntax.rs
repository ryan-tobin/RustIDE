use crate::core::text_buffer::{BufferChangeEvent, Position, Range, TextBuffer};
use anyhow::{Context, Result};
use lru::LruCache;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, instrument, warn};
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, Tree};

// External language bindings
extern "C" {
    fn tree_sitter_rust() -> Language;
    fn tree_sitter_toml() -> Language;
    fn tree_sitter_json() -> Language;
    fn tree_sitter_yaml() -> Language;
    fn tree_sitter_markdown() -> Language;
}

/// Syntax highlighting token types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenType {
    // Basic types
    Text,
    Comment,
    String,
    Number,
    Boolean,
    Null,

    // Identifiers
    Variable,
    Parameter,
    Field,
    Property,

    // Functions and methods
    Function,
    Method,
    Constructor,
    Macro,

    // Keywords
    Keyword,
    KeywordControl,
    KeywordFunction,
    KeywordReturn,
    KeywordImport,
    KeywordStorage,
    KeywordOperator,

    // Types
    Type,
    TypeBuiltin,
    TypeParameter,
    Interface,
    Struct,
    Enum,
    Union,
    Trait,

    // Operators and punctuation
    Operator,
    Punctuation,
    PunctuationBracket,
    PunctuationDelimiter,
    PunctuationSpecial,

    // Rust-specific
    Lifetime,
    Label,
    Attribute,
    DeriveMacro,
    FormatSpecifier,

    // Semantic highlighting
    Namespace,
    Module,
    Constant,
    ConstantBuiltin,

    // Error highlighting
    Error,
    Warning,

    // Documentation
    DocComment,
    DocKeyword,
}

impl TokenType {
    /// Get the default CSS class name for this token type
    pub fn css_class(&self) -> &'static str {
        match self {
            TokenType::Text => "text",
            TokenType::Comment => "comment",
            TokenType::String => "string",
            TokenType::Number => "number",
            TokenType::Boolean => "boolean",
            TokenType::Null => "null",
            TokenType::Variable => "variable",
            TokenType::Parameter => "parameter",
            TokenType::Field => "field",
            TokenType::Property => "property",
            TokenType::Function => "function",
            TokenType::Method => "method",
            TokenType::Constructor => "constructor",
            TokenType::Macro => "macro",
            TokenType::Keyword => "keyword",
            TokenType::KeywordControl => "keyword-control",
            TokenType::KeywordFunction => "keyword-function",
            TokenType::KeywordReturn => "keyword-return",
            TokenType::KeywordImport => "keyword-import",
            TokenType::KeywordStorage => "keyword-storage",
            TokenType::KeywordOperator => "keyword-operator",
            TokenType::Type => "type",
            TokenType::TypeBuiltin => "type-builtin",
            TokenType::TypeParameter => "type-parameter",
            TokenType::Interface => "interface",
            TokenType::Struct => "struct",
            TokenType::Enum => "enum",
            TokenType::Union => "union",
            TokenType::Trait => "trait",
            TokenType::Operator => "operator",
            TokenType::Punctuation => "punctuation",
            TokenType::PunctuationBracket => "punctuation-bracket",
            TokenType::PunctuationDelimiter => "punctuation-delimiter",
            TokenType::PunctuationSpecial => "punctuation-special",
            TokenType::Lifetime => "lifetime",
            TokenType::Label => "label",
            TokenType::Attribute => "attribute",
            TokenType::DeriveMacro => "derive-macro",
            TokenType::FormatSpecifier => "format-specifier",
            TokenType::Namespace => "namespace",
            TokenType::Module => "module",
            TokenType::Constant => "constant",
            TokenType::ConstantBuiltin => "constant-builtin",
            TokenType::Error => "error",
            TokenType::Warning => "warning",
            TokenType::DocComment => "doc-comment",
            TokenType::DocKeyword => "doc-keyword",
        }
    }
}

/// A syntax highlighting token
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub range: Range,
    pub token_type: TokenType,
    pub text: String,
    pub precedence: u8,
}

impl Token {
    pub fn new(range: Range, token_type: TokenType, text: String) -> Self {
        Self {
            range,
            token_type,
            text,
            precedence: token_type.default_precedence(),
        }
    }

    pub fn with_precedence(mut self, precedence: u8) -> Self {
        self.precedence = precedence;
        self
    }
}

impl TokenType {
    fn default_precedence(&self) -> u8 {
        match self {
            // Highest precedence for specific constructs
            TokenType::Error | TokenType::Warning => 100,
            TokenType::String | TokenType::Comment | TokenType::DocComment => 90,
            TokenType::Attribute | TokenType::DeriveMacro => 85,
            TokenType::Macro | TokenType::FormatSpecifier => 80,
            TokenType::Keyword | TokenType::KeywordControl | TokenType::KeywordFunction => 70,
            TokenType::Type | TokenType::TypeBuiltin | TokenType::Struct | TokenType::Enum => 60,
            TokenType::Function | TokenType::Method | TokenType::Constructor => 50,
            TokenType::Variable | TokenType::Parameter | TokenType::Field => 40,
            TokenType::Operator | TokenType::Punctuation => 30,
            TokenType::Number | TokenType::Boolean => 20,
            TokenType::Text => 10,
            _ => 50, // Default middle priority
        }
    }
}

/// Language definition for syntax highlighting
#[derive(Debug, Clone)]
pub struct LanguageConfig {
    pub name: &'static str,
    pub language: Language,
    pub highlight_query: &'static str,
    pub file_extensions: &'static [&'static str],
    pub comment_prefix: &'static str,
}

/// Get supported language configurations
pub fn get_language_configs() -> HashMap<&'static str, LanguageConfig> {
    let mut configs = HashMap::new();

    // Rust language configuration
    configs.insert(
        "rust",
        LanguageConfig {
            name: "rust",
            language: unsafe { tree_sitter_rust() },
            file_extensions: &["rs"],
            comment_prefix: "//",
            highlight_query: r##"
            ; Keywords
            ["as" "async" "await" "break" "const" "continue" "crate" "dyn" "else" "enum" 
             "extern" "false" "fn" "for" "if" "impl" "in" "let" "loop" "match" "mod" 
             "move" "mut" "pub" "ref" "return" "self" "Self" "static" "struct" "super" 
             "trait" "true" "type" "union" "unsafe" "use" "where" "while" "yield"] @keyword

            ; Control flow keywords
            ["if" "else" "match" "loop" "for" "while" "break" "continue" "return"] @keyword.control

            ; Storage keywords
            ["let" "mut" "const" "static"] @keyword.storage

            ; Function keywords
            ["fn" "async"] @keyword.function

            ; Import keywords
            ["use" "extern" "crate"] @keyword.import

            ; Types
            (primitive_type) @type.builtin
            (type_identifier) @type
            (generic_type name: (type_identifier) @type)

            ; Built-in types
            ["i8" "i16" "i32" "i64" "i128" "isize"
             "u8" "u16" "u32" "u64" "u128" "usize"
             "f32" "f64" "bool" "char" "str"] @type.builtin

            ; Functions and methods
            (function_item name: (identifier) @function)
            (function_signature_item name: (identifier) @function)
            (call_expression function: (identifier) @function)
            (call_expression function: (field_expression field: (field_identifier) @method))
            (call_expression function: (scoped_identifier name: (identifier) @function))
            (generic_function function: (identifier) @function)

            ; Macros
            (macro_invocation macro: (identifier) @macro)
            (macro_definition name: (identifier) @macro)
            (attribute_item (identifier) @attribute)
            (derive_macro_invocation (identifier) @derive-macro)

            ; Variables and identifiers
            (identifier) @variable
            (field_identifier) @field
            (shorthand_field_identifier) @field

            ; Parameters
            (parameter pattern: (identifier) @parameter)
            (closure_parameters (identifier) @parameter)

            ; Constants
            (const_item name: (identifier) @constant)
            (static_item name: (identifier) @constant)
            (SCREAMING_SNAKE_CASE) @constant

            ; Modules and namespaces
            (mod_item name: (identifier) @module)
            (scoped_identifier path: (identifier) @namespace)
            (use_declaration argument: (scoped_identifier path: (identifier) @namespace))

            ; Lifetimes
            (lifetime (identifier) @lifetime)
            (lifetime_parameter (identifier) @lifetime)

            ; Labels
            (loop_label (identifier) @label)
            (break_expression (identifier) @label)

            ; Strings and characters
            (string_literal) @string
            (raw_string_literal) @string
            (char_literal) @string
            (format_string) @string

            ; String interpolation
            (format_specifier) @format-specifier

            ; Numbers
            (integer_literal) @number
            (float_literal) @number

            ; Booleans
            ["true" "false"] @boolean

            ; Comments
            (line_comment) @comment
            (block_comment) @comment

            ; Doc comments
            (line_comment 
              (comment_text) @doc-comment 
              (#match? @doc-comment "^///"))
            (block_comment 
              (comment_text) @doc-comment 
              (#match? @doc-comment "^/\\*\\*"))

            ; Operators
            ["+" "-" "*" "/" "%" "=" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "!" "&" "|" 
             "^" "<<" ">>" "+=" "-=" "*=" "/=" "%=" "=>" "->" "?" ":" ".." "..=" "..."] @operator

            ; Punctuation
            ["(" ")" "[" "]" "{" "}"] @punctuation.bracket
            ["," ";" "::" "."] @punctuation.delimiter
            ["#" "@"] @punctuation.special

            ; Attributes
            (attribute_item) @attribute
            (inner_attribute_item) @attribute

            ; Error nodes
            (ERROR) @error
        "##,
        },
    );

    // TOML configuration
    configs.insert(
        "toml",
        LanguageConfig {
            name: "toml",
            language: unsafe { tree_sitter_toml() },
            file_extensions: &["toml"],
            comment_prefix: "#",
            highlight_query: r#"
            (comment) @comment
            (string) @string
            (integer) @number
            (float) @number
            (boolean) @boolean
            (bare_key) @property
            (quoted_key) @property
            (table_header (bare_key) @namespace)
            (table_header (quoted_key) @namespace)
            (array_table_header (bare_key) @namespace)
            (array_table_header (quoted_key) @namespace)
            ["=" "[" "]" "[[" "]]" "{" "}" "," "."] @punctuation
            (ERROR) @error
        "#,
        },
    );

    // JSON configuration
    configs.insert(
        "json",
        LanguageConfig {
            name: "json",
            language: unsafe { tree_sitter_json() },
            file_extensions: &["json"],
            comment_prefix: "",
            highlight_query: r#"
            (string) @string
            (number) @number
            (true) @boolean
            (false) @boolean
            (null) @null
            (pair key: (string) @property)
            ["{" "}" "[" "]" "," ":"] @punctuation
            (ERROR) @error
        "#,
        },
    );

    configs
}

/// Cache entry for syntax highlighting
#[derive(Debug, Clone)]
struct HighlightCache {
    tokens: Vec<Token>,
    version: u64,
    tree: Tree,
    last_updated: Instant,
}

/// Main syntax highlighter
pub struct SyntaxHighlighter {
    /// Parser for the current langauge
    parser: RwLock<Parser>,
    /// Current language configuration
    language_config: Option<LanguageConfig>,
    /// Tree-sitter query for highlighting
    highlight_query: Option<Query>,
    /// Query cursor for executing queries
    query_cursor: RwLock<QueryCursor>,
    /// Cache of highlighted tokens by buffer version
    cache: RwLock<LruCache<u64, HighlightCache>>,
    /// Current syntax tree
    current_tree: RwLock<Option<Tree>>,
    /// Performance metrics
    parse_times: RwLock<Vec<std::time::Duration>>,
}

impl SyntaxHighlighter {
    /// Create a new syntax highlighter
    pub fn new() -> Self {
        let cache_size = NonZeroUsize::new(50).unwrap();

        Self {
            parser: RwLock::new(Parser::new()),
            language_config: None,
            highlight_query: None,
            query_cursor: RwLock::new(QueryCursor::new()),
            cache: RwLock::new(LruCache::new(cache_size)),
            current_tree: RwLock::new(None),
            parse_times: RwLock::new(Vec::new()),
        }
    }

    /// Set the language for syntax highlighting
    #[instrument(skip(self))]
    pub fn set_language(&mut self, language_name: &str) -> Result<()> {
        let configs = get_language_configs();
        let config = configs
            .get(language_name)
            .ok_or_else(|| anyhow::anyhow!("Unsupported language: {}", language_name))?
            .clone();

        {
            let mut parser = self.parser.write();
            parser
                .set_language(config.language)
                .map_err(|e| anyhow::anyhow!("Failed to set language: {}", e))?;
        }

        let query = Query::new(config.language, config.highlight_query)
            .map_err(|e| anyhow::anyhow!("Failed to create highlight query: {}", e))?;

        self.language_config = Some(config);
        self.highlight_query = Some(query);

        debug!("Set syntax highlighting language to: {}", language_name);
        Ok(())
    }

    /// Get the current language name
    pub fn current_language(&self) -> Option<&str> {
        self.language_config.as_ref().map(|config| config.name)
    }

    /// Detect language from file extension
    pub fn detect_language_from_path(&self, path: &str) -> Option<&'static str> {
        let extension = std::path::Path::new(path).extension()?.to_str()?;

        let configs = get_language_configs();
        for (lang_name, config) in configs.iter() {
            if config.file_extensions.contains(&extension) {
                return Some(lang_name);
            }
        }

        None
    }

    /// Parse text and generate syntax tree
    #[instrument(skip(self, text))]
    pub fn parse(&mut self, text: &str) -> Result<()> {
        let start_time = Instant::now();

        let mut parser = self.parser.write();
        let old_tree = self.current_tree.read().clone();

        let new_tree = parser
            .parse(text, old_tree.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse text"))?;

        *self.current_tree.write() = Some(new_tree);

        let parse_time = start_time.elapsed();
        let mut parse_times = self.parse_times.write();
        parse_times.push(parse_time);
        if parse_times.len() > 100 {
            parse_times.remove(0);
        }

        debug!("Parsed {} bytes in {:?}", text.len(), parse_time);
        Ok(())
    }

    /// Get syntax highlighting tokens for the entire buffer
    #[instrument(skip(self, buffer))]
    pub fn highlight_buffer(&mut self, buffer: &TextBuffer) -> Result<Vec<Token>> {
        let version = buffer.version();

        // Check cache first
        if let Some(cached) = self.cache.write().get(&version) {
            debug!("Using cached highlighting for version {}", version);
            return Ok(cached.tokens.clone());
        }

        // Parse if needed
        if self.current_tree.read().is_none() {
            self.parse(&buffer.text())?;
        }

        let tokens = self.generate_tokens(buffer)?;

        // Cache the result
        if let Some(tree) = self.current_tree.read().clone() {
            let cache_entry = HighlightCache {
                tokens: tokens.clone(),
                version,
                tree,
                last_updated: Instant::now(),
            };
            self.cache.write().put(version, cache_entry);
        }

        debug!("Generated {} tokens for version {}", tokens.len(), version);
        Ok(tokens)
    }

    /// Get syntax highlighting tokens for a specific range
    pub fn highlight_range(&mut self, buffer: &TextBuffer, range: &Range) -> Result<Vec<Token>> {
        let all_tokens = self.highlight_buffer(buffer)?;

        let filtered_tokens: Vec<Token> = all_tokens
            .into_iter()
            .filter(|token| token.range.start <= range.end && range.start <= token.range.end)
            .collect();

        Ok(filtered_tokens)
    }

    /// Generate tokens from the current syntax tree
    fn generate_tokens(&self, buffer: &TextBuffer) -> Result<Vec<Token>> {
        let query = self
            .highlight_query
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No highlight query available"))?;

        let tree = self.current_tree.read();
        let tree = tree
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No syntax tree available"))?;

        let mut query_cursor = self.query_cursor.write();
        let root_node = tree.root_node();
        let source_text = buffer.text();

        let matches = query_cursor.matches(query, root_node, source_text.as_bytes());
        let mut tokens = Vec::new();

        for query_match in matches {
            for capture in query_match.captures {
                let node = capture.node;
                let capture_name = &query.capture_names()[capture.index as usize];

                if let Some(token_type) = self.capture_name_to_token_type(capture_name) {
                    if let Ok(token) = self.node_to_token(buffer, node, token_type) {
                        tokens.push(token);
                    }
                }
            }
        }

        tokens.sort_by(|a, b| a.range.start.cmp(&b.range.start));

        self.resolve_token_conflicts(token)
    }

    /// Convert tree-sitter capture name to TokenType
    fn capture_name_to_token_type(&self, capture_name: &str) -> Option<TokenType> {
        match capture_name {
            "comment" => Some(TokenType::Comment),
            "doc-comment" => Some(TokenType::DocComment),
            "string" => Some(TokenType::String),
            "number" => Some(TokenType::Number),
            "boolean" => Some(TokenType::Boolean),
            "null" => Some(TokenType::Null),
            "variable" => Some(TokenType::Variable),
            "parameter" => Some(TokenType::Parameter),
            "field" => Some(TokenType::Field),
            "property" => Some(TokenType::Property),
            "function" => Some(TokenType::Function),
            "method" => Some(TokenType::Method),
            "constructor" => Some(TokenType::Constructor),
            "macro" => Some(TokenType::Macro),
            "derive-macro" => Some(TokenType::DeriveMacro),
            "keyword" => Some(TokenType::Keyword),
            "keyword.control" => Some(TokenType::KeywordControl),
            "keyword.function" => Some(TokenType::KeywordFunction),
            "keyword.return" => Some(TokenType::KeywordReturn),
            "keyword.import" => Some(TokenType::KeywordImport),
            "keyword.storage" => Some(TokenType::KeywordStorage),
            "keyword.operator" => Some(TokenType::KeywordOperator),
            "type" => Some(TokenType::Type),
            "type.builtin" => Some(TokenType::TypeBuiltin),
            "type.parameter" => Some(TokenType::TypeParameter),
            "interface" => Some(TokenType::Interface),
            "struct" => Some(TokenType::Struct),
            "enum" => Some(TokenType::Enum),
            "union" => Some(TokenType::Union),
            "trait" => Some(TokenType::Trait),
            "operator" => Some(TokenType::Operator),
            "punctuation" => Some(TokenType::Punctuation),
            "punctuation.bracket" => Some(TokenType::PunctuationBracket),
            "punctuation.delimiter" => Some(TokenType::PunctuationDelimiter),
            "punctuation.special" => Some(TokenType::PunctuationSpecial),
            "lifetime" => Some(TokenType::Lifetime),
            "label" => Some(TokenType::Label),
            "attribute" => Some(TokenType::Attribute),
            "format-specifier" => Some(TokenType::FormatSpecifier),
            "namespace" => Some(TokenType::Namespace),
            "module" => Some(TokenType::Module),
            "constant" => Some(TokenType::Constant),
            "constant.builtin" => Some(TokenType::ConstantBuiltin),
            "error" => Some(TokenType::Error),
            "warning" => Some(TokenType::Warning),
            "doc-keyword" => Some(TokenType::DocKeyword),
            _ => {
                warn!("Unknown capture name: {}", capture_name);
                None
            }
        }
    }

    /// Convert tree-sitter node to token
    fn node_to_token(
        &self,
        buffer: &TextBuffer,
        node: Node,
        token_type: TokenType,
    ) -> Result<Token> {
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();

        let text = buffer.text();
        let start_pos = self.byte_offset_to_position(&text, start_byte)?;
        let end_pos = self.byte_offset_to_position(&text, end_byte)?;

        let range = Range::new(start_pos, end_pos);
        let token_text = buffer.text_in_range(&range)?;

        Ok(Token::new(range, token_type, token_text))
    }

    /// Convert byte offset to Position
    fn byte_offset_to_position(&self, text: &str, byte_offset: usize) -> Result<Position> {
        let mut line = 0;
        let mut column = 0;
        let mut current_offset = 0;

        for ch in text.chars() {
            if current_offset >= byte_offset {
                break;
            }

            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }

            current_offset += ch.len_utf8();
        }

        Ok(Position::new(line, column))
    }

    /// Resolve conflicts between overlapping tokens
    fn resolve_token_conflicts(&self, tokens: Vec<Token>) -> Result<Vec<Token>> {
        if tokens.is_empty() {
            return Ok(tokens);
        }

        let mut result = Vec::new();
        let mut current_tokens: Vec<Token> = vec![tokens[0].clone()];

        for token in tokens.into_iter().skip(1) {
            let mut has_overlap = false;

            for current_token in &current_tokens {
                if self.tokens_overlap(&token, current_token) {
                    has_overlap = true;
                    break;
                }
            }

            if !has_overlap {
                result.extend(current_tokens.drain(..));
                current_tokens.push(token);
            } else {
                current_tokens.push(token);
            }
        }

        if !current_tokens.is_empty() {
            let resolved = self.resolve_token_group_conflicts(current_tokens);
            result.extend(resolved);
        }

        Ok(result)
    }

    /// Check if two tokens overlap
    fn tokens_overlap(&self, token1: &Token, token2: &Token) -> bool {
        token1.range.start < token2.range.end && token2.range.start < token1.range.end
    }

    /// Resolve conflicts within a group of overlapping tokens
    fn resolve_token_group_conflicts(&self, mut tokens: Vec<Token>) -> Vec<Token> {
        tokens.sort_by(|a, b| {
            b.precedence.cmp(&a.precedence).then_with(|| {
                let a_size = (a.range.end.line - a.range.start.line) * 1000
                    + (a.range.end.column - a.range.start.column);
                let b_size = (b.range.end.line - b.range.start.line) * 1000
                    + (b.range.end.column - b.range.start.column);
                a_size.cmp(&b_size)
            })
        });

        if tokens.is_empty() {
            return tokens;
        }

        let highest_precedence = tokens[0].precedence;
        tokens.retain(|token| token.precedence == highest_precedence);

        tokens
    }

    /// Update highlighting after buffer changes
    #[instrument(skip(self, buffer, event))]
    pub fn update_after_change(
        &mut self,
        buffer: &TextBuffer,
        event: &BufferChangeEvent,
    ) -> Result<()> {
        // For now, we'll do a full re-parse. In the future, we can implement
        // incremental parsing using Tree-sitter's edit functionality.

        // Clear cache for this version
        self.cache.write().pop(&event.version);

        // Re-parse the buffer
        self.parse(&buffer.text())?;

        debug!(
            "Updated syntax highlighting after change, version: {}",
            event.version
        );
        Ok(())
    }

    /// Get performance statistics
    pub fn performance_stats(&self) -> SyntaxPerformanceStats {
        let parse_times = self.parse_times.read();
        let cache = self.cache.read();

        let avg_parse_time = if parse_times.is_empty() {
            std::time::Duration::ZERO
        } else {
            let total: std::time::Duration = parse_times.iter().sum();
            total / parse_times.len() as u32
        };

        SyntaxPerformanceStats {
            average_parse_time: avg_parse_time,
            total_parses: parse_times.len(),
            cache_size: cache.len(),
            cache_hit_rate: 0.0,
        }
    }

    /// Clear all caches
    pub fn clear_cache(&mut self) {
        self.cache.write().clear();
        *self.current_tree.write() = None;
        debug!("Cleared syntax highlighting cache");
    }
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

/// Performance statistics for syntax highlighting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxPerformanceStats {
    pub average_parse_time: std::time::Duration,
    pub total_parses: usize,
    pub cache_size: usize,
    pub cache_hit_rate: f64,
}

/// Theme-aware syntax highlighter that can be integrated with UI themes
pub struct ThemedSyntaxHighlighter {
    highlighter: SyntaxHighlighter,
    theme: SyntaxTheme,
}

/// Syntax highlighting theme
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxTheme {
    pub name: String,
    pub colors: HashMap<TokenType, String>,
    pub styles: HashMap<TokenType, Vec<String>>, // CSS styles like ["bold", "italic"]
}

impl SyntaxTheme {
    /// Create a default dark theme
    pub fn dark_theme() -> Self {
        let mut colors = HashMap::new();
        let mut styles = HashMap::new();

        // Define colors for dark theme
        colors.insert(TokenType::Comment, "#6A9955".to_string());
        colors.insert(TokenType::DocComment, "#608B4E".to_string());
        colors.insert(TokenType::String, "#CE9178".to_string());
        colors.insert(TokenType::Number, "#B5CEA8".to_string());
        colors.insert(TokenType::Boolean, "#569CD6".to_string());
        colors.insert(TokenType::Null, "#569CD6".to_string());
        colors.insert(TokenType::Variable, "#9CDCFE".to_string());
        colors.insert(TokenType::Parameter, "#9CDCFE".to_string());
        colors.insert(TokenType::Field, "#9CDCFE".to_string());
        colors.insert(TokenType::Property, "#9CDCFE".to_string());
        colors.insert(TokenType::Function, "#DCDCAA".to_string());
        colors.insert(TokenType::Method, "#DCDCAA".to_string());
        colors.insert(TokenType::Constructor, "#4EC9B0".to_string());
        colors.insert(TokenType::Macro, "#C586C0".to_string());
        colors.insert(TokenType::DeriveMacro, "#C586C0".to_string());
        colors.insert(TokenType::Keyword, "#569CD6".to_string());
        colors.insert(TokenType::KeywordControl, "#C586C0".to_string());
        colors.insert(TokenType::KeywordFunction, "#569CD6".to_string());
        colors.insert(TokenType::KeywordReturn, "#C586C0".to_string());
        colors.insert(TokenType::KeywordImport, "#569CD6".to_string());
        colors.insert(TokenType::KeywordStorage, "#569CD6".to_string());
        colors.insert(TokenType::KeywordOperator, "#569CD6".to_string());
        colors.insert(TokenType::Type, "#4EC9B0".to_string());
        colors.insert(TokenType::TypeBuiltin, "#569CD6".to_string());
        colors.insert(TokenType::TypeParameter, "#4EC9B0".to_string());
        colors.insert(TokenType::Interface, "#B8D7A3".to_string());
        colors.insert(TokenType::Struct, "#4EC9B0".to_string());
        colors.insert(TokenType::Enum, "#4EC9B0".to_string());
        colors.insert(TokenType::Union, "#4EC9B0".to_string());
        colors.insert(TokenType::Trait, "#B8D7A3".to_string());
        colors.insert(TokenType::Operator, "#D4D4D4".to_string());
        colors.insert(TokenType::Punctuation, "#D4D4D4".to_string());
        colors.insert(TokenType::PunctuationBracket, "#FFD700".to_string());
        colors.insert(TokenType::PunctuationDelimiter, "#D4D4D4".to_string());
        colors.insert(TokenType::PunctuationSpecial, "#C586C0".to_string());
        colors.insert(TokenType::Lifetime, "#4FC1FF".to_string());
        colors.insert(TokenType::Label, "#4FC1FF".to_string());
        colors.insert(TokenType::Attribute, "#C586C0".to_string());
        colors.insert(TokenType::FormatSpecifier, "#D7BA7D".to_string());
        colors.insert(TokenType::Namespace, "#4EC9B0".to_string());
        colors.insert(TokenType::Module, "#4EC9B0".to_string());
        colors.insert(TokenType::Constant, "#4FC1FF".to_string());
        colors.insert(TokenType::ConstantBuiltin, "#569CD6".to_string());
        colors.insert(TokenType::Error, "#F44747".to_string());
        colors.insert(TokenType::Warning, "#FF8C00".to_string());
        colors.insert(TokenType::Text, "#D4D4D4".to_string());

        // Define styles
        styles.insert(TokenType::Comment, vec!["italic".to_string()]);
        styles.insert(TokenType::DocComment, vec!["italic".to_string()]);
        styles.insert(TokenType::Keyword, vec!["bold".to_string()]);
        styles.insert(TokenType::KeywordControl, vec!["bold".to_string()]);
        styles.insert(TokenType::Type, vec!["bold".to_string()]);
        styles.insert(
            TokenType::Error,
            vec!["bold".to_string(), "underline".to_string()],
        );
        styles.insert(TokenType::Warning, vec!["underline".to_string()]);

        Self {
            name: "Dark Theme".to_string(),
            colors,
            styles,
        }
    }

    /// Create a default light theme
    pub fn light_theme() -> Self {
        let mut colors = HashMap::new();
        let mut styles = HashMap::new();

        // Define colors for light theme
        colors.insert(TokenType::Comment, "#008000".to_string());
        colors.insert(TokenType::DocComment, "#629755".to_string());
        colors.insert(TokenType::String, "#A31515".to_string());
        colors.insert(TokenType::Number, "#098658".to_string());
        colors.insert(TokenType::Boolean, "#0000FF".to_string());
        colors.insert(TokenType::Null, "#0000FF".to_string());
        colors.insert(TokenType::Variable, "#001080".to_string());
        colors.insert(TokenType::Parameter, "#001080".to_string());
        colors.insert(TokenType::Field, "#001080".to_string());
        colors.insert(TokenType::Property, "#001080".to_string());
        colors.insert(TokenType::Function, "#795E26".to_string());
        colors.insert(TokenType::Method, "#795E26".to_string());
        colors.insert(TokenType::Constructor, "#267F99".to_string());
        colors.insert(TokenType::Macro, "#AF00DB".to_string());
        colors.insert(TokenType::DeriveMacro, "#AF00DB".to_string());
        colors.insert(TokenType::Keyword, "#0000FF".to_string());
        colors.insert(TokenType::KeywordControl, "#AF00DB".to_string());
        colors.insert(TokenType::KeywordFunction, "#0000FF".to_string());
        colors.insert(TokenType::KeywordReturn, "#AF00DB".to_string());
        colors.insert(TokenType::KeywordImport, "#0000FF".to_string());
        colors.insert(TokenType::KeywordStorage, "#0000FF".to_string());
        colors.insert(TokenType::KeywordOperator, "#0000FF".to_string());
        colors.insert(TokenType::Type, "#267F99".to_string());
        colors.insert(TokenType::TypeBuiltin, "#0000FF".to_string());
        colors.insert(TokenType::TypeParameter, "#267F99".to_string());
        colors.insert(TokenType::Interface, "#267F99".to_string());
        colors.insert(TokenType::Struct, "#267F99".to_string());
        colors.insert(TokenType::Enum, "#267F99".to_string());
        colors.insert(TokenType::Union, "#267F99".to_string());
        colors.insert(TokenType::Trait, "#267F99".to_string());
        colors.insert(TokenType::Operator, "#000000".to_string());
        colors.insert(TokenType::Punctuation, "#000000".to_string());
        colors.insert(TokenType::PunctuationBracket, "#0431FA".to_string());
        colors.insert(TokenType::PunctuationDelimiter, "#000000".to_string());
        colors.insert(TokenType::PunctuationSpecial, "#AF00DB".to_string());
        colors.insert(TokenType::Lifetime, "#0070C1".to_string());
        colors.insert(TokenType::Label, "#0070C1".to_string());
        colors.insert(TokenType::Attribute, "#AF00DB".to_string());
        colors.insert(TokenType::FormatSpecifier, "#EE0000".to_string());
        colors.insert(TokenType::Namespace, "#267F99".to_string());
        colors.insert(TokenType::Module, "#267F99".to_string());
        colors.insert(TokenType::Constant, "#0070C1".to_string());
        colors.insert(TokenType::ConstantBuiltin, "#0000FF".to_string());
        colors.insert(TokenType::Error, "#CD3131".to_string());
        colors.insert(TokenType::Warning, "#B22222".to_string());
        colors.insert(TokenType::Text, "#000000".to_string());

        // Define styles (same as dark theme)
        styles.insert(TokenType::Comment, vec!["italic".to_string()]);
        styles.insert(TokenType::DocComment, vec!["italic".to_string()]);
        styles.insert(TokenType::Keyword, vec!["bold".to_string()]);
        styles.insert(TokenType::KeywordControl, vec!["bold".to_string()]);
        styles.insert(TokenType::Type, vec!["bold".to_string()]);
        styles.insert(
            TokenType::Error,
            vec!["bold".to_string(), "underline".to_string()],
        );
        styles.insert(TokenType::Warning, vec!["underline".to_string()]);

        Self {
            name: "Light Theme".to_string(),
            colors,
            styles,
        }
    }

    /// Get color for a token type
    pub fn get_color(&self, token_type: TokenType) -> Option<&String> {
        self.colors.get(&token_type)
    }

    /// Get styles for a token type
    pub fn get_styles(&self, token_type: TokenType) -> Option<&Vec<String>> {
        self.styles.get(&token_type)
    }

    /// Generate CSS for this theme
    pub fn to_css(&self) -> String {
        let mut css = String::new();

        for (token_type, color) in &self.colors {
            let class_name = token_type.css_class();
            css.push_str(&format!(".syntax-{} {{ color: {}; ", class_name, color));

            if let Some(styles) = self.styles.get(token_type) {
                for style in styles {
                    match style.as_str() {
                        "bold" => css.push_str("font-weight: bold; "),
                        "italic" => css.push_str("font-style: italic; "),
                        "underline" => css.push_str("text-decoration: underline; "),
                        _ => {}
                    }
                }
            }

            css.push_str("}\n");
        }

        css
    }
}

impl ThemedSyntaxHighlighter {
    /// Create a new themed syntax highlighter
    pub fn new(theme: SyntaxTheme) -> Self {
        Self {
            highlighter: SyntaxHighlighter::new(),
            theme,
        }
    }

    /// Create with dark theme
    pub fn with_dark_theme() -> Self {
        Self::new(SyntaxTheme::dark_theme())
    }

    /// Create with light theme
    pub fn with_light_theme() -> Self {
        Self::new(SyntaxTheme::light_theme())
    }

    /// Set the theme
    pub fn set_theme(&mut self, theme: SyntaxTheme) {
        self.theme = theme;
    }

    /// Get the current theme
    pub fn theme(&self) -> &SyntaxTheme {
        &self.theme
    }

    /// Get mutable reference to the highlighter
    pub fn highlighter_mut(&mut self) -> &mut SyntaxHighlighter {
        &mut self.highlighter
    }

    /// Get reference to the highlighter
    pub fn highlighter(&self) -> &SyntaxHighlighter {
        &self.highlighter
    }

    /// Get themed tokens (tokens with color and style information)
    pub fn get_themed_tokens(&mut self, buffer: &TextBuffer) -> Result<Vec<ThemedToken>> {
        let tokens = self.highlighter.highlight_buffer(buffer)?;

        let themed_tokens = tokens
            .into_iter()
            .map(|token| {
                let color = self
                    .theme
                    .get_color(token.token_type)
                    .cloned()
                    .unwrap_or_else(|| "#D4D4D4".to_string()); // Default color
                let styles = self
                    .theme
                    .get_styles(token.token_type)
                    .cloned()
                    .unwrap_or_default();

                ThemedToken {
                    token,
                    color,
                    styles,
                }
            })
            .collect();

        Ok(themed_tokens)
    }

    /// Get themed tokens for a range
    pub fn get_themed_tokens_for_range(
        &mut self,
        buffer: &TextBuffer,
        range: &Range,
    ) -> Result<Vec<ThemedToken>> {
        let tokens = self.highlighter.highlight_range(buffer, range)?;

        let themed_tokens = tokens
            .into_iter()
            .map(|token| {
                let color = self
                    .theme
                    .get_color(token.token_type)
                    .cloned()
                    .unwrap_or_else(|| "#D4D4D4".to_string());
                let styles = self
                    .theme
                    .get_styles(token.token_type)
                    .cloned()
                    .unwrap_or_default();

                ThemedToken {
                    token,
                    color,
                    styles,
                }
            })
            .collect();

        Ok(themed_tokens)
    }
}

/// A token with theme information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemedToken {
    pub token: Token,
    pub color: String,
    pub styles: Vec<String>,
}

impl ThemedToken {
    /// Convert to CSS style string
    pub fn to_css_style(&self) -> String {
        let mut style = format!("color: {};", self.color);

        for css_style in &self.styles {
            match css_style.as_str() {
                "bold" => style.push_str(" font-weight: bold;"),
                "italic" => style.push_str(" font-style: italic;"),
                "underline" => style.push_str(" text-decoration: underline;"),
                _ => {}
            }
        }

        style
    }

    /// Get CSS class names
    pub fn css_classes(&self) -> String {
        let mut classes = vec![format!("syntax-{}", self.token.token_type.css_class())];
        classes.extend(self.styles.iter().map(|s| format!("syntax-{}", s)));
        classes.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::text_buffer::TextBuffer;

    #[test]
    fn test_language_detection() {
        let highlighter = SyntaxHighlighter::new();

        assert_eq!(
            highlighter.detect_language_from_path("main.rs"),
            Some("rust")
        );
        assert_eq!(
            highlighter.detect_language_from_path("Cargo.toml"),
            Some("toml")
        );
        assert_eq!(
            highlighter.detect_language_from_path("config.json"),
            Some("json")
        );
        assert_eq!(highlighter.detect_language_from_path("unknown.xyz"), None);
    }

    #[test]
    fn test_syntax_highlighter_creation() {
        let mut highlighter = SyntaxHighlighter::new();
        assert!(highlighter.current_language().is_none());

        highlighter.set_language("rust").unwrap();
        assert_eq!(highlighter.current_language(), Some("rust"));
    }

    #[test]
    fn test_token_type_css_classes() {
        assert_eq!(TokenType::Keyword.css_class(), "keyword");
        assert_eq!(TokenType::String.css_class(), "string");
        assert_eq!(TokenType::Function.css_class(), "function");
        assert_eq!(TokenType::Comment.css_class(), "comment");
    }

    #[test]
    fn test_token_precedence() {
        let token1 = Token::new(
            Range::new(Position::new(0, 0), Position::new(0, 5)),
            TokenType::String,
            "hello".to_string(),
        );
        let token2 = Token::new(
            Range::new(Position::new(0, 0), Position::new(0, 5)),
            TokenType::Keyword,
            "hello".to_string(),
        );

        // String should have higher precedence than keyword by default
        assert!(token1.precedence > token2.precedence);
    }

    #[test]
    fn test_rust_syntax_highlighting() {
        let mut highlighter = SyntaxHighlighter::new();
        highlighter.set_language("rust").unwrap();

        let code = r#"
fn main() {
    let x: i32 = 42;
    println!("Hello, world! {}", x);
    // This is a comment
}
"#;
        let buffer = TextBuffer::from_content(code, None).unwrap();
        let tokens = highlighter.highlight_buffer(&buffer).unwrap();

        assert!(!tokens.is_empty());

        // Check that we have some expected token types
        let token_types: Vec<TokenType> = tokens.iter().map(|t| t.token_type).collect();
        assert!(token_types.contains(&TokenType::KeywordFunction)); // fn
        assert!(token_types.contains(&TokenType::Function)); // main
        assert!(token_types.contains(&TokenType::KeywordStorage)); // let
        assert!(token_types.contains(&TokenType::TypeBuiltin)); // i32
        assert!(token_types.contains(&TokenType::Number)); // 42
        assert!(token_types.contains(&TokenType::Macro)); // println!
        assert!(token_types.contains(&TokenType::String)); // "Hello, world! {}"
        assert!(token_types.contains(&TokenType::Comment)); // comment
    }

    #[test]
    fn test_theme_creation() {
        let dark_theme = SyntaxTheme::dark_theme();
        assert_eq!(dark_theme.name, "Dark Theme");
        assert!(dark_theme.colors.contains_key(&TokenType::Keyword));
        assert!(dark_theme.styles.contains_key(&TokenType::Comment));

        let light_theme = SyntaxTheme::light_theme();
        assert_eq!(light_theme.name, "Light Theme");
        assert!(light_theme.colors.contains_key(&TokenType::String));
    }

    #[test]
    fn test_themed_highlighter() {
        let mut highlighter = ThemedSyntaxHighlighter::with_dark_theme();
        highlighter.highlighter_mut().set_language("rust").unwrap();

        let code = "fn main() {}";
        let buffer = TextBuffer::from_content(code, None).unwrap();
        let themed_tokens = highlighter.get_themed_tokens(&buffer).unwrap();

        assert!(!themed_tokens.is_empty());

        for themed_token in &themed_tokens {
            assert!(!themed_token.color.is_empty());
            assert!(themed_token.color.starts_with('#'));
        }
    }

    #[test]
    fn test_css_generation() {
        let theme = SyntaxTheme::dark_theme();
        let css = theme.to_css();

        assert!(css.contains(".syntax-keyword"));
        assert!(css.contains("color: #569CD6"));
        assert!(css.contains("font-weight: bold"));
        assert!(css.contains("font-style: italic"));
    }

    #[test]
    fn test_token_range_filtering() {
        let mut highlighter = SyntaxHighlighter::new();
        highlighter.set_language("rust").unwrap();

        let code = "fn main() {\n    let x = 42;\n}";
        let buffer = TextBuffer::from_content(code, None).unwrap();

        // Get tokens for just the second line
        let range = Range::new(Position::new(1, 0), Position::new(1, 15));
        let tokens = highlighter.highlight_range(&buffer, &range).unwrap();

        // Should have tokens, but not as many as the full buffer
        let all_tokens = highlighter.highlight_buffer(&buffer).unwrap();
        assert!(!tokens.is_empty());
        assert!(tokens.len() < all_tokens.len());
    }

    #[test]
    fn test_performance_stats() {
        let mut highlighter = SyntaxHighlighter::new();
        highlighter.set_language("rust").unwrap();

        let code = "fn main() {}";
        let buffer = TextBuffer::from_content(code, None).unwrap();

        // Generate some tokens to create stats
        highlighter.highlight_buffer(&buffer).unwrap();

        let stats = highlighter.performance_stats();
        assert!(stats.total_parses > 0);
        assert!(stats.average_parse_time > std::time::Duration::ZERO);
    }

    #[test]
    fn test_cache_behavior() {
        let mut highlighter = SyntaxHighlighter::new();
        highlighter.set_language("rust").unwrap();

        let code = "fn main() {}";
        let buffer = TextBuffer::from_content(code, None).unwrap();

        // First highlight should parse
        let start = Instant::now();
        let tokens1 = highlighter.highlight_buffer(&buffer).unwrap();
        let first_duration = start.elapsed();

        // Second highlight should use cache (should be faster)
        let start = Instant::now();
        let tokens2 = highlighter.highlight_buffer(&buffer).unwrap();
        let second_duration = start.elapsed();

        assert_eq!(tokens1.len(), tokens2.len());
        // Cache hit should generally be faster, but this is not guaranteed in tests
        // assert!(second_duration < first_duration);
    }
}
