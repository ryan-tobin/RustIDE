// src-tauri/src/core/cursor.rs
use crate::core::text_buffer::{Position, Range, TextBuffer, TextEdit};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use tracing::{debug, instrument};

/// Represents the direction of cursor movement
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Represents different cursor movement units
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MovementUnit {
    Character,
    Word,
    Line,
    Page,
    Document,
}

/// Represents the type of selection operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionMode {
    /// Normal selection with start and end
    Normal,
    /// Line-based selection (selects entire lines)
    Line,
    /// Block/column selection (rectangular selection)
    Block,
}

/// A single cursor with position and selection
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// Current cursor position
    pub position: Position,
    /// Selection anchor (where selection started)
    pub anchor: Position,
    /// Preferred column for vertical movement
    pub preferred_column: Option<usize>,
    /// Whether this cursor has an active selection
    pub has_selection: bool,
    /// Unique identifier for this cursor
    pub id: usize,
}

impl Cursor {
    /// Create a new cursor at the given position
    pub fn new(id: usize, position: Position) -> Self {
        Self {
            position,
            anchor: position,
            preferred_column: Some(position.column),
            has_selection: false,
            id,
        }
    }

    /// Create a cursor with a selection
    pub fn with_selection(id: usize, anchor: Position, position: Position) -> Self {
        Self {
            position,
            anchor,
            preferred_column: Some(position.column),
            has_selection: anchor != position,
            id,
        }
    }

    /// Get the selection range (normalized with start <= end)
    pub fn selection_range(&self) -> Range {
        if self.anchor <= self.position {
            Range::new(self.anchor, self.position)
        } else {
            Range::new(self.position, self.anchor)
        }
    }

    /// Get the selected text from the buffer
    pub fn selected_text(&self, buffer: &TextBuffer) -> Result<String> {
        if !self.has_selection {
            return Ok(String::new());
        }
        buffer.text_in_range(&self.selection_range())
    }

    /// Check if this cursor's selection contains the given position
    pub fn selection_contains(&self, position: Position) -> bool {
        self.has_selection && self.selection_range().contains(position)
    }

    /// Move cursor to a new position, clearing selection
    pub fn move_to(&mut self, position: Position) {
        self.position = position;
        self.anchor = position;
        self.has_selection = false;
        self.preferred_column = Some(position.column);
    }

    /// Move cursor to a new position, extending selection
    pub fn move_to_with_selection(&mut self, position: Position) {
        self.position = position;
        self.has_selection = self.anchor != position;
        self.preferred_column = Some(position.column);
    }

    /// Start a new selection from current position
    pub fn start_selection(&mut self) {
        self.anchor = self.position;
        self.has_selection = false;
    }

    /// Clear the selection
    pub fn clear_selection(&mut self) {
        self.anchor = self.position;
        self.has_selection = false;
    }

    /// Select all text in the given range
    pub fn select_range(&mut self, range: Range) {
        self.anchor = range.start;
        self.position = range.end;
        self.has_selection = !range.is_empty();
        self.preferred_column = Some(self.position.column);
    }

    /// Check if this cursor overlaps with another cursor's selection
    pub fn overlaps_with(&self, other: &Cursor) -> bool {
        if !self.has_selection || !other.has_selection {
            return false;
        }

        let self_range = self.selection_range();
        let other_range = other.selection_range();

        // Check for overlap
        self_range.start <= other_range.end && other_range.start <= self_range.end
    }
}

/// Manages multiple cursors and their operations
pub struct CursorManager {
    /// All cursors (first one is primary)
    cursors: Vec<Cursor>,
    /// Next available cursor ID
    next_id: usize,
    /// Selection mode
    selection_mode: SelectionMode,
    /// History of cursor states for undo/redo
    history: VecDeque<Vec<Cursor>>,
    /// Maximum history entries
    max_history: usize,
    /// Page size for page up/down operations
    page_size: usize,
}

impl CursorManager {
    /// Create a new cursor manager with a single cursor at (0, 0)
    pub fn new() -> Self {
        let primary_cursor = Cursor::new(0, Position::zero());
        Self {
            cursors: vec![primary_cursor],
            next_id: 1,
            selection_mode: SelectionMode::Normal,
            history: VecDeque::new(),
            max_history: 100,
            page_size: 25,
        }
    }

