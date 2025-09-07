use crate::core::{Position, Range};
use std::path::Path;

/// Create indentation string
pub fn create_indentation(level: usize, use_tabs: bool, tab_size: usize) -> String {
    if use_tabs {
        "\t".repeat(level)
    } else {
        " ".repeat(level * tab_size)
    }
}

/// Get the indentation level of a line
pub fn get_line_indentation(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// Check if a character is a word character
pub fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Check if a character is a whitespace character
pub fn is_whitespace_char(ch: char) -> bool {
    ch.is_whitespace()
}

/// Find word boundaries around a position
pub fn find_word_boundaries(text: &str, position: Position) -> Option<Range> {
    let lines: Vec<&str> = text.lines().collect();
    if position.line >= lines.len() {
        return None;
    }

    let line = lines[position.line];
    if position.column > line.len() {
        return None;
    }

    let chars: Vec<char> = line.chars().collect();
    if position.column >= chars.len() {
        return None;
    }

    // Find start of word
    let mut start = position.column;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }

    // Find end of word
    let mut end = position.column;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }

    Some(Range::new(
        Position::new(position.line, start),
        Position::new(position.line, end),
    ))
}

/// Convert line ending style
pub fn convert_line_endings(text: &str, target: crate::core::LineEnding) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    
    match target {
        crate::core::LineEnding::Unix => normalized,
        crate::core::LineEnding::Windows => normalized.replace('\n', "\r\n"),
        crate::core::LineEnding::Mac => normalized.replace('\n', "\r"),
    }
}

/// Count lines in text
pub fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        1
    } else {
        text.lines().count().max(1)
    }
}

/// Count words in text
pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Extract file extension from path
pub fn get_file_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_lowercase())
}

/// Check if a file is likely to be a text file based on extension
pub fn is_text_file(path: &Path) -> bool {
    const TEXT_EXTENSIONS: &[&str] = &[
        "rs", "toml", "md", "txt", "json", "yaml", "yml", "xml", "html", "css", "js", "ts",
        "py", "java", "cpp", "c", "h", "hpp", "cs", "go", "php", "rb", "swift", "kt",
    ];

    if let Some(ext) = get_file_extension(path) {
        TEXT_EXTENSIONS.contains(&ext.as_str())
    } else {
        false
    }
}

/// Check if a file is likely to be binary
pub fn is_binary_file(path: &Path) -> bool {
    const BINARY_EXTENSIONS: &[&str] = &[
        "exe", "dll", "so", "dylib", "a", "lib", "obj", "o", "bin", "dat",
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
        "jpg", "jpeg", "png", "gif", "bmp", "ico", "svg",
        "mp3", "wav", "ogg", "flac", "m4a",
        "mp4", "avi", "mkv", "mov", "wmv",
        "zip", "tar", "gz", "rar", "7z",
    ];

    if let Some(ext) = get_file_extension(path) {
        BINARY_EXTENSIONS.contains(&ext.as_str())
    } else {
        false
    }
}

/// Clamp a value between min and max
pub fn clamp<T: PartialOrd>(value: T, min: T, max: T) -> T {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Calculate edit distance between two strings (Levenshtein distance)
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    
    let mut matrix = vec![vec![0; b_chars.len() + 1]; a_chars.len() + 1];
    
    for i in 0..=a_chars.len() {
        matrix[i][0] = i;
    }
    
    for j in 0..=b_chars.len() {
        matrix[0][j] = j;
    }
    
    for i in 1..=a_chars.len() {
        for j in 1..=b_chars.len() {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }
    
    matrix[a_chars.len()][b_chars.len()]
}

/// Find common prefix of two strings
pub fn common_prefix(a: &str, b: &str) -> String {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    
    let mut i = 0;
    while i < a_chars.len() && i < b_chars.len() && a_chars[i] == b_chars[i] {
        i += 1;
    }
    
    a_chars[..i].iter().collect()
}

/// Find common suffix of two strings
pub fn common_suffix(a: &str, b: &str) -> String {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    
    let mut i = 0;
    while i < a_chars.len() && i < b_chars.len() {
        let a_idx = a_chars.len() - 1 - i;
        let b_idx = b_chars.len() - 1 - i;
        if a_chars[a_idx] != b_chars[b_idx] {
            break;
        }
        i += 1;
    }
    
    if i == 0 {
        String::new()
    } else {
        a_chars[a_chars.len() - i..].iter().collect()
    }
}

/// Normalize whitespace in a string
pub fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Escape special characters for regex
pub fn escape_regex(text: &str) -> String {
    const SPECIAL_CHARS: &[char] = &[
        '\\', '^', '$', '.', '|', '?', '*', '+', '(', ')', '[', ']', '{', '}',
    ];

    let mut result = String::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if SPECIAL_CHARS.contains(&ch) {
            result.push('\\');
        }
        result.push(ch);
    }
    result
}

