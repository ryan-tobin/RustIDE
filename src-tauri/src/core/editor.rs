use crate::core::{
    cursor::{CursorManager, Direction, MovementUnit, SelectionMode},
    syntax::{SyntaxTheme, ThemedSyntaxHighlighter, ThemedToken},
    text_buffer::{BufferChangeEvent, BufferConfig, Position, Range, TextBuffer, TextEdit},
    traits::EditorEventListener,
    utils, EditorError, EditorResult,
};

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

/// Configuration for the editor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorConfig {
    /// Tab size in spaces
    pub tab_size: usize,
    /// Whether to use tabs or spaces for indentation
    pub use_tabs: bool,
    /// Whether to show line numbers
    pub show_line_numbers: bool,
    /// Whether to show the ruler/column guide
    pub show_ruler: bool,
    /// Column position for the ruler
    pub ruler_column: usize,
    /// Whether to wrap lines
    pub word_wrap: bool,
    /// Whether to show whitespace characters
    pub show_whitespace: bool,
    /// Number of lines to scroll for page up/down
    pub page_scroll_lines: usize,
    /// Whether to highlight the current line
    pub highlight_current_line: bool,
    /// Whether to show matching brackets
    pub show_matching_brackets: bool,
    /// Auto-indent configuration
    pub auto_indent: bool,
    /// Auto-closing brackets
    pub auto_close_brackets: bool,
    /// Font family
    pub font_family: String,
    /// Font size
    pub font_size: f32,
    /// Line height multiplier
    pub line_height: f32,
    /// Maximum number of undo operations
    pub max_undo_operations: usize,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            tab_size: 4,
            use_tabs: false,
            show_line_numbers: true,
            show_ruler: true,
            ruler_column: 100,
            word_wrap: false,
            show_whitespace: false,
            page_scroll_lines: 25,
            highlight_current_line: true,
            show_matching_brackets: true,
            auto_indent: true,
            auto_close_brackets: true,
            font_family: "JetBrains Mono".to_string(),
            font_size: 14.0,
            line_height: 1.4,
            max_undo_operations: 1000,
        }
    }
}

/// Represents the current state of the editor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorState {
    /// Whether the editor has focus
    pub has_focus: bool,
    /// Whether the editor is read-only
    pub is_readonly: bool,
    /// Current file path (if any)
    pub file_path: Option<PathBuf>,
    /// Whether the file has unsaved changes
    pub is_dirty: bool,
    /// Current language mode
    pub language: Option<String>,
    /// Number of lines in the document
    pub line_count: usize,
    /// Number of characters in the document
    pub char_count: usize,
    /// Current encoding
    pub encoding: String,
    /// Line ending style
    pub line_ending: String,
}

/// Viewport/scroll information for the editor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewState {
    /// First visible line (0-indexed)
    pub scroll_top: usize,
    /// Horizontal scroll position in pixels
    pub scroll_left: f32,
    /// Number of visible lines in the viewport
    pub visible_lines: usize,
    /// Width of the editor viewport in pixels
    pub viewport_width: f32,
    /// Height of the editor viewport in pixels
    pub viewport_height: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            scroll_top: 0,
            scroll_left: 0.0,
            visible_lines: 25,
            viewport_width: 800.0,
            viewport_height: 600.0,
        }
    }
}

/// Scroll position information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollInfo {
    /// Total number of lines that can be scrolled
    pub total_lines: usize,
    /// Currently visible range of lines
    pub visible_range: Range,
    /// Whether vertical scrolling is needed
    pub needs_vertical_scroll: bool,
    /// Whether horizontal scrolling is needed  
    pub needs_horizontal_scroll: bool,
    /// Maximum scroll position
    pub max_scroll_top: usize,
    /// Maximum horizontal scroll
    pub max_scroll_left: f32,
}

/// Editor performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorMetrics {
    /// Time taken for last operation
    pub last_operation_time: Duration,
    /// Average time for text operations
    pub average_operation_time: Duration,
    /// Number of operations performed
    pub operation_count: u64,
    /// Memory usage information
    pub memory_usage: usize,
    /// Cache hit rate for syntax highlighting
    pub syntax_cache_hit_rate: f64,
}

/// Search configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Search query
    pub query: String,
    /// Whether to match case
    pub case_sensitive: bool,
    /// Whether to match whole words only
    pub whole_word: bool,
    /// Whether to use regular expressions
    pub use_regex: bool,
    /// Search direction (true = forward, false = backward)
    pub forward: bool,
    /// Whether to wrap around at document boundaries
    pub wrap_around: bool,
}

/// Search result information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Range of the match
    pub range: Range,
    /// Matched text
    pub text: String,
    /// Match index (0-based)
    pub match_index: usize,
    /// Total number of matches
    pub total_matches: usize,
}