    /// Create cursor manager with cursor at specific position
    pub fn with_position(position: Position) -> Self {
        let primary_cursor = Cursor::new(0, position);
        Self {
            cursors: vec![primary_cursor],
            next_id: 1,
            selection_mode: SelectionMode::Normal,
            history: VecDeque::new(),
            max_history: 100,
            page_size: 25,
        }
    }

    /// Get the primary cursor (first cursor)
    pub fn primary_cursor(&self) -> &Cursor {
        &self.cursors[0]
    }

    /// Get mutable reference to primary cursor
    pub fn primary_cursor_mut(&mut self) -> &mut Cursor {
        &mut self.cursors[0]
    }

    /// Get all cursors
    pub fn cursors(&self) -> &[Cursor] {
        &self.cursors
    }

    /// Get number of cursors
    pub fn cursor_count(&self) -> usize {
        self.cursors.len()
    }

    /// Get current selection mode
    pub fn selection_mode(&self) -> SelectionMode {
        self.selection_mode
    }

    /// Set selection mode
    pub fn set_selection_mode(&mut self, mode: SelectionMode) {
        self.selection_mode = mode;
    }

    /// Set page size for page up/down operations
    pub fn set_page_size(&mut self, size: usize) {
        self.page_size = size;
    }

    /// Save current cursor state to history
    fn save_state(&mut self) {
        self.history.push_back(self.cursors.clone());
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }
    }

    /// Restore cursor state from history
    pub fn restore_previous_state(&mut self) -> bool {
        if let Some(previous_state) = self.history.pop_back() {
            self.cursors = previous_state;
            true
        } else {
            false
        }
    }

    /// Add a new cursor at the given position
    pub fn add_cursor(&mut self, position: Position) {
        let cursor = Cursor::new(self.next_id, position);
        self.next_id += 1;
        self.cursors.push(cursor);
        self.merge_overlapping_cursors();
        debug!(
            "Added cursor at {}, total cursors: {}",
            position,
            self.cursors.len()
        );
    }

    /// Add a cursor with selection
    pub fn add_cursor_with_selection(&mut self, anchor: Position, position: Position) {
        let cursor = Cursor::with_selection(self.next_id, anchor, position);
        self.next_id += 1;
        self.cursors.push(cursor);
        self.merge_overlapping_cursors();
        debug!(
            "Added cursor with selection from {} to {}",
            anchor, position
        );
    }

    /// Remove all cursors except the primary one
    pub fn clear_secondary_cursors(&mut self) {
        self.cursors.truncate(1);
        debug!("Cleared secondary cursors, keeping only primary");
    }

    /// Remove cursor by ID
    pub fn remove_cursor(&mut self, id: usize) -> bool {
        if self.cursors.len() <= 1 {
            return false; // Don't remove the last cursor
        }

        let initial_len = self.cursors.len();
        self.cursors.retain(|cursor| cursor.id != id);
        initial_len != self.cursors.len()
    }

    /// Move all cursors in the given direction
    #[instrument(skip(self, buffer))]
    pub fn move_cursors(
        &mut self,
        buffer: &TextBuffer,
        direction: Direction,
        unit: MovementUnit,
        extend_selection: bool,
    ) -> Result<()> {
        self.save_state();

        for cursor in &mut self.cursors {
            let new_position = self.calculate_movement(buffer, cursor, direction, unit)?;

            if extend_selection {
                cursor.move_to_with_selection(new_position);
            } else {
                cursor.move_to(new_position);
            }
        }

        self.merge_overlapping_cursors();
        debug!(
            "Moved {} cursors {:?} by {:?}",
            self.cursors.len(),
            direction,
            unit
        );
        Ok(())
    }

    /// Calculate new position for cursor movement
    fn calculate_movement(
        &self,
        buffer: &TextBuffer,
        cursor: &Cursor,
        direction: Direction,
        unit: MovementUnit,
    ) -> Result<Position> {
        match (direction, unit) {
            (Direction::Left, MovementUnit::Character) => {
                self.move_character_left(buffer, cursor.position)
            }
            (Direction::Right, MovementUnit::Character) => {
                self.move_character_right(buffer, cursor.position)
            }
            (Direction::Up, MovementUnit::Line) => self.move_line_up(buffer, cursor),
            (Direction::Down, MovementUnit::Line) => self.move_line_down(buffer, cursor),
            (Direction::Left, MovementUnit::Word) => self.move_word_left(buffer, cursor.position),
            (Direction::Right, MovementUnit::Word) => self.move_word_right(buffer, cursor.position),
            (Direction::Up, MovementUnit::Page) => self.move_page_up(buffer, cursor),
            (Direction::Down, MovementUnit::Page) => self.move_page_down(buffer, cursor),
            (Direction::Up, MovementUnit::Document) => Ok(Position::zero()),
            (Direction::Down, MovementUnit::Document) => {
                let last_line = buffer.len_lines().saturating_sub(1);
                let last_column = buffer.line_len(last_line).unwrap_or(0);
                Ok(Position::new(last_line, last_column))
            }
            _ => Err(anyhow::anyhow!(
                "Invalid movement combination: {:?} {:?}",
                direction,
                unit
            )),
        }
    }

    /// Move cursor one character to the left
    fn move_character_left(&self, buffer: &TextBuffer, position: Position) -> Result<Position> {
        if position.column > 0 {
            Ok(Position::new(position.line, position.column - 1))
        } else if position.line > 0 {
            let prev_line = position.line - 1;
            let line_len = buffer.line_len(prev_line)?;
            Ok(Position::new(prev_line, line_len))
        } else {
            Ok(position) // Already at start of document
        }
    }

    /// Move cursor one character to the right
    fn move_character_right(&self, buffer: &TextBuffer, position: Position) -> Result<Position> {
        let line_len = buffer.line_len(position.line)?;

        if position.column < line_len {
            Ok(Position::new(position.line, position.column + 1))
        } else if position.line < buffer.len_lines() - 1 {
            Ok(Position::new(position.line + 1, 0))
        } else {
            Ok(position) // Already at end of document
        }
    }

    /// Move cursor up one line, maintaining preferred column
    fn move_line_up(&self, buffer: &TextBuffer, cursor: &Cursor) -> Result<Position> {
        if cursor.position.line == 0 {
            return Ok(Position::new(0, 0));
        }

        let target_line = cursor.position.line - 1;
        let line_len = buffer.line_len(target_line)?;
        let preferred_column = cursor.preferred_column.unwrap_or(cursor.position.column);
        let new_column = preferred_column.min(line_len);

        Ok(Position::new(target_line, new_column))
    }

    /// Move cursor down one line, maintaining preferred column
    fn move_line_down(&self, buffer: &TextBuffer, cursor: &Cursor) -> Result<Position> {
        if cursor.position.line >= buffer.len_lines() - 1 {
            let last_line = buffer.len_lines() - 1;
            let line_len = buffer.line_len(last_line)?;
            return Ok(Position::new(last_line, line_len));
        }

        let target_line = cursor.position.line + 1;
        let line_len = buffer.line_len(target_line)?;
        let preferred_column = cursor.preferred_column.unwrap_or(cursor.position.column);
        let new_column = preferred_column.min(line_len);

        Ok(Position::new(target_line, new_column))
    }

    /// Move cursor to previous word boundary
    fn move_word_left(&self, buffer: &TextBuffer, position: Position) -> Result<Position> {
        let mut current_pos = position;

        // Move left until we find a word boundary
        loop {
            let prev_pos = self.move_character_left(buffer, current_pos)?;
            if prev_pos == current_pos {
                break; // At document start
            }

            let char_index = buffer.position_to_char_index(prev_pos)?;
            let rope_slice = buffer.rope_slice();

            if let Some(ch) = rope_slice.chars().nth(char_index) {
                if ch.is_whitespace() || ch.is_ascii_punctuation() {
                    if current_pos != position {
                        break; // Found word boundary
                    }
                } else if current_pos != position {
                    // We were in whitespace/punctuation and hit a letter
                    break;
                }
            }

            current_pos = prev_pos;
        }

        Ok(current_pos)
    }

    /// Move cursor to next word boundary
    fn move_word_right(&self, buffer: &TextBuffer, position: Position) -> Result<Position> {
        let mut current_pos = position;
        let rope_slice = buffer.rope_slice();

        // Skip current word/whitespace/punctuation
        loop {
            let next_pos = self.move_character_right(buffer, current_pos)?;
            if next_pos == current_pos {
                break; // At document end
            }

            let char_index = buffer.position_to_char_index(current_pos)?;
            if let Some(ch) = rope_slice.chars().nth(char_index) {
                if ch.is_whitespace() || ch.is_ascii_punctuation() {
                    if current_pos != position {
                        break; // Found boundary after word
                    }
                } else {
                    // In a word, continue until we hit boundary
                    current_pos = next_pos;
                    continue;
                }
            }

            current_pos = next_pos;
        }

        Ok(current_pos)
    }

    /// Move cursor up by one page
    fn move_page_up(&self, buffer: &TextBuffer, cursor: &Cursor) -> Result<Position> {
        let target_line = cursor.position.line.saturating_sub(self.page_size);
        let line_len = buffer.line_len(target_line)?;
        let preferred_column = cursor.preferred_column.unwrap_or(cursor.position.column);
        let new_column = preferred_column.min(line_len);

        Ok(Position::new(target_line, new_column))
    }

    /// Move cursor down by one page
    fn move_page_down(&self, buffer: &TextBuffer, cursor: &Cursor) -> Result<Position> {
        let target_line = (cursor.position.line + self.page_size).min(buffer.len_lines() - 1);
        let line_len = buffer.line_len(target_line)?;
        let preferred_column = cursor.preferred_column.unwrap_or(cursor.position.column);
        let new_column = preferred_column.min(line_len);

        Ok(Position::new(target_line, new_column))
    }

    /// Select all text in the buffer
    pub fn select_all(&mut self, buffer: &TextBuffer) -> Result<()> {
        self.clear_secondary_cursors();

        let start = Position::zero();
        let last_line = buffer.len_lines().saturating_sub(1);
        let end_column = buffer.line_len(last_line)?;
        let end = Position::new(last_line, end_column);

        self.primary_cursor_mut()
            .select_range(Range::new(start, end));
        debug!("Selected all text from {} to {}", start, end);
        Ok(())
    }

    /// Select the current line for all cursors
    pub fn select_lines(&mut self, buffer: &TextBuffer) -> Result<()> {
        for cursor in &mut self.cursors {
            let start = Position::new(cursor.position.line, 0);
            let end = if cursor.position.line < buffer.len_lines() - 1 {
                Position::new(cursor.position.line + 1, 0)
            } else {
                let line_len = buffer.line_len(cursor.position.line)?;
                Position::new(cursor.position.line, line_len)
            };

            cursor.select_range(Range::new(start, end));
        }

        self.merge_overlapping_cursors();
        debug!("Selected lines for {} cursors", self.cursors.len());
        Ok(())
    }

    /// Expand selection to word boundaries for all cursors
    pub fn expand_selection_to_words(&mut self, buffer: &TextBuffer) -> Result<()> {
        for cursor in &mut self.cursors {
            let current_range = if cursor.has_selection {
                cursor.selection_range()
            } else {
                Range::single_point(cursor.position)
            };

            let word_start = self.move_word_left(buffer, current_range.start)?;
            let word_end = self.move_word_right(buffer, current_range.end)?;

            cursor.select_range(Range::new(word_start, word_end));
        }

        self.merge_overlapping_cursors();
        debug!(
            "Expanded selection to words for {} cursors",
            self.cursors.len()
        );
        Ok(())
    }

    /// Move cursors to start of their selections
    pub fn move_to_selection_start(&mut self) {
        for cursor in &mut self.cursors {
            if cursor.has_selection {
                let range = cursor.selection_range();
                cursor.move_to(range.start);
            }
        }
        debug!("Moved cursors to selection start");
    }

    /// Move cursors to end of their selections
    pub fn move_to_selection_end(&mut self) {
        for cursor in &mut self.cursors {
            if cursor.has_selection {
                let range = cursor.selection_range();
                cursor.move_to(range.end);
            }
        }
        debug!("Moved cursors to selection end");
    }

    /// Clear all selections but keep cursor positions
    pub fn clear_selections(&mut self) {
        for cursor in &mut self.cursors {
            cursor.clear_selection();
        }
        debug!("Cleared all selections");
    }

    /// Get all selected ranges, sorted by position
    pub fn selected_ranges(&self) -> Vec<Range> {
        let mut ranges: Vec<Range> = self
            .cursors
            .iter()
            .filter(|cursor| cursor.has_selection)
            .map(|cursor| cursor.selection_range())
            .collect();

        ranges.sort_by(|a, b| a.start.cmp(&b.start));
        ranges
    }

    /// Get all cursor positions
    pub fn cursor_positions(&self) -> Vec<Position> {
        self.cursors.iter().map(|cursor| cursor.position).collect()
    }

    /// Check if any cursor has a selection
    pub fn has_selection(&self) -> bool {
        self.cursors.iter().any(|cursor| cursor.has_selection)
    }

    /// Get total number of selected characters across all cursors
    pub fn selected_char_count(&self, buffer: &TextBuffer) -> Result<usize> {
        let mut total = 0;
        for cursor in &self.cursors {
            if cursor.has_selection {
                let range = cursor.selection_range();
                let start_char = buffer.position_to_char_index(range.start)?;
                let end_char = buffer.position_to_char_index(range.end)?;
                total += end_char - start_char;
            }
        }
        Ok(total)
    }

    /// Merge overlapping cursors and selections
    fn merge_overlapping_cursors(&mut self) {
        if self.cursors.len() <= 1 {
            return;
        }

        // Sort cursors by position
        self.cursors.sort_by(|a, b| {
            let a_pos = if a.has_selection {
                a.selection_range().start
            } else {
                a.position
            };
            let b_pos = if b.has_selection {
                b.selection_range().start
            } else {
                b.position
            };
            a_pos.cmp(&b_pos)
        });

        let mut merged = Vec::new();
        let mut current = self.cursors[0].clone();

        for next in self.cursors.iter().skip(1) {
            if self.should_merge_cursors(&current, next) {
                current = self.merge_two_cursors(&current, next);
            } else {
                merged.push(current);
                current = next.clone();
            }
        }
        merged.push(current);

        if merged.len() != self.cursors.len() {
            debug!(
                "Merged {} cursors into {}",
                self.cursors.len(),
                merged.len()
            );
        }

        self.cursors = merged;
    }

    /// Check if two cursors should be merged
    fn should_merge_cursors(&self, cursor1: &Cursor, cursor2: &Cursor) -> bool {
        // If either has no selection, check if positions are adjacent or equal
        if !cursor1.has_selection && !cursor2.has_selection {
            return cursor1.position == cursor2.position;
        }

        // If both have selections, check for overlap
        if cursor1.has_selection && cursor2.has_selection {
            return cursor1.overlaps_with(cursor2);
        }

        // If one has selection and other doesn't, check if cursor is within selection
        let (with_selection, without_selection) = if cursor1.has_selection {
            (cursor1, cursor2)
        } else {
            (cursor2, cursor1)
        };

        with_selection.selection_contains(without_selection.position)
    }

    /// Merge two cursors into one
    fn merge_two_cursors(&self, cursor1: &Cursor, cursor2: &Cursor) -> Cursor {
        let id = cursor1.id.min(cursor2.id); // Keep the lower ID

        // If neither has selection, just use the first position
        if !cursor1.has_selection && !cursor2.has_selection {
            return Cursor::new(id, cursor1.position);
        }

        // Calculate merged selection range
        let range1 = if cursor1.has_selection {
            cursor1.selection_range()
        } else {
            Range::single_point(cursor1.position)
        };

        let range2 = if cursor2.has_selection {
            cursor2.selection_range()
        } else {
            Range::single_point(cursor2.position)
        };

        let merged_start = if range1.start <= range2.start {
            range1.start
        } else {
            range2.start
        };

        let merged_end = if range1.end >= range2.end {
            range1.end
        } else {
            range2.end
        };

        Cursor::with_selection(id, merged_start, merged_end)
    }

    /// Update cursor positions after text edits
    pub fn update_after_edits(&mut self, edits: &[TextEdit]) -> Result<()> {
        for cursor in &mut self.cursors {
            cursor.position = self.adjust_position_after_edits(cursor.position, edits)?;
            cursor.anchor = self.adjust_position_after_edits(cursor.anchor, edits)?;

            // Update selection state
            cursor.has_selection = cursor.position != cursor.anchor;

            // Update preferred column
            cursor.preferred_column = Some(cursor.position.column);
        }

        self.merge_overlapping_cursors();
        debug!("Updated cursor positions after {} edits", edits.len());
        Ok(())
    }

    /// Adjust a single position based on text edits
    fn adjust_position_after_edits(
        &self,
        mut position: Position,
        edits: &[TextEdit],
    ) -> Result<Position> {
        // Apply edits in reverse order (they should already be sorted)
        for edit in edits.iter().rev() {
            position = self.adjust_position_after_edit(position, edit)?;
        }
        Ok(position)
    }

    /// Adjust a position after a single edit
    fn adjust_position_after_edit(&self, position: Position, edit: &TextEdit) -> Result<Position> {
        let edit_start = edit.range.start;
        let edit_end = edit.range.end;

        // If position is before the edit, no change needed
        if position < edit_start {
            return Ok(position);
        }

        // If position is within the edited range, move to start of edit
        if position >= edit_start && position <= edit_end {
            return Ok(edit_start);
        }

        // Position is after the edit, need to adjust
        let deleted_text_lines = edit_end.line - edit_start.line;
        let inserted_text_lines = edit.new_text.lines().count().saturating_sub(1);

        if deleted_text_lines == 0 && inserted_text_lines == 0 {
            // Single-line edit
            if position.line == edit_start.line {
                let chars_deleted = edit_end.column - edit_start.column;
                let chars_inserted = edit.new_text.len();

                if chars_inserted >= chars_deleted {
                    let chars_added = chars_inserted - chars_deleted;
                    return Ok(Position::new(position.line, position.column + chars_added));
                } else {
                    let chars_removed = chars_deleted - chars_inserted;
                    return Ok(Position::new(
                        position.line,
                        position.column.saturating_sub(chars_removed),
                    ));
                }
            }
        }

        // Multi-line edit adjustment (simplified)
        let line_delta = inserted_text_lines as i32 - deleted_text_lines as i32;
        let new_line = (position.line as i32 + line_delta).max(0) as usize;

        Ok(Position::new(new_line, position.column))
    }
}

