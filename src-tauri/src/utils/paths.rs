use crate::utils::{UtilError, UtilResult};
use serde::de::Expected;
use std::path::{Component, Path, PathBuf};
use tracing::{debug, instrument};

/// Extension trait for Path with additional utilities
pub trait PathExt {
    /// Check if path is a descendant of another path
    fn is_descendant_of<P: AsRef<Path>>(&self, ancestor: P) -> bool;

    /// Get the relative path from a base path
    fn relative_to<P: AsRef<Path>>(&self, base: P) -> Option<PathBuf>;

    /// Normalize a path by resolving . and .. components
    fn normalize(&self) -> PathBuf;

    /// Check if path has any of the given extensions
    fn has_extension(&self, extensions: &[&str]) -> bool;

    /// Get file stem as string
    fn file_stem_str(&self) -> Option<&str>;

    /// Check if path is hidden (starts with .)
    fn is_hidden(&self) -> bool;
}

impl<P: AsRef<Path>> PathExt for P {
    fn is_descendant_of<Q: AsRef<Path>>(&self, ancestor: Q) -> bool {
        let path = self.as_ref();
        let ancestor = ancestor.as_ref();

        path.strip_prefix(ancestor).is_ok()
    }

    fn relative_to<P: AsRef<Path>>(&self, base: Q) -> Option<PathBuf> {
        let path = self.as_ref();
        let base = base.as_ref();

        path.strip_prefix(base).ok().map(|p| p.to_path_buf())
    }

    fn normalize(&self) -> PathBuf {
        normalize_path(self.as_ref())
    }

    fn has_extension(&self, extensions: &[&str]) -> bool {
        if let Some(ext) = self.as_ref().extension().and_then(|e| e.to_str()) {
            extensions
                .iter()
                .any(|&expected| ext.eq_ignore_ascii_case(expected))
        } else {
            false
        }
    }

    fn file_stem_str(&self) -> Option<&str> {
        self.as_ref().file_stem().and_then(|s| s.to_str())
    }

    fn is_hidden(&self) -> bool {
        self.as_ref()
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("."))
            .unwrap_or(false)
    }
}

/// Normalize a path by resolving . and .. components
pub fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    let mut components = Vec::new();

    for component in path.components() {
        match component {
            Component::CurDir => {
                // Skip current directory components
            }
            Component::ParentDir => {
                // Handle parent directory
                if let Some(last) = components.last() {
                    if *last != Component::ParentDir {
                        components.pop();
                    } else {
                        components.push(component);
                    }
                } else {
                    components.push(component);
                }
            }
            _ => {
                components.push(component);
            }
        }
    }

    components.iter().collect()
}

/// Get relative path from base to target
pub fn get_relative_path<P: AsRef<Path>, Q: AsRef<Path>>(
    target: P,
    base: Q,
) -> UtilResult<PathBuf> {
    let target = target.as_ref().canonicalize()?;
    let base = base.as_ref().canonicalize()?;

    target
        .strip_prefix(&base)
        .map(|p| p.to_path_buf())
        .map_err(|_| UtilError::Path {
            message: format!(
                "Path {} is not relative to {}",
                target.display(),
                base.display()
            ),
        })
}

/// Ensure a directory exists, creating it if necessary
#[instrument]
pub async fn ensure_directory<P: AsRef<Path>>(path: P) -> UtilResult<()> {
    let path = path.as_ref();

    if !path.exists() {
        tokio::fs::create_dir_all(path).await?;
        debug!("Created directory: {}", path.display());
    } else if !path.is_dir() {
        return Err(UtilError::Path {
            message: format!("Path exists but is not a directory: {}", path.display()),
        });
    }

    Ok(())
}

/// Find all files with specific extensions in a directory
#[instrument]
pub async fn find_files_with_extensions<P: AsRef<Path>>(
    dir: P,
    extensions: &[&str],
    recursive: bool,
) -> UtilResult<Vec<PathBuf>> {
    let dir = dir.as_ref();
    let mut files = Vec::new();

    if !dir.is_dir() {
        return Err(UtilError::Path {
            message: format!("Not a directory: {}", dir.display()),
        });
    }

    let mut entries = tokio::fs::read_dir(dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if path.is_file() && path.has_extension(extensions) {
            files.push(path);
        } else if recursive && path.is_dir() && !path.is_hidden() {
            let mut sub_files = find_files_with_extensions(&path, extensions, recursive).await?;
            files.append(&mut sub_files);
        }
    }

    files.sort();
    Ok((files))
}