/// Events emitted by the editor
#[derive(Debug, Clone, Serialize)]
pub enum EditorEvent {
    /// Text content has changed
    TextChanged {
        version: u64,
        changes: Vec<TextEdit>,
    },
    /// Cursor position has changed
    CursorMoved {
        positions: Vec<Position>,
        selections: Vec<Range>,
    },
    /// Selection has changed
    SelectionChanged {
        has_selection: bool,
        selected_text: String,
        char_count: usize,
    },
    /// File has been saved
    FileSaved { path: PathBuf },
    /// File has been loaded
    FileLoaded {
        path: PathBuf,
        language: Option<String>,
    },
    /// Language mode has changed
    LanguageChanged { language: String },
    /// Configuration has changed
    ConfigChanged { config: EditorConfig },
    /// Search results updated
    SearchResults {
        results: Vec<SearchResult>,
        current_index: Option<usize>,
    },
}

/// The main editor implementation
pub struct Editor {
    /// Unique identifier for this editor instance
    id: Uuid,
    /// Text buffer for content management
    buffer: TextBuffer,
    /// Cursor and selection management
    cursor_manager: CursorManager,
    /// Syntax highlighting
    syntax_highlighter: ThemedSyntaxHighlighter,
    /// Editor configuration
    config: EditorConfig,
    /// Current editor state
    state: EditorState,
    /// Viewport state
    view_state: ViewState,
    /// Event listeners
    event_listeners: Vec<Arc<dyn EditorEventListener>>,
    /// Current search state
    current_search: Option<SearchOptions>,
    /// Recent search results
    search_results: Vec<SearchResult>,
    /// Performance metrics
    metrics: EditorMetrics,
    /// Operation history for metrics
    operation_times: VecDeque<Duration>,
}

impl Editor {
    /// Create a new empty editor
    pub fn new() -> Self {
        Self::with_config(EditorConfig::default())
    }

    /// Create a new editor with custom configuration
    pub fn with_config(config: EditorConfig) -> Self {
        let buffer_config = BufferConfig {
            max_undo_entries: config.max_undo_operations,
            ..BufferConfig::default()
        };

        let buffer = TextBuffer::with_config(buffer_config);
        let mut cursor_manager = CursorManager::new();
        cursor_manager.set_page_size(config.page_scroll_lines);

        let syntax_highlighter = ThemedSyntaxHighlighter::with_dark_theme();

        let state = EditorState {
            has_focus: false,
            is_readonly: false,
            file_path: None,
            is_dirty: false,
            language: None,
            line_count: 1,
            char_count: 0,
            encoding: "UTF-8".to_string(),
            line_ending: "LF".to_string(),
        };

        Self {
            id: Uuid::new_v4(),
            buffer,
            cursor_manager,
            syntax_highlighter,
            config,
            state,
            view_state: ViewState::default(),
            event_listeners: Vec::new(),
            current_search: None,
            search_results: Vec::new(),
            metrics: EditorMetrics {
                last_operation_time: Duration::ZERO,
                average_operation_time: Duration::ZERO,
                operation_count: 0,
                memory_usage: 0,
                syntax_cache_hit_rate: 0.0,
            },
            operation_times: VecDeque::new(),
        }
    }

    /// Get the editor's unique ID
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get reference to the text buffer
    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    /// Get mutable reference to the text buffer
    pub fn buffer_mut(&mut self) -> &mut TextBuffer {
        &mut self.buffer
    }

    /// Get reference to the cursor manager
    pub fn cursor_manager(&self) -> &CursorManager {
        &self.cursor_manager
    }

    /// Get mutable reference to the cursor manager
    pub fn cursor_manager_mut(&mut self) -> &mut CursorManager {
        &mut self.cursor_manager
    }

    /// Get reference to the syntax highlighter
    pub fn syntax_highlighter(&self) -> &ThemedSyntaxHighlighter {
        &self.syntax_highlighter
    }

    /// Get mutable reference to the syntax highlighter
    pub fn syntax_highlighter_mut(&mut self) -> &mut ThemedSyntaxHighlighter {
        &mut self.syntax_highlighter
    }

    /// Get current editor configuration
    pub fn config(&self) -> &EditorConfig {
        &self.config
    }

    /// Get current editor state
    pub fn state(&self) -> &EditorState {
        &self.state
    }

    /// Get current view state
    pub fn view_state(&self) -> &ViewState {
        &self.view_state
    }

    /// Get performance metrics
    pub fn metrics(&self) -> &EditorMetrics {
        &self.metrics
    }

    /// Update editor configuration
    #[instrument(skip(self))]
    pub fn set_config(&mut self, config: EditorConfig) -> EditorResult<()> {
        let old_config = self.config.clone();
        self.config = config;

        // Update dependent components
        self.cursor_manager
            .set_page_size(self.config.page_scroll_lines);

        // Update buffer config if needed
        if old_config.max_undo_operations != self.config.max_undo_operations {
            let mut buffer_config = self.buffer.config().clone();
            buffer_config.max_undo_entries = self.config.max_undo_operations;
            self.buffer.set_config(buffer_config);
        }

        self.emit_event(EditorEvent::ConfigChanged {
            config: self.config.clone(),
        });

        debug!("Updated editor configuration");
        Ok(())
    }

    /// Set syntax highlighting theme
    pub fn set_theme(&mut self, theme: SyntaxTheme) {
        self.syntax_highlighter.set_theme(theme);
        debug!("Updated syntax highlighting theme");
    }