impl Default for CursorManager {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CursorManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CursorManager")
            .field("cursor_count", &self.cursors.len())
            .field("selection_mode", &self.selection_mode)
            .field("has_selection", &self.has_selection())
            .field("primary_position", &self.primary_cursor().position)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::text_buffer::TextBuffer;

    #[test]
    fn test_cursor_creation() {
        let cursor = Cursor::new(0, Position::new(1, 5));
        assert_eq!(cursor.position, Position::new(1, 5));
        assert_eq!(cursor.anchor, Position::new(1, 5));
        assert!(!cursor.has_selection);
        assert_eq!(cursor.preferred_column, Some(5));
    }

    #[test]
    fn test_cursor_with_selection() {
        let cursor = Cursor::with_selection(0, Position::new(1, 0), Position::new(1, 10));
        assert!(cursor.has_selection);
        assert_eq!(
            cursor.selection_range(),
            Range::new(Position::new(1, 0), Position::new(1, 10))
        );
    }

    #[test]
    fn test_cursor_manager_creation() {
        let manager = CursorManager::new();
        assert_eq!(manager.cursor_count(), 1);
        assert_eq!(manager.primary_cursor().position, Position::zero());
        assert!(!manager.has_selection());
    }

    #[test]
    fn test_add_cursor() {
        let mut manager = CursorManager::new();
        manager.add_cursor(Position::new(5, 10));

        assert_eq!(manager.cursor_count(), 2);
        assert_eq!(manager.cursors()[1].position, Position::new(5, 10));
    }