/// Get the common ancestor path of multiple paths
pub fn get_common_ancestor<I, P>(paths: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut paths = paths.into_iter();
    let first = paths.next()?.as_ref().to_path_buf();

    let mut common = first;

    for path in paths {
        let path = path.as_ref();

        let mut common_components = Vec::new();
        let common_iter = common.components();
        let path_iter = path.components();

        for (a, b) in common_iter.zip(path_iter) {
            if a == b {
                common_components.push(a);
            } else {
                break;
            }
        }

        common = common_components.iter().collect();

        if common_components.is_empty() {
            return None;
        }
    }

    Some(common)
}

/// Check if a path is safe to access (not outside allowed boundaries)
pub fn is_safe_path<P: AsRef<Path>, Q: AsRef<Path>>(path: P, allowed_root: Q) -> bool {
    let path = match path.as_ref().canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    let allowed_root = match allowed_root.as_ref().canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    path.starts_with(&allowed_root)
}

/// Get file size in bytes
pub async fn get_file_size<P: AsRef<Path>>(path: P) -> UtilResult<u64> {
    let metadata = tokio::fs::metadata(path).await?;
    Ok(metadata.len())
}

/// Check if file is larger than specified size
pub async fn is_file_too_large<P: AsRef<Path>>(path: P, max_size_bytes: u64) -> UtilResult<bool> {
    let size = get_file_size(path).await?;
    Ok(size > max_size_bytes)
}

/// Copy file with progress reporting
pub async fn copy_file_with_progress<P: AsRef<Path>, Q: AsRef<Path>, F>(
    from: P,
    to: Q,
    progress_callback: F,
) -> UtilResult<()>
where
    F: Fn(u64, u64),
{
    let from = from.as_ref();
    let to = to.as_ref();

    if let Some(parent) = to.parent() {
        ensure_directory(parent).await?;
    }

    let total_size = get_file_size(from).await?;
    let mut source = tokio::fs::File::open(from).await?;
    let mut dest = tokio::fs::File::create(to).await?;

    let mut buffer = vec![0u8; 8192];
    let mut bytes_copied = 0u64;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    progress_callback(0, total_size);

    loop {
        let bytes_read = source.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }

        dest.write_all(&buffer[..bytes_read]).await?;
        bytes_copied += bytes_read as u64;

        progress_callback(bytes_copied, total_size);

        tokio::task::yield_now().await;
    }

    dest.flush().await?;
    dest.sync_all().await?;

    debug!(
        "Copied file: {} -> {} ({} bytes)",
        from.display(),
        to.display(),
        bytes_copied
    );

    Ok(())
}

/// Copy file with progress reporting and cancellation support
pub async fn copy_file_with_progress_cancellable<P: AsRef<Path>, Q: AsRef<Path>, F>(
    from: P,
    to: Q,
    progress_callback: F,
    cancellation_token: &CancellationToken,
) -> UtilResult<()>
where
    F: Fn(u64, u64), // (bytes_copied, total_bytes)
{
    let from = from.as_ref();
    let to = to.as_ref();

    // Check for cancellation before starting
    if cancellation_token.is_cancelled() {
        return Err(UtilError::Cancelled);
    }

    // Ensure destination directory exists
    if let Some(parent) = to.parent() {
        ensure_directory(parent).await?;
    }

    let total_size = get_file_size(from).await?;
    let mut source = tokio::fs::File::open(from).await?;
    let mut dest = tokio::fs::File::create(to).await?;

    let mut buffer = vec![0u8; 8192]; // 8KB buffer
    let mut bytes_copied = 0u64;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Initial progress callback
    progress_callback(0, total_size);

    loop {
        // Check for cancellation
        if cancellation_token.is_cancelled() {
            // Clean up: close files and delete partial destination
            drop(source);
            drop(dest);
            if let Err(e) = tokio::fs::remove_file(to).await {
                warn!("Failed to clean up partial file {}: {}", to.display(), e);
            }
            return Err(UtilError::Cancelled);
        }

        let bytes_read = source.read(&mut buffer).await?;
        if bytes_read == 0 {
            break; // End of file
        }

        dest.write_all(&buffer[..bytes_read]).await?;
        bytes_copied += bytes_read as u64;

        // Report progress
        progress_callback(bytes_copied, total_size);

        // Yield to allow other tasks to run and check cancellation
        tokio::task::yield_now().await;
    }

    // Ensure all data is written to disk
    dest.flush().await?;
    dest.sync_all().await?;

    debug!(
        "Copied file: {} -> {} ({} bytes)",
        from.display(),
        to.display(),
        bytes_copied
    );

    Ok(())
}

