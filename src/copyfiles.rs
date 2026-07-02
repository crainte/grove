use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::fs;

/// Get list of ignored files in a worktree
pub fn list_ignored_files(worktree: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--ignored", "--exclude-standard"])
        .current_dir(worktree)
        .output()
        .context("Failed to list ignored files")?;

    if !output.status.success() {
        anyhow::bail!("git ls-files failed");
    }

    let files: Vec<String> = String::from_utf8(output.stdout)?
        .lines()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    Ok(files)
}

/// Filter files by path prefixes
pub fn filter_by_paths(files: Vec<String>, paths: &[String]) -> Vec<String> {
    if paths.is_empty() {
        return files;
    }

    files
        .into_iter()
        .filter(|file| {
            paths.iter().any(|path| {
                let path = path.trim_end_matches('/');
                file == path || file.starts_with(&format!("{}/", path))
            })
        })
        .collect()
}

/// Copy files from source to destination with parallel I/O and progress bar
pub fn copy_files_parallel(
    files: &[String],
    src_root: &Path,
    dest_root: &Path,
) -> Result<usize> {
    if files.is_empty() {
        return Ok(0);
    }

    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.green} [{bar:40.cyan/dim}] {pos}/{len} files")
            .unwrap()
            .progress_chars("━━╸"),
    );

    let copied = AtomicUsize::new(0);

    // Parallel copy using rayon
    files.par_iter().for_each(|file| {
        let src = src_root.join(file);
        let dest = dest_root.join(file);

        if src.exists() {
            // Create parent directories
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }

            // Copy file (preserve metadata with copy)
            if fs::copy(&src, &dest).is_ok() {
                copied.fetch_add(1, Ordering::Relaxed);
            }
        }

        pb.inc(1);
    });

    pb.finish_and_clear();

    Ok(copied.load(Ordering::Relaxed))
}

/// Sync ignored files between worktrees
pub fn sync_ignored(
    src_worktree: &Path,
    dest_worktree: &Path,
    paths: &[String],
) -> Result<(usize, Vec<String>)> {
    // Get ignored files from source
    let files = list_ignored_files(src_worktree)?;
    
    // Filter by paths if specified
    let files = filter_by_paths(files, paths);

    if files.is_empty() {
        return Ok((0, vec![]));
    }

    // Copy in parallel with progress
    let count = copy_files_parallel(&files, src_worktree, dest_worktree)?;
    Ok((count, files))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_by_paths_empty() {
        let files = vec!["a.log".to_string(), "b.log".to_string()];
        let filtered = filter_by_paths(files.clone(), &[]);
        assert_eq!(filtered, files);
    }

    #[test]
    fn test_filter_by_paths_exact() {
        let files = vec![".env".to_string(), "app.log".to_string()];
        let filtered = filter_by_paths(files, &[".env".to_string()]);
        assert_eq!(filtered, vec![".env"]);
    }

    #[test]
    fn test_filter_by_paths_prefix() {
        let files = vec![
            "logs/app.log".to_string(),
            "logs/error.log".to_string(),
            ".env".to_string(),
        ];
        let filtered = filter_by_paths(files, &["logs".to_string()]);
        assert_eq!(filtered, vec!["logs/app.log", "logs/error.log"]);
    }
}