    #[test]
    fn test_cursor_movement() {
        let buffer = TextBuffer::from_content("Hello\nWorld\nRust", None).unwrap();
        let mut manager = CursorManager::with_position(Position::new(1, 2));

        // Move right
        manager
            .move_cursors(&buffer, Direction::Right, MovementUnit::Character, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(1, 3));

        // Move up
        manager
            .move_cursors(&buffer, Direction::Up, MovementUnit::Line, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(0, 3));

        // Move down
        manager
            .move_cursors(&buffer, Direction::Down, MovementUnit::Line, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(1, 3));
    }

    #[test]
    fn test_cursor_selection() {
        let buffer = TextBuffer::from_content("Hello World", None).unwrap();
        let mut manager = CursorManager::with_position(Position::new(0, 0));

        // Select with movement
        manager
            .move_cursors(&buffer, Direction::Right, MovementUnit::Word, true)
            .unwrap();

        assert!(manager.has_selection());
        assert_eq!(
            manager.primary_cursor().selection_range(),
            Range::new(Position::new(0, 0), Position::new(0, 5))
        );
    }

    #[test]
    fn test_select_all() {
        let buffer = TextBuffer::from_content("Hello\nWorld", None).unwrap();
        let mut manager = CursorManager::new();

        manager.select_all(&buffer).unwrap();

        assert!(manager.has_selection());
        assert_eq!(manager.cursor_count(), 1);
        assert_eq!(
            manager.primary_cursor().selection_range(),
            Range::new(Position::zero(), Position::new(1, 5))
        );
    }

