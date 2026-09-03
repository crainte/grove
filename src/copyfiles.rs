use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Outcome of a copy operation
///
/// Failures are collected rather than discarded so callers can warn about a
/// partial copy instead of silently reporting a short count.
#[derive(Debug, Default)]
pub struct CopyReport {
    pub copied: usize,
    /// (path, error message) for each entry that could not be copied
    pub failed: Vec<(String, String)>,
}

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

/// List files matching glob patterns from ignored files in a worktree
/// Patterns can be:
/// - Exact filenames: ".env"
/// - Glob patterns: ".env*", "*.log"
/// - Directories: ".terraform/" (copies entire directory)
pub fn list_matching_files(worktree: &Path, patterns: &[String]) -> Result<Vec<String>> {
    if patterns.is_empty() {
        return Ok(Vec::new());
    }

    // Get all ignored files
    let ignored = list_ignored_files(worktree)?;

    // Filter by patterns
    let matched: Vec<String> = ignored
        .into_iter()
        .filter(|file| {
            patterns.iter().any(|pattern| {
                let pattern = pattern.trim_end_matches('/');
                // Directory pattern: matches files under that directory
                if file.starts_with(&format!("{}/", pattern)) {
                    return true;
                }
                // Glob pattern with *
                if pattern.contains('*') {
                    return glob_match(pattern, file);
                }
                // Exact match
                file == pattern
            })
        })
        .collect();

    Ok(matched)
}

/// Simple glob matching (supports * wildcard)
fn glob_match(pattern: &str, text: &str) -> bool {
    // Split pattern by * and match parts
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        // No wildcard
        return pattern == text;
    }

    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if i == 0 {
            // First part must match at start
            if !text.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            // Last part must match at end
            if !text.ends_with(part) {
                return false;
            }
        } else {
            // Middle parts must exist somewhere after current position
            if let Some(found) = text[pos..].find(part) {
                pos += found + part.len();
            } else {
                return false;
            }
        }
    }

    true
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

/// Recreate a symlink at `dest` pointing at the same target as `src`
///
/// The target is reproduced verbatim: a relative target stays relative (and so
/// remains valid inside the new worktree), and an absolute target keeps pointing
/// at the same shared location (e.g. a Terraform provider plugin cache).
#[cfg(unix)]
fn copy_symlink(src: &Path, dest: &Path) -> Result<()> {
    let target =
        fs::read_link(src).with_context(|| format!("Failed to read symlink {}", src.display()))?;

    // symlink(2) fails with EEXIST, so clear any existing entry first.
    if let Ok(meta) = fs::symlink_metadata(dest) {
        if meta.is_dir() {
            fs::remove_dir_all(dest)
        } else {
            fs::remove_file(dest)
        }
        .with_context(|| format!("Failed to replace {}", dest.display()))?;
    }

    std::os::unix::fs::symlink(&target, dest).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            dest.display(),
            target.display()
        )
    })
}

#[cfg(not(unix))]
fn copy_symlink(src: &Path, _dest: &Path) -> Result<()> {
    anyhow::bail!(
        "Cannot copy symlink {}: symlinks are not supported on this platform",
        src.display()
    )
}

/// Copy a single entry, preserving symlinks rather than dereferencing them
fn copy_entry(src: &Path, dest: &Path) -> Result<()> {
    // symlink_metadata does not follow links, so a broken symlink is still seen
    // here (unlike Path::exists, which would report it as missing).
    let meta =
        fs::symlink_metadata(src).with_context(|| format!("Failed to stat {}", src.display()))?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    if meta.file_type().is_symlink() {
        return copy_symlink(src, dest);
    }

    fs::copy(src, dest)
        .with_context(|| format!("Failed to copy {}", src.display()))
        .map(|_| ())
}

/// Copy files from source to destination with parallel I/O and progress bar
pub fn copy_files_parallel(
    files: &[String],
    src_root: &Path,
    dest_root: &Path,
) -> Result<CopyReport> {
    if files.is_empty() {
        return Ok(CopyReport::default());
    }

    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.green} [{bar:40.cyan/dim}] {pos}/{len} files")
            .unwrap()
            .progress_chars("━━╸"),
    );

    let copied = AtomicUsize::new(0);
    let failed = Mutex::new(Vec::new());

    // Parallel copy using rayon
    files.par_iter().for_each(|file| {
        let src = src_root.join(file);
        let dest = dest_root.join(file);

        match copy_entry(&src, &dest) {
            Ok(()) => {
                copied.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                if let Ok(mut failed) = failed.lock() {
                    failed.push((file.clone(), e.to_string()));
                }
            }
        }

        pb.inc(1);
    });

    pb.finish_and_clear();

    let mut failed = failed.into_inner().unwrap_or_default();
    // Deterministic ordering for display
    failed.sort();

    Ok(CopyReport {
        copied: copied.load(Ordering::Relaxed),
        failed,
    })
}