/// Copy multiple files with overall progress reporting
pub async fn copy_files_with_progress<F>(
    file_pairs: Vec<(PathBuf, PathBuf)>, // (source, destination) pairs
    progress_callback: F,
) -> UtilResult<Vec<Result<(), UtilError>>>
where
    F: Fn(usize, usize, u64, u64), // (files_completed, total_files, bytes_completed, total_bytes)
{
    if file_pairs.is_empty() {
        return Ok(Vec::new());
    }

    // Calculate total size of all files
    let mut total_bytes = 0u64;
    let mut file_sizes = Vec::with_capacity(file_pairs.len());

    for (source, _) in &file_pairs {
        match get_file_size(source).await {
            Ok(size) => {
                file_sizes.push(size);
                total_bytes += size;
            }
            Err(e) => {
                warn!("Failed to get size of {}: {}", source.display(), e);
                file_sizes.push(0);
            }
        }
    }

    let mut results = Vec::with_capacity(file_pairs.len());
    let mut completed_bytes = 0u64;

    for (index, ((source, dest), &file_size)) in
        file_pairs.iter().zip(file_sizes.iter()).enumerate()
    {
        let file_start_bytes = completed_bytes;

        // Progress callback for individual file
        let individual_progress = |bytes_copied: u64, _file_total: u64| {
            let current_total_bytes = file_start_bytes + bytes_copied;
            progress_callback(index, file_pairs.len(), current_total_bytes, total_bytes);
        };

        // Copy the file
        let result = copy_file_with_progress(source, dest, individual_progress).await;

        // Update completed bytes
        if result.is_ok() {
            completed_bytes += file_size;
        }

        results.push(result);

        // Report completion of this file
        progress_callback(index + 1, file_pairs.len(), completed_bytes, total_bytes);
    }

    Ok(results)
}

/// Copy directory recursively with progress reporting
pub async fn copy_directory_with_progress<P: AsRef<Path>, Q: AsRef<Path>, F>(
    from: P,
    to: Q,
    progress_callback: F,
) -> UtilResult<()>
where
    F: Fn(usize, usize, u64, u64) + Clone, // (files_completed, total_files, bytes_completed, total_bytes)
{
    let from = from.as_ref();
    let to = to.as_ref();

    // Ensure source exists and is a directory
    if !from.exists() {
        return Err(UtilError::Path {
            message: format!("Source directory does not exist: {}", from.display()),
        });
    }

    if !from.is_dir() {
        return Err(UtilError::Path {
            message: format!("Source is not a directory: {}", from.display()),
        });
    }

    // Collect all files to copy
    let mut file_pairs = Vec::new();
    collect_files_recursive(from, to, &mut file_pairs).await?;

    // Copy all files
    copy_files_with_progress(file_pairs, progress_callback).await?;

    Ok(())
}

/// Helper function to collect files recursively
async fn collect_files_recursive(
    source_dir: &Path,
    dest_dir: &Path,
    file_pairs: &mut Vec<(PathBuf, PathBuf)>,
) -> UtilResult<()> {
    let mut entries = tokio::fs::read_dir(source_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let source_path = entry.path();
        let relative_path = source_path
            .strip_prefix(source_dir)
            .map_err(|_| UtilError::Path {
                message: "Failed to get relative path".to_string(),
            })?;
        let dest_path = dest_dir.join(relative_path);

        if source_path.is_file() {
            file_pairs.push((source_path, dest_path));
        } else if source_path.is_dir() {
            // Create destination directory
            ensure_directory(&dest_path).await?;
            // Recurse into subdirectory
            collect_files_recursive(&source_path, &dest_path, file_pairs).await?;
        }
    }

    Ok(())
}

/// Copy file with verification (checksum comparison)
pub async fn copy_file_with_verification<P: AsRef<Path>, Q: AsRef<Path>, F>(
    from: P,
    to: Q,
    progress_callback: F,
) -> UtilResult<()>
where
    F: Fn(u64, u64), // (bytes_copied, total_bytes)
{
    use sha2::{Digest, Sha256};

    let from = from.as_ref();
    let to = to.as_ref();

    // Copy the file first
    copy_file_with_progress(from, to, progress_callback).await?;

    // Verify the copy by comparing checksums
    let source_hash = calculate_file_hash(from).await?;
    let dest_hash = calculate_file_hash(to).await?;

    if source_hash != dest_hash {
        // Remove the invalid copy
        if let Err(e) = tokio::fs::remove_file(to).await {
            warn!("Failed to remove invalid copy {}: {}", to.display(), e);
        }

        return Err(UtilError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "File copy verification failed: checksums do not match",
        )));
    }

    debug!("File copy verified successfully: {}", to.display());
    Ok(())
}

