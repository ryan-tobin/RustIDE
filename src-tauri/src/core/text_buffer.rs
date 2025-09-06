use anyhow::{Context, Result};
use ropey::{Rope, RopeSlice};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use tracing::{debug, instrument, warn};

/// Represents a position in the text buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    pub fn zero() -> Self {
        Self { line: 0, column: 0 }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.column + 1)
    }
}

/// Represents a range in the text buffer
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn single_point(position: Position) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, position: Position) -> bool {
        self.start <= position && position <= self.end
    }
}

// Represents a text edit operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
    pub timestamp: std::time::SystemTime,
}

impl TextEdit {
    pub fn new(range: Range, new_text: String) -> Self {
        Self {
            range,
            new_text,
            timestamp: std::time::SystemTime::now(),
        }
    }

    /// Create an insertion edit
    pub fn insert(position: Position, text: String) -> Self {
        Self::new(Range::single_point(position), text)
    }

    /// Create a deletion edit
    pub fn delete(range: Range) -> Self {
        Self::new(range, String::new())
    }

    /// Create a replacement edit
    pub fn replace(range: Range, text: String) -> Self {
        Self::new(range, text)
    }
}

/// Undo/Redo entry containing multiple edits that should be treated as one operation
#[debug(Debug, Clone)]
pub struct UndoRedoEntry {
    pub edits: Vec<TextEdit>,
    pub cursor_before: Vec<Position>,
    pub cursor_after: Vec<Position>,
    pub timestamp: std::time::SystemTime,
}

impl UndoRedoEntry {
    pub fn new(
        edits: Vec<TextEdit>,
        cursor_before: Vec<Position>,
        cursor_after: Vec<Position>,
    ) -> Self {
        Self {
            edits,
            cursor_before,
            cursor_after,
            timestamp: std::time::SystemTime::now(),
        }
    }
}

// Event fired when the buffer changes
#[derive(Debug, Clone, Serialize)]
pub struct BufferChangeEvent {
    pub version: u64,
    pub edits: Vec<TextEdit>,
    pub full_text_length: usize,
    pub line_count: usize,
}

/// Configuration for the text buffer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferConfig {
    /// Maximum number of undo/redo entries to keep
    pub max_undo_entries: usize,
    /// Whether to automatically detect line endings
    pub auto_detect_line_endings: bool,
    /// Default line ending style
    pub line_ending: LineEnding,
    /// Whether to trim trailing whitespace on save
    pub trim_trailing_whitespace: bool,
    /// Whether to ensure file ends with newline
    pub insert_final_newline: bool,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            max_undo_entries: 1000,
            auto_detect_line_endings: true,
            line_ending: LineEnding::default(),
            trim_trailing_whitespace: true,
            insert_final_newline: true,
        }
    }
}

/// Line ending styles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    /// Unix-style line endings (\n)
    Unix,
    /// Windows-style line endings (\r\n)
    Windows,
    /// Classic Mac-style line endings (\r)
    Mac,
}

impl Default for LineEnding {
    fn default() -> Self {
        if cfg!(windows) {
            LineEnding::Windows
        } else {
            LineEnding::Unix
        }
    }
}

impl LineEnding {
    pub fn as_str(&self) -> &'static str {
        match self {
            LineEnding::Unix => "\n",
            LineEnding::Windows => "\r\n",
            LineEnding::Mac => "\r",
        }
    }

    /// Detect line ending from text context
    pub fn detect(text: &str) -> Self {
        let mut crlf_count = 0;
        let mut lf_count = 0;
        let mut cr_count = 0;

        let chars: Vec<char> = text.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            match ch {
                '\r' => {
                    if i + 1 < chars.len() && chars[i + 1] == '\n' {
                        crlf_count += 1;
                    } else {
                        cr_count += 1;
                    }
                }
                '\n' => {
                    if i == 0 || chars[i + 1] != '\r' {
                        lf_count += 1;
                    }
                }
                _ => {}
            }
        }

        if crlf_count >= lf_count && crlf_count >= cr_count {
            LineEnding::Windows
        } else if cr_count >= lf_count {
            LineEnding::Mac
        } else {
            LineEnding::Unix
        }
    }
}