    /// Load file into the editor
    #[instrument(skip(self))]
    pub async fn load_file<P: AsRef<Path>>(&mut self, path: P) -> EditorResult<()> {
        let path = path.as_ref();
        let start_time = Instant::now();

        // Load the file
        self.buffer = TextBuffer::from_file(path.to_path_buf())
            .await
            .context("Failed to load file")?;

        // Detect and set language
        if let Some(language) = self
            .syntax_highlighter
            .highlighter()
            .detect_language_from_path(path.to_str().unwrap_or(""))
        {
            self.syntax_highlighter
                .highlighter_mut()
                .set_language(language)
                .context("Failed to set language")?;
            self.state.language = Some(language.to_string());
        }

        // Update state
        self.state.file_path = Some(path.to_path_buf());
        self.state.is_dirty = false;
        self.update_state_from_buffer();

        // Reset cursor to start
        self.cursor_manager = CursorManager::new();

        // Clear search state
        self.current_search = None;
        self.search_results.clear();

        self.record_operation_time(start_time.elapsed());

        self.emit_event(EditorEvent::FileLoaded {
            path: path.to_path_buf(),
            language: self.state.language.clone(),
        });

        info!("Loaded file: {}", path.display());
        Ok(())
    }

    /// Save the current file
    #[instrument(skip(self))]
    pub async fn save(&mut self) -> EditorResult<()> {
        let start_time = Instant::now();

        self.buffer.save().await.context("Failed to save file")?;

        self.state.is_dirty = false;

        self.record_operation_time(start_time.elapsed());

        if let Some(path) = &self.state.file_path {
            self.emit_event(EditorEvent::FileSaved { path: path.clone() });
        }

        info!("Saved file");
        Ok(())
    }

    /// Save to a specific file
    #[instrument(skip(self))]
    pub async fn save_as<P: AsRef<Path>>(&mut self, path: P) -> EditorResult<()> {
        let path = path.as_ref();
        let start_time = Instant::now();

        self.buffer
            .save_to_file(path.to_path_buf())
            .await
            .context("Failed to save file")?;

        self.state.file_path = Some(path.to_path_buf());
        self.state.is_dirty = false;

        self.record_operation_time(start_time.elapsed());

        self.emit_event(EditorEvent::FileSaved {
            path: path.to_path_buf(),
        });

        info!("Saved file as: {}", path.display());
        Ok(())
    }

    /// Insert text at current cursor positions
    #[instrument(skip(self, text))]
    pub fn insert_text(&mut self, text: &str) -> EditorResult<()> {
        let start_time = Instant::now();

        if self.state.is_readonly {
            return Err(EditorError::SearchError("Editor is read-only".to_string()));
        }

        let mut edits = Vec::new();
        let cursor_positions = self.cursor_manager.cursor_positions();

        // Create text edits for each cursor
        for position in cursor_positions.iter().rev() {
            // Insert in reverse order to maintain position accuracy
            edits.push(TextEdit::insert(*position, text.to_string()));
        }

        // Apply edits
        self.buffer
            .apply_edits(edits.clone())
            .context("Failed to apply text edits")?;

        // Update cursor positions
        self.cursor_manager
            .update_after_edits(&edits)
            .context("Failed to update cursor positions")?;

        self.update_state_from_buffer();
        self.record_operation_time(start_time.elapsed());

        self.emit_event(EditorEvent::TextChanged {
            version: self.buffer.version(),
            changes: edits,
        });

        self.emit_cursor_event();

        debug!("Inserted text: {:?}", text);
        Ok(())
    }

    /// Type a single character (with auto-completion features)
    #[instrument(skip(self))]
    pub fn type_char(&mut self, ch: char) -> EditorResult<()> {
        if self.state.is_readonly {
            return Err(EditorError::SearchError("Editor is read-only".to_string()));
        }

        let mut text = ch.to_string();

        // Handle auto-indentation
        if ch == '\n' && self.config.auto_indent {
            if let Some(indent) = self.calculate_auto_indent()? {
                text.push_str(&indent);
            }
        }

        // Handle auto-closing brackets
        if self.config.auto_close_brackets {
            if let Some(closing) = self.get_auto_close_char(ch) {
                text.push(closing);
                // We'll need to adjust cursor position after insertion
            }
        }

        self.insert_text(&text)?;

        // Move cursor back if we auto-closed a bracket
        if self.config.auto_close_brackets && self.get_auto_close_char(ch).is_some() {
            self.move_cursors(Direction::Left, MovementUnit::Character, false)?;
        }

        Ok(())
    }