/// Calculate SHA256 hash of a file
async fn calculate_file_hash<P: AsRef<Path>>(path: P) -> UtilResult<String> {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_copy_file_with_progress() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.txt");
        let dest = temp_dir.path().join("dest.txt");

        // Create source file
        let content = "Hello, World!".repeat(1000); // Make it larger for progress testing
        tokio::fs::write(&source, &content).await.unwrap();

        // Track progress
        let progress_calls = Arc::new(Mutex::new(Vec::new()));
        let progress_calls_clone = progress_calls.clone();

        // Copy with progress
        copy_file_with_progress(&source, &dest, move |bytes_copied, total_bytes| {
            progress_calls_clone
                .lock()
                .unwrap()
                .push((bytes_copied, total_bytes));
        })
        .await
        .unwrap();

        // Verify copy
        let copied_content = tokio::fs::read_to_string(&dest).await.unwrap();
        assert_eq!(content, copied_content);

        // Verify progress was reported
        let calls = progress_calls.lock().unwrap();
        assert!(!calls.is_empty());
        assert_eq!(calls[0].0, 0); // First call should be 0 bytes
        assert_eq!(calls.last().unwrap().0, calls.last().unwrap().1); // Last call should be complete
    }

    #[tokio::test]
    async fn test_copy_with_cancellation() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("large_source.txt");
        let dest = temp_dir.path().join("dest.txt");

        // Create a large source file
        let content = "Hello, World!".repeat(100000); // Large enough to allow cancellation
        tokio::fs::write(&source, &content).await.unwrap();

        let cancellation_token = CancellationToken::new();
        let cancel_token = cancellation_token.clone();

        // Cancel after a short delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancel_token.cancel();
        });

        // Copy with cancellation
        let result = copy_file_with_progress_cancellable(
            &source,
            &dest,
            |_, _| {}, // No progress callback needed for this test
            &cancellation_token,
        )
        .await;

        // Should be cancelled
        assert!(matches!(result, Err(UtilError::Cancelled)));

        // Destination file should not exist (cleaned up)
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn test_copy_multiple_files() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source");
        let dest_dir = temp_dir.path().join("dest");

        tokio::fs::create_dir(&source_dir).await.unwrap();
        tokio::fs::create_dir(&dest_dir).await.unwrap();

        // Create multiple source files
        let mut file_pairs = Vec::new();
        for i in 0..3 {
            let source_file = source_dir.join(format!("file{}.txt", i));
            let dest_file = dest_dir.join(format!("file{}.txt", i));
            let content = format!("Content of file {}", i);

            tokio::fs::write(&source_file, &content).await.unwrap();
            file_pairs.push((source_file, dest_file));
        }

        // Track progress
        let progress_calls = Arc::new(Mutex::new(Vec::new()));
        let progress_calls_clone = progress_calls.clone();

        // Copy multiple files
        let results = copy_files_with_progress(
            file_pairs.clone(),
            move |files_done, total_files, bytes_done, total_bytes| {
                progress_calls_clone.lock().unwrap().push((
                    files_done,
                    total_files,
                    bytes_done,
                    total_bytes,
                ));
            },
        )
        .await
        .unwrap();

        // Verify all copies succeeded
        assert_eq!(results.len(), 3);
        for result in results {
            assert!(result.is_ok());
        }

        // Verify files were copied correctly
        for (i, (_, dest_file)) in file_pairs.iter().enumerate() {
            let content = tokio::fs::read_to_string(dest_file).await.unwrap();
            assert_eq!(content, format!("Content of file {}", i));
        }

        // Verify progress was reported
        let calls = progress_calls.lock().unwrap();
        assert!(!calls.is_empty());

        // Last call should show completion
        let last_call = calls.last().unwrap();
        assert_eq!(last_call.0, 3); // 3 files completed
        assert_eq!(last_call.1, 3); // 3 total files
    }

    #[tokio::test]
    async fn test_copy_directory_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source");
        let dest_dir = temp_dir.path().join("dest");

        // Create source directory structure
        tokio::fs::create_dir_all(source_dir.join("subdir"))
            .await
            .unwrap();
        tokio::fs::write(source_dir.join("file1.txt"), "Content 1")
            .await
            .unwrap();
        tokio::fs::write(source_dir.join("subdir/file2.txt"), "Content 2")
            .await
            .unwrap();

        // Copy directory
        copy_directory_with_progress(&source_dir, &dest_dir, |_, _, _, _| {})
            .await
            .unwrap();

        // Verify structure was copied
        assert!(dest_dir.join("file1.txt").exists());
        assert!(dest_dir.join("subdir").exists());
        assert!(dest_dir.join("subdir/file2.txt").exists());

        // Verify content
        let content1 = tokio::fs::read_to_string(dest_dir.join("file1.txt"))
            .await
            .unwrap();
        let content2 = tokio::fs::read_to_string(dest_dir.join("subdir/file2.txt"))
            .await
            .unwrap();
        assert_eq!(content1, "Content 1");
        assert_eq!(content2, "Content 2");
    }
}