/// The main text buffer implementation
pub struct TextBuffer {
    /// The rope data structure holding the text
    rope: Rope,
    /// Current version number (incremented on each change)
    version: u64,
    /// Whether the buffer has unsaved changes
    dirty: bool,
    /// File path (if associated with a file)
    file_path: Option<PathBuf>,
    /// Line ending style for this buffer
    line_ending: LineEnding,
    /// Buffer configuration
    config: BufferConfig,
    /// Undo stack
    undo_stack: VecDeque<UndoRedoEntry>,
    /// Redo stack
    redo_stack: VecDeque<UndoRedoEntry>,
    /// Change event listeners
    change_listeners: Vec<Box<dyn Fn(&BufferChangeEvent) + Send + Sync>>,
}

impl TextBuffer {
    /// Create a new empty text buffer
    pub fn new() -> Self {
        Self::with_config(BufferConfig::default())
    }

    /// Create a new text buffer with custom configuration
    pub fn with_config(config: BufferConfig) -> Self {
        let rope: Rope::new();
        debug!("Created new text buffer with {} lines", rope.len_lines());

        Self {
            rope,
            version: 0,
            dirty: false,
            file_path: None,
            line_ending: config.line_ending,
            config,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            change_listeners: Vec::new(),
        }
    }

    /// Create a text buffer from file content
    #[instrument(skip(content))]
    pub fn from_content(content: &str, file_path: Option<PathBuf>) -> Result<Self> {
        let config = BufferConfig::default();
        let line_ending = if config.auto_detect_line_endings {
            LineEnding::detect(content)
        } else {
            config.line_ending
        };

        let rope = Rope::from_str(content);
        debug!(
            "Created text buffer from content: {} chars, {} lines, line ending: {:?}",
            rope.len_chars(),
            rope.len_lines(),
            line_ending
        );

        Ok(Self {
            rope,
            version: 0,
            dirty: false,
            file_path,
            line_ending,
            config,
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            change_listeners: Vec::new(),
        })
    }

    /// Load text buffer from file
    #[instrument]
    pub async fn from_file(path: PathBuf) -> Result<Self> {
        let content = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        Self::from_content(&content, Some(path))
    }

    /// Get the current version of the buffer
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Check if buffer has unsaved changes
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Get the file path associted with this buffer
    pub fn file_path(&self) -> Option<&PathBuf> {
        self.file_path.as_ref()
    }

    /// Set the file path for this buffer
    pub fn set_file_path(&mut self, path: Option<PathBuf>) {
        self.file_path = path;
    }