/// Check if a string looks like a URL
pub fn is_url_like(text: &str) -> bool {
    text.starts_with("http://") || 
    text.starts_with("https://") || 
    text.starts_with("ftp://") ||
    text.starts_with("file://")
}

/// Check if a string looks like an email address
pub fn is_email_like(text: &str) -> bool {
    text.contains('@') && text.contains('.') && !text.contains(' ')
}

/// Truncate text to a maximum length with ellipsis
pub fn truncate_with_ellipsis(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        let mut result = text.chars().take(max_len.saturating_sub(3)).collect::<String>();
        result.push_str("...");
        result
    }
}

/// Format file size in human-readable format
pub fn format_file_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", size as u64, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

/// Get the visual width of a string (accounting for tabs)
pub fn visual_width(text: &str, tab_size: usize) -> usize {
    let mut width = 0;
    for ch in text.chars() {
        if ch == '\t' {
            width = (width / tab_size + 1) * tab_size;
        } else {
            width += 1;
        }
    }
    width
}

/// Convert column position accounting for tabs
pub fn column_to_visual_column(text: &str, column: usize, tab_size: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut visual_col = 0;
    
    for i in 0..column.min(chars.len()) {
        if chars[i] == '\t' {
            visual_col = (visual_col / tab_size + 1) * tab_size;
        } else {
            visual_col += 1;
        }
    }
    
    visual_col
}