    #[test]
    fn test_word_movement() {
        let buffer = TextBuffer::from_content("hello world rust", None).unwrap();
        let mut manager = CursorManager::with_position(Position::new(0, 0));

        // Move to next word
        manager
            .move_cursors(&buffer, Direction::Right, MovementUnit::Word, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(0, 5));

        // Move to previous word
        manager
            .move_cursors(&buffer, Direction::Left, MovementUnit::Word, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(0, 0));
    }

    #[test]
    fn test_line_selection() {
        let buffer = TextBuffer::from_content("Hello\nWorld\nRust", None).unwrap();
        let mut manager = CursorManager::with_position(Position::new(1, 2));

        manager.select_lines(&buffer).unwrap();

        assert!(manager.has_selection());
        assert_eq!(
            manager.primary_cursor().selection_range(),
            Range::new(Position::new(1, 0), Position::new(2, 0))
        );
    }

    #[test]
    fn test_cursor_merging() {
        let mut manager = CursorManager::new();

        // Add overlapping cursors
        manager.add_cursor_with_selection(Position::new(0, 0), Position::new(0, 5));
        manager.add_cursor_with_selection(Position::new(0, 3), Position::new(0, 8));

        // Should be merged into one cursor
        assert_eq!(manager.cursor_count(), 1);
        assert_eq!(
            manager.primary_cursor().selection_range(),
            Range::new(Position::new(0, 0), Position::new(0, 8))
        );
    }