/// Sync ignored files between worktrees
pub fn sync_ignored(
    src_worktree: &Path,
    dest_worktree: &Path,
    paths: &[String],
) -> Result<(CopyReport, Vec<String>)> {
    // Get ignored files from source
    let files = list_ignored_files(src_worktree)?;

    // Filter by paths if specified
    let files = filter_by_paths(files, paths);

    if files.is_empty() {
        return Ok((CopyReport::default(), vec![]));
    }

    // Copy in parallel with progress
    let report = copy_files_parallel(&files, src_worktree, dest_worktree)?;
    Ok((report, files))
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

    #[cfg(unix)]
    mod symlinks {
        use super::*;
        use std::os::unix::fs::symlink;
        use tempfile::TempDir;

        fn is_symlink(p: &Path) -> bool {
            fs::symlink_metadata(p)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        }

        #[test]
        fn test_copy_preserves_symlink_to_file() {
            let src = TempDir::new().expect("tempdir");
            let dest = TempDir::new().expect("tempdir");

            fs::write(src.path().join("target.txt"), "payload").expect("write");
            symlink("target.txt", src.path().join("link.txt")).expect("symlink");

            let files = vec!["link.txt".to_string()];
            let report =
                copy_files_parallel(&files, src.path(), dest.path()).expect("copy should succeed");

            let dest_link = dest.path().join("link.txt");
            assert!(is_symlink(&dest_link), "dest link.txt should be a symlink");
            assert_eq!(
                fs::read_link(&dest_link).expect("read_link"),
                Path::new("target.txt"),
                "relative target must be reproduced verbatim"
            );
            assert_eq!(report.copied, 1);
            assert!(report.failed.is_empty());
        }

        #[test]
        fn test_copy_preserves_symlink_to_directory() {
            let src = TempDir::new().expect("tempdir");
            let dest = TempDir::new().expect("tempdir");

            fs::create_dir(src.path().join("realdir")).expect("mkdir");
            fs::write(src.path().join("realdir/a.txt"), "inner").expect("write");
            symlink("realdir", src.path().join("dirlink")).expect("symlink");

            let files = vec!["dirlink".to_string()];
            let report =
                copy_files_parallel(&files, src.path(), dest.path()).expect("copy should succeed");

            // The real directory must also exist for the link to resolve.
            fs::create_dir_all(dest.path().join("realdir")).expect("mkdir");
            fs::write(dest.path().join("realdir/a.txt"), "inner").expect("write");

            let dest_link = dest.path().join("dirlink");
            assert!(is_symlink(&dest_link), "dest dirlink should be a symlink");
            assert_eq!(
                fs::read_to_string(dest.path().join("dirlink/a.txt")).expect("read through link"),
                "inner"
            );
            assert_eq!(report.copied, 1);
            assert!(report.failed.is_empty());
        }

        #[test]
        fn test_copy_preserves_absolute_symlink_outside_root() {
            let src = TempDir::new().expect("tempdir");
            let dest = TempDir::new().expect("tempdir");
            let outside = TempDir::new().expect("tempdir");

            fs::write(outside.path().join("provider.bin"), "binary").expect("write");
            let abs_target = outside.path().join("provider.bin");
            symlink(&abs_target, src.path().join("cached")).expect("symlink");

            let files = vec!["cached".to_string()];
            let report =
                copy_files_parallel(&files, src.path(), dest.path()).expect("copy should succeed");

            let dest_link = dest.path().join("cached");
            assert!(is_symlink(&dest_link), "dest cached should be a symlink");
            assert_eq!(
                fs::read_link(&dest_link).expect("read_link"),
                abs_target,
                "absolute target must not be rewritten or relativized"
            );
            assert_eq!(report.copied, 1);
        }

        #[test]
        fn test_copy_symlink_counts_toward_total() {
            let src = TempDir::new().expect("tempdir");
            let dest = TempDir::new().expect("tempdir");

            fs::write(src.path().join("plain.txt"), "data").expect("write");
            fs::create_dir(src.path().join("realdir")).expect("mkdir");
            symlink("realdir", src.path().join("dirlink")).expect("symlink");

            let files = vec!["plain.txt".to_string(), "dirlink".to_string()];
            let report =
                copy_files_parallel(&files, src.path(), dest.path()).expect("copy should succeed");

            assert_eq!(report.copied, 2, "both real file and symlink must count");
            assert!(report.failed.is_empty());
        }

        #[test]
        fn test_copy_replaces_existing_dest_symlink() {
            let src = TempDir::new().expect("tempdir");
            let dest = TempDir::new().expect("tempdir");

            fs::write(src.path().join("target.txt"), "payload").expect("write");
            symlink("target.txt", src.path().join("link.txt")).expect("symlink");

            // Pre-existing symlink at the destination pointing elsewhere.
            symlink("somewhere-else", dest.path().join("link.txt")).expect("symlink");

            let files = vec!["link.txt".to_string()];
            let report =
                copy_files_parallel(&files, src.path(), dest.path()).expect("copy should succeed");

            let dest_link = dest.path().join("link.txt");
            assert!(is_symlink(&dest_link));
            assert_eq!(
                fs::read_link(&dest_link).expect("read_link"),
                Path::new("target.txt"),
                "existing dest symlink must be replaced, not left in place"
            );
            assert_eq!(report.copied, 1);
        }

        #[test]
        fn test_copy_reports_failure_count() {
            let src = TempDir::new().expect("tempdir");
            let dest = TempDir::new().expect("tempdir");

            fs::write(src.path().join("present.txt"), "data").expect("write");

            let files = vec!["present.txt".to_string(), "missing.txt".to_string()];
            let report = copy_files_parallel(&files, src.path(), dest.path())
                .expect("copy should not hard-fail");

            assert_eq!(report.copied, 1, "only the present file copies");
            assert_eq!(report.failed.len(), 1, "the missing entry must be reported");
            assert_eq!(report.failed[0].0, "missing.txt");
        }
    }
}