    /// Delete text at current selections or at cursor positions
    #[instrument(skip(self))]
    pub fn delete_selection(&mut self) -> EditorResult<()> {
        let start_time = Instant::now();

        if self.state.is_readonly {
            return Err(EditorError::SearchError("Editor is read-only".to_string()));
        }

        let mut edits = Vec::new();

        if self.cursor_manager.has_selection() {
            // Delete selected text
            let ranges = self.cursor_manager.selected_ranges();
            for range in ranges.iter().rev() {
                edits.push(TextEdit::delete(range.clone()));
            }
        } else {
            // Delete character at cursor positions
            for cursor in self.cursor_manager.cursors().iter().rev() {
                let end_pos = self.move_position_right(cursor.position)?;
                if end_pos != cursor.position {
                    let range = Range::new(cursor.position, end_pos);
                    edits.push(TextEdit::delete(range));
                }
            }
        }

        if !edits.is_empty() {
            self.buffer
                .apply_edits(edits.clone())
                .context("Failed to apply delete edits")?;

            self.cursor_manager
                .update_after_edits(&edits)
                .context("Failed to update cursor positions")?;

            self.update_state_from_buffer();
            self.record_operation_time(start_time.elapsed());

            self.emit_event(EditorEvent::TextChanged {
                version: self.buffer.version(),
                changes: vec![], // We don't track the inverse changes for now
            });

            self.emit_cursor_event();

            debug!("Redo operation completed");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Search for text in the buffer
    #[instrument(skip(self))]
    pub fn search(&mut self, options: SearchOptions) -> EditorResult<Vec<SearchResult>> {
        let start_time = Instant::now();

        self.current_search = Some(options.clone());
        let text = self.buffer.text();

        let results = if options.use_regex {
            self.search_regex(&text, &options)?
        } else {
            self.search_literal(&text, &options)?
        };

        self.search_results = results.clone();
        self.record_operation_time(start_time.elapsed());

        self.emit_event(EditorEvent::SearchResults {
            results: results.clone(),
            current_index: None,
        });

        debug!("Search completed: found {} matches", results.len());
        Ok(results)
    }

    /// Find next search result
    pub fn find_next(&mut self) -> EditorResult<Option<SearchResult>> {
        if let Some(options) = &self.current_search.clone() {
            let current_pos = self.cursor_manager.primary_cursor().position;

            for (index, result) in self.search_results.iter().enumerate() {
                if (options.forward && result.range.start > current_pos)
                    || (!options.forward && result.range.start < current_pos)
                {
                    // Move cursor to this result
                    self.cursor_manager
                        .primary_cursor_mut()
                        .select_range(result.range.clone());

                    self.emit_event(EditorEvent::SearchResults {
                        results: self.search_results.clone(),
                        current_index: Some(index),
                    });

                    return Ok(Some(result.clone()));
                }
            }

            // Handle wrap around
            if options.wrap_around && !self.search_results.is_empty() {
                let index = if options.forward {
                    0
                } else {
                    self.search_results.len() - 1
                };
                let result = &self.search_results[index];

                self.cursor_manager
                    .primary_cursor_mut()
                    .select_range(result.range.clone());

                self.emit_event(EditorEvent::SearchResults {
                    results: self.search_results.clone(),
                    current_index: Some(index),
                });

                return Ok(Some(result.clone()));
            }
        }

        Ok(None)
    }

    /// Replace text at current selection
    pub fn replace(&mut self, replacement: &str) -> EditorResult<bool> {
        if self.cursor_manager.has_selection() {
            self.insert_text(replacement)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Replace all occurrences
    pub fn replace_all(&mut self, replacement: &str) -> EditorResult<usize> {
        if self.search_results.is_empty() {
            return Ok(0);
        }

        let mut edits = Vec::new();

        // Create replacement edits in reverse order
        for result in self.search_results.iter().rev() {
            edits.push(TextEdit::replace(
                result.range.clone(),
                replacement.to_string(),
            ));
        }

        let count = edits.len();

        self.buffer
            .apply_edits(edits.clone())
            .context("Failed to apply replacement edits")?;

        self.cursor_manager
            .update_after_edits(&edits)
            .context("Failed to update cursor positions")?;

        // Clear search results since positions have changed
        self.search_results.clear();
        self.current_search = None;

        self.update_state_from_buffer();

        self.emit_event(EditorEvent::TextChanged {
            version: self.buffer.version(),
            changes: edits,
        });

        debug!("Replaced {} occurrences", count);
        Ok(count)
    }

    /// Get highlighted tokens for the visible area
    pub fn get_visible_tokens(&mut self) -> EditorResult<Vec<ThemedToken>> {
        let visible_range = self.get_visible_range();
        self.syntax_highlighter
            .get_themed_tokens_for_range(&self.buffer, &visible_range)
            .map_err(|e| EditorError::SyntaxError(e.to_string()))
    }

    /// Get all highlighted tokens
    pub fn get_all_tokens(&mut self) -> EditorResult<Vec<ThemedToken>> {
        self.syntax_highlighter
            .get_themed_tokens(&self.buffer)
            .map_err(|e| EditorError::SyntaxError(e.to_string()))
    }

    /// Update view state (called by UI layer)
    pub fn update_view_state(&mut self, view_state: ViewState) {
        self.view_state = view_state;
    }

    /// Scroll to ensure position is visible
    pub fn scroll_to_position(&mut self, position: Position) {
        let line = position.line;

        // Adjust vertical scroll if needed
        if line < self.view_state.scroll_top {
            self.view_state.scroll_top = line;
        } else if line >= self.view_state.scroll_top + self.view_state.visible_lines {
            self.view_state.scroll_top = line.saturating_sub(self.view_state.visible_lines - 1);
        }

        debug!("Scrolled to position {}", position);
    }

    /// Get scroll information
    pub fn get_scroll_info(&self) -> ScrollInfo {
        let total_lines = self.buffer.len_lines();
        let visible_start = self.view_state.scroll_top;
        let visible_end = (visible_start + self.view_state.visible_lines).min(total_lines);

        ScrollInfo {
            total_lines,
            visible_range: Range::new(
                Position::new(visible_start, 0),
                Position::new(visible_end, 0),
            ),
            needs_vertical_scroll: total_lines > self.view_state.visible_lines,
            needs_horizontal_scroll: false, // We'll implement this later
            max_scroll_top: total_lines.saturating_sub(self.view_state.visible_lines),
            max_scroll_left: 0.0, // We'll implement this later
        }
    }

    /// Add an event listener
    pub fn add_event_listener(&mut self, listener: Arc<dyn EditorEventListener>) {
        self.event_listeners.push(listener);
    }

    /// Set read-only mode
    pub fn set_readonly(&mut self, readonly: bool) {
        self.state.is_readonly = readonly;
        debug!("Set read-only mode: {}", readonly);
    }

    /// Set focus state
    pub fn set_focus(&mut self, has_focus: bool) {
        self.state.has_focus = has_focus;
        debug!("Set focus: {}", has_focus);
    }

    /// Get current line content
    pub fn current_line(&self) -> EditorResult<String> {
        let line_num = self.cursor_manager.primary_cursor().position.line;
        self.buffer
            .line_text(line_num)
            .map_err(|e| EditorError::BufferError(e))
    }

    /// Go to specific line and column
    pub fn goto_position(&mut self, position: Position) -> EditorResult<()> {
        // Validate position
        if position.line >= self.buffer.len_lines() {
            return Err(EditorError::InvalidPosition { position });
        }

        let line_len = self
            .buffer
            .line_len(position.line)
            .map_err(|e| EditorError::BufferError(e))?;

        if position.column > line_len {
            return Err(EditorError::InvalidPosition { position });
        }

        // Move cursor
        self.cursor_manager.clear_secondary_cursors();
        self.cursor_manager.primary_cursor_mut().move_to(position);

        // Ensure position is visible
        self.scroll_to_position(position);

        self.emit_cursor_event();
        debug!("Moved to position {}", position);
        Ok(())
    }

    /// Go to specific line number (1-indexed for user interface)
    pub fn goto_line(&mut self, line_number: usize) -> EditorResult<()> {
        if line_number == 0 {
            return Err(EditorError::InvalidPosition {
                position: Position::new(0, 0),
            });
        }

        let position = Position::new(line_number - 1, 0);
        self.goto_position(position)
    }

    /// Indent selected lines or current line
    pub fn indent_lines(&mut self) -> EditorResult<()> {
        if self.state.is_readonly {
            return Err(EditorError::SearchError("Editor is read-only".to_string()));
        }

        let indent_text = utils::create_indentation(1, self.config.use_tabs, self.config.tab_size);
        let mut edits = Vec::new();

        if self.cursor_manager.has_selection() {
            // Indent all lines that have selections
            let ranges = self.cursor_manager.selected_ranges();
            for range in &ranges {
                for line in range.start.line..=range.end.line {
                    edits.push(TextEdit::insert(
                        Position::new(line, 0),
                        indent_text.clone(),
                    ));
                }
            }
        } else {
            // Indent current lines for all cursors
            for cursor in self.cursor_manager.cursors() {
                edits.push(TextEdit::insert(
                    Position::new(cursor.position.line, 0),
                    indent_text.clone(),
                ));
            }
        }

        self.buffer
            .apply_edits(edits.clone())
            .context("Failed to apply indent edits")?;

        self.cursor_manager
            .update_after_edits(&edits)
            .context("Failed to update cursor positions")?;

        self.update_state_from_buffer();

        self.emit_event(EditorEvent::TextChanged {
            version: self.buffer.version(),
            changes: edits,
        });

        debug!("Indented lines");
        Ok(())
    }

    /// Unindent selected lines or current line
    pub fn unindent_lines(&mut self) -> EditorResult<()> {
        if self.state.is_readonly {
            return Err(EditorError::SearchError("Editor is read-only".to_string()));
        }

        let mut edits = Vec::new();
        let tab_size = self.config.tab_size;

        let lines_to_unindent: Vec<usize> = if self.cursor_manager.has_selection() {
            let ranges = self.cursor_manager.selected_ranges();
            ranges
                .iter()
                .flat_map(|range| range.start.line..=range.end.line)
                .collect()
        } else {
            self.cursor_manager
                .cursors()
                .iter()
                .map(|cursor| cursor.position.line)
                .collect()
        };

        for line_num in lines_to_unindent.iter().rev() {
            let line_text = self
                .buffer
                .line_text(*line_num)
                .map_err(|e| EditorError::BufferError(e))?;

            let mut chars_to_remove = 0;
            let chars: Vec<char> = line_text.chars().collect();

            if !chars.is_empty() && chars[0] == '\t' {
                chars_to_remove = 1;
            } else {
                let spaces = chars
                    .iter()
                    .take(tab_size)
                    .take_while(|&&c| c == ' ')
                    .count();
                chars_to_remove = spaces.min(tab_size);
            }

            if chars_to_remove > 0 {
                let range = Range::new(
                    Position::new(*line_num, 0),
                    Position::new(*line_num, chars_to_remove),
                );
                edits.push(TextEdit::delete(range));
            }
        }

        if !edits.is_empty() {
            self.buffer
                .apply_edits(edits.clone())
                .context("Failed to apply unindent edits")?;

            self.cursor_manager
                .update_after_edits(&edits)
                .context("Failed to update cursor positions")?;

            self.update_state_from_buffer();

            self.emit_event(EditorEvent::TextChanged {
                version: self.buffer.version(),
                changes: edits,
            });
        }

        debug!("Unindented lines");
        Ok(())
    }

    /// Comment/uncomment selected lines
    pub fn toggle_line_comment(&mut self) -> EditorResult<()> {
        if self.state.is_readonly {
            return Err(EditorError::SearchError("Editor is read-only".to_string()));
        }

        // Get comment prefix for current language
        let comment_prefix = self.get_comment_prefix();
        if comment_prefix.is_empty() {
            return Ok(()); // No comment syntax for this language
        }

        let lines_to_toggle: Vec<usize> = if self.cursor_manager.has_selection() {
            let ranges = self.cursor_manager.selected_ranges();
            ranges
                .iter()
                .flat_map(|range| range.start.line..=range.end.line)
                .collect()
        } else {
            self.cursor_manager
                .cursors()
                .iter()
                .map(|cursor| cursor.position.line)
                .collect()
        };

        // Check if all lines are already commented
        let all_commented = lines_to_toggle.iter().all(|&line_num| {
            if let Ok(line_text) = self.buffer.line_text(line_num) {
                line_text.trim_start().starts_with(&comment_prefix)
            } else {
                false
            }
        });

        let mut edits = Vec::new();

        for line_num in lines_to_toggle.iter().rev() {
            let line_text = self
                .buffer
                .line_text(*line_num)
                .map_err(|e| EditorError::BufferError(e))?;

            if all_commented {
                // Uncomment: remove comment prefix
                if let Some(pos) = line_text.find(&comment_prefix) {
                    let range = Range::new(
                        Position::new(*line_num, pos),
                        Position::new(*line_num, pos + comment_prefix.len()),
                    );
                    edits.push(TextEdit::delete(range));
                }
            } else {
                // Comment: add comment prefix at start of line (after indentation)
                let indent_len = utils::get_line_indentation(&line_text);
                edits.push(TextEdit::insert(
                    Position::new(*line_num, indent_len),
                    format!("{} ", comment_prefix),
                ));
            }
        }

        if !edits.is_empty() {
            self.buffer
                .apply_edits(edits.clone())
                .context("Failed to apply comment edits")?;

            self.cursor_manager
                .update_after_edits(&edits)
                .context("Failed to update cursor positions")?;

            self.update_state_from_buffer();

            self.emit_event(EditorEvent::TextChanged {
                version: self.buffer.version(),
                changes: edits,
            });
        }

        debug!("Toggled line comments");
        Ok(())
    }

    // Helper methods

    /// Update editor state from buffer
    fn update_state_from_buffer(&mut self) {
        self.state.line_count = self.buffer.len_lines();
        self.state.char_count = self.buffer.len_chars();
        self.state.is_dirty = self.buffer.is_dirty();
        self.state.line_ending = match self.buffer.line_ending() {
            crate::core::text_buffer::LineEnding::Unix => "LF".to_string(),
            crate::core::text_buffer::LineEnding::Windows => "CRLF".to_string(),
            crate::core::text_buffer::LineEnding::Mac => "CR".to_string(),
        };
    }

    /// Emit cursor position event
    fn emit_cursor_event(&self) {
        let positions = self.cursor_manager.cursor_positions();
        let selections = self.cursor_manager.selected_ranges();

        self.emit_event(EditorEvent::CursorMoved {
            positions,
            selections: selections.clone(),
        });

        // Also emit selection changed event
        let has_selection = self.cursor_manager.has_selection();
        let selected_text = if has_selection {
            self.cursor_manager
                .cursors()
                .iter()
                .filter_map(|cursor| cursor.selected_text(&self.buffer).ok())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };

        let char_count = selected_text.len();

        self.emit_event(EditorEvent::SelectionChanged {
            has_selection,
            selected_text,
            char_count,
        });
    }

    /// Emit an event to all listeners
    fn emit_event(&self, event: EditorEvent) {
        for listener in &self.event_listeners {
            match &event {
                EditorEvent::TextChanged { .. } => {
                    // We'll need to convert this to BufferChangeEvent format
                    // For now, we'll skip this to avoid complex conversions
                }
                EditorEvent::CursorMoved { positions, .. } => {
                    listener.on_cursor_moved(positions);
                }
                EditorEvent::SelectionChanged { has_selection, .. } => {
                    listener.on_selection_changed(*has_selection);
                }
                EditorEvent::FileSaved { path } => {
                    listener.on_file_saved(path);
                }
                _ => {} // Other events don't have corresponding trait methods
            }
        }
    }

    /// Record operation time for metrics
    fn record_operation_time(&mut self, duration: Duration) {
        self.metrics.last_operation_time = duration;
        self.metrics.operation_count += 1;

        self.operation_times.push_back(duration);
        if self.operation_times.len() > 100 {
            self.operation_times.pop_front();
        }

        // Calculate average
        if !self.operation_times.is_empty() {
            let total: Duration = self.operation_times.iter().sum();
            self.metrics.average_operation_time = total / self.operation_times.len() as u32;
        }
    }

    /// Get visible range based on current view state
    fn get_visible_range(&self) -> Range {
        let start_line = self.view_state.scroll_top;
        let end_line = (start_line + self.view_state.visible_lines).min(self.buffer.len_lines());

        Range::new(Position::new(start_line, 0), Position::new(end_line, 0))
    }

    /// Move position one character to the right
    fn move_position_right(&self, position: Position) -> EditorResult<Position> {
        let line_len = self
            .buffer
            .line_len(position.line)
            .map_err(|e| EditorError::BufferError(e))?;

        if position.column < line_len {
            Ok(Position::new(position.line, position.column + 1))
        } else if position.line < self.buffer.len_lines() - 1 {
            Ok(Position::new(position.line + 1, 0))
        } else {
            Ok(position) // At end of document
        }
    }

    /// Move position one character to the left
    fn move_position_left(&self, position: Position) -> EditorResult<Position> {
        if position.column > 0 {
            Ok(Position::new(position.line, position.column - 1))
        } else if position.line > 0 {
            let prev_line = position.line - 1;
            let line_len = self
                .buffer
                .line_len(prev_line)
                .map_err(|e| EditorError::BufferError(e))?;
            Ok(Position::new(prev_line, line_len))
        } else {
            Ok(position) // At start of document
        }
    }

    /// Calculate auto-indent for new line
    fn calculate_auto_indent(&self) -> EditorResult<Option<String>> {
        let cursor_pos = self.cursor_manager.primary_cursor().position;
        if cursor_pos.line == 0 {
            return Ok(None);
        }

        let current_line = self
            .buffer
            .line_text(cursor_pos.line)
            .map_err(|e| EditorError::BufferError(e))?;

        let indent_level = utils::get_line_indentation(&current_line);
        let indent =
            utils::create_indentation(indent_level, self.config.use_tabs, self.config.tab_size);

        Ok(Some(indent))
    }

    /// Get auto-closing character for the given opening character
    fn get_auto_close_char(&self, ch: char) -> Option<char> {
        match ch {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '"' => Some('"'),
            '\'' => Some('\''),
            _ => None,
        }
    }

    /// Get comment prefix for current language
    fn get_comment_prefix(&self) -> String {
        // This could be expanded to use language-specific comment syntax
        match self.state.language.as_deref() {
            Some("rust") => "//".to_string(),
            Some("toml") => "#".to_string(),
            Some("json") => "//".to_string(), // JSON doesn't have comments, but we'll use this
            _ => "//".to_string(),            // Default
        }
    }

    /// Search for literal text
    fn search_literal(
        &self,
        text: &str,
        options: &SearchOptions,
    ) -> EditorResult<Vec<SearchResult>> {
        let query = if options.case_sensitive {
            options.query.clone()
        } else {
            options.query.to_lowercase()
        };

        let search_text = if options.case_sensitive {
            text.to_string()
        } else {
            text.to_lowercase()
        };

        let mut results = Vec::new();
        let mut start_pos = 0;

        while let Some(pos) = search_text[start_pos..].find(&query) {
            let actual_pos = start_pos + pos;
            let end_pos = actual_pos + query.len();

            // Convert byte positions to line/column positions
            let start_position = self.byte_to_position(text, actual_pos)?;
            let end_position = self.byte_to_position(text, end_pos)?;

            // Check whole word constraint
            if options.whole_word && !self.is_whole_word_match(text, actual_pos, end_pos) {
                start_pos = actual_pos + 1;
                continue;
            }

            results.push(SearchResult {
                range: Range::new(start_position, end_position),
                text: text[actual_pos..end_pos].to_string(),
                match_index: results.len(),
                total_matches: 0, // Will be updated after all matches are found
            });

            start_pos = actual_pos + 1;
        }

        // Update total_matches count
        for result in &mut results {
            result.total_matches = results.len();
        }

        Ok(results)
    }

    /// Search using regular expressions
    fn search_regex(&self, text: &str, options: &SearchOptions) -> EditorResult<Vec<SearchResult>> {
        let regex = if options.case_sensitive {
            Regex::new(&options.query)
        } else {
            Regex::new(&format!("(?i){}", options.query))
        }
        .map_err(|e| EditorError::SearchError(format!("Invalid regex: {}", e)))?;

        let mut results = Vec::new();

        for mat in regex.find_iter(text) {
            let start_pos = mat.start();
            let end_pos = mat.end();

            let start_position = self.byte_to_position(text, start_pos)?;
            let end_position = self.byte_to_position(text, end_pos)?;

            results.push(SearchResult {
                range: Range::new(start_position, end_position),
                text: mat.as_str().to_string(),
                match_index: results.len(),
                total_matches: 0, // Will be updated after all matches are found
            });
        }

        // Update total_matches count
        for result in &mut results {
            result.total_matches = results.len();
        }

        Ok(results)
    }

    /// Convert byte position to line/column position
    fn byte_to_position(&self, text: &str, byte_pos: usize) -> EditorResult<Position> {
        let mut line = 0;
        let mut column = 0;
        let mut current_byte = 0;

        for ch in text.chars() {
            if current_byte >= byte_pos {
                break;
            }

            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }

            current_byte += ch.len_utf8();
        }

        Ok(Position::new(line, column))
    }

    /// Check if a match is a whole word
    fn is_whole_word_match(&self, text: &str, start: usize, end: usize) -> bool {
        let chars: Vec<char> = text.chars().collect();

        // Check character before match
        if start > 0 && utils::is_word_char(chars[start - 1]) {
            return false;
        }

        // Check character after match
        if end < chars.len() && utils::is_word_char(chars[end]) {
            return false;
        }

        true
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_creation() {
        let editor = Editor::new();
        assert_eq!(editor.state().line_count, 1);
        assert_eq!(editor.state().char_count, 0);
        assert!(!editor.state().is_dirty);
        assert!(!editor.state().is_readonly);
    }

    #[test]
    fn test_text_insertion() {
        let mut editor = Editor::new();
        editor.insert_text("Hello, World!").unwrap();

        assert_eq!(editor.buffer().text(), "Hello, World!");
        assert!(editor.state().is_dirty);
        assert_eq!(editor.state().char_count, 13);
    }

    #[test]
    fn test_cursor_movement() {
        let mut editor = Editor::new();
        editor.insert_text("Line 1\nLine 2\nLine 3").unwrap();

        // Move down
        editor
            .move_cursors(Direction::Down, MovementUnit::Line, false)
            .unwrap();
        assert_eq!(
            editor.cursor_manager().primary_cursor().position,
            Position::new(2, 0)
        );

        // Test invalid line number
        assert!(editor.goto_line(0).is_err());
        assert!(editor.goto_line(100).is_err());
    }

    #[test]
    fn test_replace_operations() {
        let mut editor = Editor::new();
        editor.insert_text("Hello World\nHello Rust").unwrap();

        let options = SearchOptions {
            query: "Hello".to_string(),
            case_sensitive: true,
            whole_word: false,
            use_regex: false,
            forward: true,
            wrap_around: true,
        };

        editor.search(options).unwrap();
        let replaced = editor.replace_all("Hi").unwrap();

        assert_eq!(replaced, 2);
        assert_eq!(editor.buffer().text(), "Hi World\nHi Rust");
    }

    #[test]
    fn test_readonly_mode() {
        let mut editor = Editor::new();
        editor.set_readonly(true);

        let result = editor.insert_text("Hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_performance_metrics() {
        let mut editor = Editor::new();

        // Perform some operations
        editor.insert_text("Hello").unwrap();
        editor.insert_text(" World").unwrap();

        let metrics = editor.metrics();
        assert!(metrics.operation_count > 0);
        assert!(metrics.last_operation_time > Duration::ZERO);
    }

    #[test]
    fn test_view_state_management() {
        let mut editor = Editor::new();

        let view_state = ViewState {
            scroll_top: 10,
            scroll_left: 50.0,
            visible_lines: 30,
            viewport_width: 1000.0,
            viewport_height: 800.0,
        };

        editor.update_view_state(view_state.clone());
        assert_eq!(editor.view_state().scroll_top, 10);
        assert_eq!(editor.view_state().visible_lines, 30);
    }

    #[test]
    fn test_scroll_to_position() {
        let mut editor = Editor::new();

        // Create a document with many lines
        let lines: Vec<String> = (0..100).map(|i| format!("Line {}", i)).collect();
        editor.insert_text(&lines.join("\n")).unwrap();

        // Scroll to line 50
        editor.scroll_to_position(Position::new(50, 0));

        let scroll_info = editor.get_scroll_info();
        assert!(scroll_info.visible_range.start.line <= 50);
        assert!(scroll_info.visible_range.end.line > 50);
    }

    #[test]
    fn test_auto_indentation() {
        let mut editor = Editor::with_config(EditorConfig {
            auto_indent: true,
            tab_size: 4,
            use_tabs: false,
            ..EditorConfig::default()
        });

        editor.insert_text("    fn main() {").unwrap();
        editor.type_char('\n').unwrap();

        let current_line = editor.current_line().unwrap();
        assert!(current_line.starts_with("    ")); // Should maintain indentation
    }

    #[test]
    fn test_auto_close_brackets() {
        let mut editor = Editor::with_config(EditorConfig {
            auto_close_brackets: true,
            ..EditorConfig::default()
        });

        editor.type_char('(').unwrap();
        assert_eq!(editor.buffer().text(), "()");

        // Cursor should be between brackets
        assert_eq!(
            editor.cursor_manager().primary_cursor().position,
            Position::new(0, 1)
        );
    }

    #[test]
    fn test_regex_search() {
        let mut editor = Editor::new();
        editor.insert_text("test123\ntest456\nabc789").unwrap();

        let options = SearchOptions {
            query: r"test\d+".to_string(),
            case_sensitive: true,
            whole_word: false,
            use_regex: true,
            forward: true,
            wrap_around: true,
        };

        let results = editor.search(options).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].text, "test123");
        assert_eq!(results[1].text, "test456");
    }
}