    #[test]
    fn test_cursor_position_update_after_edit() {
        let mut manager = CursorManager::with_position(Position::new(0, 10));

        // Insert text before cursor
        let edit = TextEdit::insert(Position::new(0, 5), "INSERTED".to_string());
        manager.update_after_edits(&[edit]).unwrap();

        // Cursor should move to account for inserted text
        assert_eq!(manager.primary_cursor().position, Position::new(0, 18));
    }

    #[test]
    fn test_multiple_cursors_no_overlap() {
        let mut manager = CursorManager::new();

        manager.add_cursor(Position::new(1, 0));
        manager.add_cursor(Position::new(2, 0));
        manager.add_cursor(Position::new(3, 0));

        assert_eq!(manager.cursor_count(), 4); // Including primary

        // Clear secondary cursors
        manager.clear_secondary_cursors();
        assert_eq!(manager.cursor_count(), 1);
    }

    #[test]
    fn test_page_movement() {
        let content = (0..100)
            .map(|i| format!("Line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let buffer = TextBuffer::from_content(&content, None).unwrap();
        let mut manager = CursorManager::with_position(Position::new(50, 0));
        manager.set_page_size(10);

        // Move up one page
        manager
            .move_cursors(&buffer, Direction::Up, MovementUnit::Page, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(40, 0));

        // Move down one page
        manager
            .move_cursors(&buffer, Direction::Down, MovementUnit::Page, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(50, 0));
    }

    #[test]
    fn test_document_boundaries() {
        let buffer = TextBuffer::from_content("Hello\nWorld", None).unwrap();
        let mut manager = CursorManager::with_position(Position::new(0, 0));

        // Move to document start (should stay at start)
        manager
            .move_cursors(&buffer, Direction::Up, MovementUnit::Document, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(0, 0));

        // Move to document end
        manager
            .move_cursors(&buffer, Direction::Down, MovementUnit::Document, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(1, 5));
    }

    #[test]
    fn test_preferred_column_maintenance() {
        let buffer =
            TextBuffer::from_content("Long line here\nShort\nAnother long line", None).unwrap();
        let mut manager = CursorManager::with_position(Position::new(0, 10));

        // Move down to shorter line
        manager
            .move_cursors(&buffer, Direction::Down, MovementUnit::Line, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(1, 5)); // End of "Short"
        assert_eq!(manager.primary_cursor().preferred_column, Some(10));

        // Move down to longer line - should return to preferred column
        manager
            .move_cursors(&buffer, Direction::Down, MovementUnit::Line, false)
            .unwrap();
        assert_eq!(manager.primary_cursor().position, Position::new(2, 10));
    }
}