/// Convert visual column position back to actual column
pub fn visual_column_to_column(text: &str, visual_column: usize, tab_size: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut visual_col = 0;
    let mut column = 0;
    
    while column < chars.len() && visual_col < visual_column {
        if chars[column] == '\t' {
            visual_col = (visual_col / tab_size + 1) * tab_size;
        } else {
            visual_col += 1;
        }
        column += 1;
    }
    
    column
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{LineEnding, Position, Range};

    #[test]
    fn test_create_indentation() {
        assert_eq!(create_indentation(2, true, 4), "\t\t");
        assert_eq!(create_indentation(2, false, 4), "        ");
        assert_eq!(create_indentation(1, false, 2), "  ");
    }

    #[test]
    fn test_get_line_indentation() {
        assert_eq!(get_line_indentation("    hello"), 4);
        assert_eq!(get_line_indentation("\t\thello"), 2);
        assert_eq!(get_line_indentation("hello"), 0);
        assert_eq!(get_line_indentation(""), 0);
    }

    #[test]
    fn test_is_word_char() {
        assert!(is_word_char('a'));
        assert!(is_word_char('Z'));
        assert!(is_word_char('5'));
        assert!(is_word_char('_'));
        assert!(!is_word_char(' '));
        assert!(!is_word_char('-'));
        assert!(!is_word_char('.'));
    }

    #[test]
    fn test_find_word_boundaries() {
        let text = "hello world";
        let pos = Position::new(0, 2); // 'l' in "hello"
        let range = find_word_boundaries(text, pos).unwrap();
        assert_eq!(range.start, Position::new(0, 0));
        assert_eq!(range.end, Position::new(0, 5));

        let pos = Position::new(0, 6); // 'w' in "world"
        let range = find_word_boundaries(text, pos).unwrap();
        assert_eq!(range.start, Position::new(0, 6));
        assert_eq!(range.end, Position::new(0, 11));
    }

    #[test]
    fn test_convert_line_endings() {
        let text = "line1\r\nline2\nline3\r";
        
        let unix = convert_line_endings(text, LineEnding::Unix);
        assert_eq!(unix, "line1\nline2\nline3\n");
        
        let windows = convert_line_endings(text, LineEnding::Windows);
        assert_eq!(windows, "line1\r\nline2\r\nline3\r\n");
        
        let mac = convert_line_endings(text, LineEnding::Mac);
        assert_eq!(mac, "line1\rline2\rline3\r");
    }

    #[test]
    fn test_count_lines() {
        assert_eq!(count_lines(""), 1);
        assert_eq!(count_lines("single line"), 1);
        assert_eq!(count_lines("line1\nline2"), 2);
        assert_eq!(count_lines("line1\nline2\nline3\n"), 3);
    }

    #[test]
    fn test_count_words() {
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("hello"), 1);
        assert_eq!(count_words("hello world"), 2);
        assert_eq!(count_words("  hello   world  "), 2);
    }

    #[test]
    fn test_file_extension() {
        assert_eq!(get_file_extension(Path::new("file.rs")), Some("rs".to_string()));
        assert_eq!(get_file_extension(Path::new("file.TXT")), Some("txt".to_string()));
        assert_eq!(get_file_extension(Path::new("file")), None);
        assert_eq!(get_file_extension(Path::new(".gitignore")), None);
    }

    #[test]
    fn test_is_text_file() {
        assert!(is_text_file(Path::new("main.rs")));
        assert!(is_text_file(Path::new("README.md")));
        assert!(is_text_file(Path::new("config.json")));
        assert!(!is_text_file(Path::new("binary.exe")));
        assert!(!is_text_file(Path::new("image.png")));
    }

    #[test]
    fn test_is_binary_file() {
        assert!(is_binary_file(Path::new("program.exe")));
        assert!(is_binary_file(Path::new("image.jpg")));
        assert!(is_binary_file(Path::new("archive.zip")));
        assert!(!is_binary_file(Path::new("source.rs")));
        assert!(!is_binary_file(Path::new("text.txt")));
    }

    #[test]
    fn test_clamp() {
        assert_eq!(clamp(5, 0, 10), 5);
        assert_eq!(clamp(-1, 0, 10), 0);
        assert_eq!(clamp(15, 0, 10), 10);
    }

    #[test]
    fn test_edit_distance() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("hello", "hello"), 0);
        assert_eq!(edit_distance("hello", "helo"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_common_prefix() {
        assert_eq!(common_prefix("hello", "help"), "hel");
        assert_eq!(common_prefix("abc", "xyz"), "");
        assert_eq!(common_prefix("test", "testing"), "test");
    }

    #[test]
    fn test_common_suffix() {
        assert_eq!(common_suffix("hello", "jello"), "ello");
        assert_eq!(common_suffix("abc", "xyz"), "");
        assert_eq!(common_suffix("testing", "ing"), "ing");
    }

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_whitespace("  hello    world  "), "hello world");
        assert_eq!(normalize_whitespace("single"), "single");
        assert_eq!(normalize_whitespace(""), "");
    }

    #[test]
    fn test_escape_regex() {
        assert_eq!(escape_regex("hello.world"), "hello\\.world");
        assert_eq!(escape_regex("test*"), "test\\*");
        assert_eq!(escape_regex("(abc)"), "\\(abc\\)");
        assert_eq!(escape_regex("normal"), "normal");
    }

    #[test]
    fn test_is_url_like() {
        assert!(is_url_like("https://example.com"));
        assert!(is_url_like("http://localhost:3000"));
        assert!(is_url_like("ftp://files.example.com"));
        assert!(!is_url_like("not a url"));
        assert!(!is_url_like("mailto:test@example.com"));
    }

    #[test]
    fn test_is_email_like() {
        assert!(is_email_like("user@example.com"));
        assert!(is_email_like("test.email@domain.co.uk"));
        assert!(!is_email_like("not an email"));
        assert!(!is_email_like("missing.domain"));
        assert!(!is_email_like("has spaces@example.com"));
    }

    #[test]
    fn test_truncate_with_ellipsis() {
        assert_eq!(truncate_with_ellipsis("hello", 10), "hello");
        assert_eq!(truncate_with_ellipsis("hello world", 8), "hello...");
        assert_eq!(truncate_with_ellipsis("short", 3), "");
        assert_eq!(truncate_with_ellipsis("exactly", 7), "exactly");
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
        assert_eq!(format_file_size(1048576), "1.0 MB");
    }

    #[test]
    fn test_visual_width() {
        assert_eq!(visual_width("hello", 4), 5);
        assert_eq!(visual_width("he\tllo", 4), 8); // tab expands to column 4
        assert_eq!(visual_width("\t", 4), 4);
        assert_eq!(visual_width("a\tb", 4), 5); // 'a' at 0, tab to 4, 'b' at 4
    }

    #[test]
    fn test_column_conversions() {
        let text = "a\tb\tc";
        assert_eq!(column_to_visual_column(text, 0, 4), 0); // 'a'
        assert_eq!(column_to_visual_column(text, 1, 4), 1); // tab position
        assert_eq!(column_to_visual_column(text, 2, 4), 4); // 'b' after tab
        
        assert_eq!(visual_column_to_column(text, 0, 4), 0); // 'a'
        assert_eq!(visual_column_to_column(text, 4, 4), 2); // 'b'
        assert_eq!(visual_column_to_column(text, 8, 4), 4); // 'c'
    }
}