    /// Get the line ending style
    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Set the line ending style
    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
        self.mark_dirty();
    }

    /// Get buffer configuration
    pub fn config(&self) -> &BufferConfig {
        &self.config
    }

    /// Update buffer configuration
    pub fn set_config(&mut self, config: BufferConfig) {
        self.config = config;
    }

    /// Get the total number of characters in the buffer
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Get the total number of lines in the buffer
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    /// Get the entire text content
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Get a slice of text within the given range
    pub fn text_in_range(&self, range: &Range) -> Result<String> {
        let start_char = self.position_to_char_index(range.start)?;
        let end_char = self.position_to_char_index(range.end)?;

        if start_char > end_char {
            return Err(anyhow::anyhow!("Invalid range: start > end"));
        }

        Ok(self.rope.slice(start_char..end_char).to_string())
    }

    /// Get text of a specific line (0-indexed)
    pub fn line_text(&self, line: usize) -> Result<String> {
        if line >= self.len_lines() {
            return Err(anyhow::anyhow!("Line {} is out of bounds", line));
        }

        let line_slice = self.rope.line(line);
        Ok(line_slice.to_string())
    }

    /// Get the length of a specific line (excluding line ending)
    pub fn line_len(&self, line: usize) -> Result<usize> {
        if line >= self.len_lines() {
            return Err(anyhow::anyhow!("Line {} is out of bounds", line));
        }

        let line_slice = self.rope.line(line);
        let mut len = line_slice.len_chars();

        let line_str = line_slice.to_string();
        if line_str.ends_with("\r\n") {
            len = len.saturating_sub(2);
        } else if line_str.ends_with('\n') || line_str.ends_with('\r') {
            len = len.saturating_sub(1);
        }

        Ok(len)
    }

    /// Convert a position to a character index
    pub fn position_to_char_index(&self, position: Position) -> Result<usize> {
        if position.line >= self.len_lines() {
            return Err(anyhow::anyhow!("Line {} is out of bounds", position.line));
        }

        let line_start_char = self.rope.line_to_char(position.line);
        let line_len = self.line_len(position.line)?;

        if position.column > line_len {
            return Err(anyhow::anyhow!(
                "Column {} is out of bounds for line {}",
                position.column,
                position.line
            ));
        }

        Ok(line_start_char + position.column)
    }

    /// Convert a character index to a position
    pub fn char_index_to_position(&self, char_index: usize) -> Result<Position> {
        if char_index > self.len_chars() {
            return Err(anyhow::anyhow!(
                "Character index {} is out of bounds",
                char_index
            ));
        }

        let line = self.rope.char_to_line(char_index);
        let line_start_char = self.rope.line_to_char(line);
        let column = char_index - line_start_char;

        Ok(Position::new(line, column))
    }

    /// Apply a single text edit to the buffer
    #[instrument(skip(self, edit))]
    pub fn apply_edit(&mut self, edit: TextEdit) -> Result<()> {
        self.apply_edits(vec![edit])
    }

    /// Apply multiple text edits atomically
    #[instrument(skip(self, edits))]
    pub fn apply_edits(&mut self, edits: Vec<TextEdit>) -> Result<()> {
        if edits.is_empty() {
            return Ok(());
        }

        let mut sorted_edits = edits.clone();
        sorted_edits.sort_by(|a, b| b.range.start.cmp(&a.range.start));

        for edit in &sorted_edits {
            self.validate_edit(edit)?;
        }

        for edit in &sorted_edits {
            self.apply_single_edit(edit)?;
        }

        self.version += 1;
        self.mark_dirty();

        self.redo_stack.clear();

        self.fire_change_event(edits);

        debug!(
            "Applied {} edits, new version: {}",
            sorted_edits.len(),
            self.version
        );
        Ok(())
    }

    /// Validate that an edit is valid for the current buffer state
    fn validate_edit(&self, edit: &TextEdit) -> Result<()> {
        self.position_to_char_index(edit.range.start)
            .with_context(|| format!("Invalid start position: {}", edit.range.start))?;

        self.position_to_char_index(edit.range.end)
            .with_context(|| format!("Invalid end position: {}", edit.range.end))?;

        if edit.range.start > edit.range.end {
            return Err(anyhow::anyhow!("Invalid range: start > end"));
        }

        Ok(())
    }

    /// Apply a single edit to the rope
    fn apply_single_edit(&mut self, edit: &TextEdit) -> Result<()> {
        let start_char = self.position_to_char_index(edit.range.start)?;
        let end_char = self.position_to_char_index(edit.range.end)?;

        // Remove the old text
        if start_char < end_char {
            self.rope.remove(start_char..end_char);
        }

        // Insert the new text
        if !edit.new_text.is_empty() {
            self.rope.insert(start_char, &edit.new_text);
        }

        Ok(())
    }

    /// Begin an undo group (for grouping multiple edits into one undo operation)
    pub fn begin_undo_group(&mut self, cursor_positions: Vec<Position>) {
        // This will be used when we implement the undo group functionality
        // For now, we store the cursor positions for the next undo entry
    }

    /// End an undo group
    pub fn end_undo_group(&mut self, edits: Vec<TextEdit>, final_cursor_positions: Vec<Position>) {
        let entry = UndoRedoEntry::new(
            edits,
            vec![], // We'll implement proper cursor tracking later
            final_cursor_positions,
        );

        self.undo_stack.push_back(entry);

        // Keep undo stack size manageable
        while self.undo_stack.len() > self.config.max_undo_entries {
            self.undo_stack.pop_front();
        }
    }

    /// Undo the last operation
    pub fn undo(&mut self) -> Result<Option<Vec<Position>>> {
        if let Some(entry) = self.undo_stack.pop_back() {
            let mut inverse_edits = Vec::new();
            for edit in &entry.edits {
                let old_text = self.text_in_range(&edit_range)?;
                let inverse_edit =
                    TextEdit::new(Range::new(edit.range.start, edit.range.start), old_text);
                inverse_edits.push(inverse_edit);
            }

            self.apply_edit(inverse_edits.clone())?;

            let redo_entry = UndoRedoEntry::new(
                inverse_edits,
                entry.cursor_after.clone(),
                entry.cursor_before.clone(),
            );
            self.redo_stack.push_back(redo_entry);

            debug!("Undid operation, returning to version {}", self.version - 1);
            return Ok(Some(entry.cursor_before));
        }

        Ok(None)
    }

    /// Redo the last undone operation
    pub fn redo(&mut self) -> Result<Option<Vec<Position>>> {
        if let Some(entry) = self.redo_stack.pop_back() {
            self.apply_edits(entry.edits.clone())?;

            self.undo_stack.push_back(entry.clone());

            debug!("Redid operation, version {}", self.version);
            return Ok(Some(entry.cursor_after));
        }

        Ok(None)
    }

    /// Check if undo is available
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Save the buffer to its associated file
    #[instrument(skip(self))]
    pub async fn save(&mut self) -> Result<()> {
        let file_path = self
            .file_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No file path associated with buffer"))?;

        self.save_to_file(file_path).await
    }

    /// Save the buffer to a specific file
    #[instrument(skip(self))]
    pub async fn save_to_file(&mut self, path: PathBuf) -> Result<()> {
        let mut content = self.text();

        if self.config.trim_trailing_whitespace {
            content = self.trim_trailing_whitespace(&content);
        }

        if self.config.insert_final_newline && !content.ends_with('\n') && !content.is_empty() {
            content.push_str(self.line_ending.as_str());
        }

        tokio::fs::write(&path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", path.display()))?;

        self.file_path = Some(path.clone());
        self.dirty = false;

        debug!("Saved buffer to: {}", path.display());
        Ok(())
    }

    /// Trim trailing whitespace from content
    fn trim_trailing_whitespace(&self, content: &str) -> String {
        content
            .lines()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join(self.line_ending.as_str())
    }

    /// Mark the buffer as dirty
    fn mark_dirty(&mut self) {
        if !self.dirty {
            self.dirty = true;
            debug!("Buffer marked as dirty");
        }
    }

    /// Add a change event listener
    pub fn add_change_listener<F>(&mut self, listener: F)
    where
        F: Fn(&BufferChangeEvent) + Send + Sync + 'static,
    {
        self.change_listeners.push(Box::new(listener));
    }

    /// Fire change event to all listeners
    fn fire_change_event(&self, edits: Vec<TextEdit>) {
        let event = BufferChangeEvent {
            verson: self.version,
            edits,
            full_text_length: self.len_chars(),
            line_count: self.len_lines(),
        };

        for listener in &self.change_listeners {
            listener(&event);
        }
    }

    /// Get rope slice for advanced operations
    pub fn rope_slice(&self) -> RopeSlice {
        self.rope.slice(..)
    }

    /// Get the underlying rope (for advanced use cases)
    pub fn rope(&self) -> &Rope {
        &self.rope
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for TextBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextBuffer")
            .field("version", &self.version)
            .field("dirty", &self.dirty)
            .field("file_path", &self.file_path)
            .field("line_ending", &self.line_ending)
            .field("char_count", &self.len_chars())
            .field("line_count", &self.len_lines())
            .field("undo_stack_size", &self.undo_stack.len())
            .field("redo_stack_size", &self.redo_stack.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_buffer() {
        let buffer = TextBuffer::new();
        assert_eq!(buffer.len_chars(), 0);
        assert_eq!(buffer.len_lines(), 1); // Rope always has at least 1 line
        assert!(!buffer.is_dirty());
        assert_eq!(buffer.version(), 0);
    }

    #[test]
    fn test_buffer_from_content() {
        let content = "Hello\nWorld\n";
        let buffer = TextBuffer::from_content(content, None).unwrap();

        assert_eq!(buffer.len_chars(), 12);
        assert_eq!(buffer.len_lines(), 3); // "Hello\n", "World\n", ""
        assert_eq!(buffer.text(), content);
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn test_position_to_char_index() {
        let buffer = TextBuffer::from_content("Hello\nWorld", None).unwrap();

        assert_eq!(
            buffer.position_to_char_index(Position::new(0, 0)).unwrap(),
            0
        );
        assert_eq!(
            buffer.position_to_char_index(Position::new(0, 5)).unwrap(),
            5
        );
        assert_eq!(
            buffer.position_to_char_index(Position::new(1, 0)).unwrap(),
            6
        );
        assert_eq!(
            buffer.position_to_char_index(Position::new(1, 5)).unwrap(),
            11
        );
    }

    #[test]
    fn test_char_index_to_position() {
        let buffer = TextBuffer::from_content("Hello\nWorld", None).unwrap();

        assert_eq!(
            buffer.char_index_to_position(0).unwrap(),
            Position::new(0, 0)
        );
        assert_eq!(
            buffer.char_index_to_position(5).unwrap(),
            Position::new(0, 5)
        );
        assert_eq!(
            buffer.char_index_to_position(6).unwrap(),
            Position::new(1, 0)
        );
        assert_eq!(
            buffer.char_index_to_position(11).unwrap(),
            Position::new(1, 5)
        );
    }

    #[test]
    fn test_text_insertion() {
        let mut buffer = TextBuffer::from_content("Hello World", None).unwrap();
        let edit = TextEdit::insert(Position::new(0, 5), ", Rust".to_string());

        buffer.apply_edit(edit).unwrap();

        assert_eq!(buffer.text(), "Hello, Rust World");
        assert!(buffer.is_dirty());
        assert_eq!(buffer.version(), 1);
    }

    #[test]
    fn test_text_deletion() {
        let mut buffer = TextBuffer::from_content("Hello World", None).unwrap();
        let edit = TextEdit::delete(Range::new(Position::new(0, 5), Position::new(0, 6)));

        buffer.apply_edit(edit).unwrap();

        assert_eq!(buffer.text(), "HelloWorld");
        assert!(buffer.is_dirty());
    }

    #[test]
    fn test_text_replacement() {
        let mut buffer = TextBuffer::from_content("Hello World", None).unwrap();
        let edit = TextEdit::replace(
            Range::new(Position::new(0, 6), Position::new(0, 11)),
            "Rust".to_string(),
        );

        buffer.apply_edit(edit).unwrap();

        assert_eq!(buffer.text(), "Hello Rust");
        assert!(buffer.is_dirty());
    }

    #[test]
    fn test_line_ending_detection() {
        assert_eq!(LineEnding::detect("Hello\nWorld"), LineEnding::Unix);
        assert_eq!(LineEnding::detect("Hello\r\nWorld"), LineEnding::Windows);
        assert_eq!(LineEnding::detect("Hello\rWorld"), LineEnding::Mac);
        assert_eq!(LineEnding::detect("Hello World"), LineEnding::Unix); // Default
    }

    #[test]
    fn test_line_operations() {
        let buffer = TextBuffer::from_content("Hello\nWorld\nRust", None).unwrap();

        assert_eq!(buffer.line_text(0).unwrap(), "Hello\n");
        assert_eq!(buffer.line_text(1).unwrap(), "World\n");
        assert_eq!(buffer.line_text(2).unwrap(), "Rust");

        assert_eq!(buffer.line_len(0).unwrap(), 5);
        assert_eq!(buffer.line_len(1).unwrap(), 5);
        assert_eq!(buffer.line_len(2).unwrap(), 4);
    }

    #[test]
    fn test_multiple_edits() {
        let mut buffer = TextBuffer::from_content("Hello World", None).unwrap();
        let edits = vec![
            TextEdit::insert(Position::new(0, 5), ",".to_string()),
            TextEdit::replace(
                Range::new(Position::new(0, 6), Position::new(0, 11)),
                "Rust".to_string(),
            ),
        ];

        buffer.apply_edits(edits).unwrap();

        assert_eq!(buffer.text(), "Hello, Rust");
        assert_eq!(buffer.version(), 1);
    }
}
