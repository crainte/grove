use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Find the git repository root from current directory
/// Handles both main repo and worktrees correctly
/// Returns Err with special message if in deleted worktree
pub fn find_repo_root() -> Result<PathBuf> {
    // Get the git common dir (always points to main repo's .git)
    let output = Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .context("Failed to execute git")?;

    if !output.status.success() {
        // Check if we're in an orphaned worktree directory (deleted but shell still there)
        if let Some(repo_root) = detect_orphaned_worktree() {
            bail!("ORPHANED_WORKTREE:{}", repo_root.display());
        }
        bail!("Not in a git repository");
    }

    let git_common = String::from_utf8(output.stdout)
        .context("Invalid UTF-8 in git output")?
        .trim()
        .to_string();

    // git_common is like "/repo/.git" or ".git" - get parent
    let git_common_path = PathBuf::from(&git_common);
    let git_common_abs = if git_common_path.is_absolute() {
        git_common_path
    } else {
        std::env::current_dir()?
            .join(&git_common_path)
            .canonicalize()?
    };

    // Parent of .git is repo root
    git_common_abs
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow::anyhow!("Invalid git directory structure"))
}

/// Detect if we're in an orphaned worktree (directory deleted but shell still there)
/// Returns the main repo path if detected
fn detect_orphaned_worktree() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let cwd_str = cwd.to_str()?;

    // Check if path contains /.git/wt/
    if let Some(idx) = cwd_str.find("/.git/wt/") {
        let main_repo = PathBuf::from(&cwd_str[..idx]);
        // Verify the main repo still exists
        if main_repo.is_dir() && main_repo.join(".git").exists() {
            return Some(main_repo);
        }
    }

    None
}

/// Get the default branch name (main or master)
pub fn default_branch(repo_root: &Path) -> Result<String> {
    // Check for main first, then master
    for branch in ["main", "master"] {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{}", branch)])
            .current_dir(repo_root)
            .output()?;

        if output.status.success() {
            return Ok(branch.to_string());
        }
    }

    // Fall back to getting the default branch from remote
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(repo_root)
        .output()?;

    if output.status.success() {
        let refname = String::from_utf8(output.stdout)?.trim().to_string();
        if let Some(branch) = refname.strip_prefix("refs/remotes/origin/") {
            return Ok(branch.to_string());
        }
    }

    bail!("Could not determine main branch")
}

/// Check if a branch exists
pub fn branch_exists(repo_root: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", &format!("refs/heads/{}", branch)])
        .current_dir(repo_root)
        .output()?;

    Ok(output.status.success())
}

/// Create a git worktree with new branch
pub fn worktree_add(repo_root: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            branch,
            path.to_str().unwrap(),
            base,
        ])
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree add failed: {}", stderr.trim());
    }

    Ok(())
}

/// Remove a git worktree
pub fn worktree_remove(repo_root: &Path, path: &Path, force: bool) -> Result<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(path.to_str().unwrap());

    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git worktree remove")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree remove failed: {}", stderr.trim());
    }

    Ok(())
}

/// Delete a branch
pub fn branch_delete(repo_root: &Path, branch: &str, force: bool) -> Result<()> {
    let flag = if force { "-D" } else { "-d" };
    let output = Command::new("git")
        .args(["branch", flag, branch])
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git branch delete")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Friendlier message for "not fully merged" error
        if stderr.contains("not fully merged") {
            bail!(
                "Branch '{}' not fully merged. Use 'grove rm --force {}' to remove anyway.",
                branch,
                branch
            );
        }
        bail!("git branch delete failed: {}", stderr.trim());
    }

    Ok(())
}

/// Check if a branch is merged into another branch.
/// Works for all merge strategies: fast-forward, merge commit, squash, and rebase.
/// Compares what merging would produce vs current target tree.
pub fn is_branch_merged(repo_root: &Path, branch: &str, into: &str) -> Result<bool> {
    // Compute what tree merging branch into target would produce
    let merge_tree = Command::new("git")
        .args(["merge-tree", "--write-tree", into, branch])
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git merge-tree")?;

    if !merge_tree.status.success() {
        // Merge conflict or branch doesn't exist — not cleanly merged
        return Ok(false);
    }

    let merge_result = String::from_utf8_lossy(&merge_tree.stdout)
        .trim()
        .to_string();

    // Get target's current tree
    let target_tree = Command::new("git")
        .args(["rev-parse", &format!("{}^{{tree}}", into)])
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git rev-parse")?;

    if !target_tree.status.success() {
        return Ok(false);
    }

    let target_result = String::from_utf8_lossy(&target_tree.stdout)
        .trim()
        .to_string();

    // If merging would produce the same tree as target, branch is already merged
    Ok(merge_result == target_result)
}

/// Pull latest changes
pub fn pull(repo_root: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["pull"])
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git pull")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git pull failed: {}", stderr.trim());
    }

    Ok(())
}

/// Prune stale worktree references
pub fn worktree_prune(repo_root: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git worktree prune")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree prune failed: {}", stderr.trim());
    }

    Ok(())
}

/// Check worktree status
/// Returns (has_modified, has_untracked)
pub fn worktree_status(worktree_path: &Path) -> Result<(bool, bool)> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_path)
        .output()
        .context("Failed to execute git status")?;

    if !output.status.success() {
        bail!("git status failed");
    }

    let mut has_modified = false;
    let mut has_untracked = false;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.starts_with("??") {
            has_untracked = true;
        } else if !line.is_empty() {
            has_modified = true;
        }
    }

    Ok((has_modified, has_untracked))
}

/// Get ahead/behind counts for a branch
/// Returns (ahead, behind) relative to upstream or parent branch
pub fn ahead_behind(
    worktree_path: &Path,
    branch: &str,
    compare_to: Option<&str>,
) -> Result<(u32, u32)> {
    // First try upstream tracking branch
    let upstream = Command::new("git")
        .args([
            "rev-parse",
            "--abbrev-ref",
            &format!("{}@{{upstream}}", branch),
        ])
        .current_dir(worktree_path)
        .output()
        .context("Failed to get upstream")?;

    let compare_ref = if upstream.status.success() {
        String::from_utf8_lossy(&upstream.stdout).trim().to_string()
    } else if let Some(parent) = compare_to {
        parent.to_string()
    } else {
        return Ok((0, 0)); // Nothing to compare to
    };

    let output = Command::new("git")
        .args([
            "rev-list",
            "--left-right",
            "--count",
            &format!("{}...{}", branch, compare_ref),
        ])
        .current_dir(worktree_path)
        .output()
        .context("Failed to get ahead/behind")?;

    if !output.status.success() {
        return Ok((0, 0));
    }

    let counts = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = counts.split_whitespace().collect();

    if parts.len() == 2 {
        let ahead = parts[0].parse().unwrap_or(0);
        let behind = parts[1].parse().unwrap_or(0);
        Ok((ahead, behind))
    } else {
        Ok((0, 0))
    }
}

/// Get the wt directory for storing worktrees
pub fn wt_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".git/wt")
}

/// Info about a git worktree
#[derive(Debug)]
pub struct GitWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// List all git worktrees
pub fn worktree_list(repo_root: &Path) -> Result<Vec<GitWorktree>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .context("Failed to execute git worktree list")?;

    if !output.status.success() {
        bail!("git worktree list failed");
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            // Save previous worktree if exists
            if let Some(path) = current_path.take() {
                worktrees.push(GitWorktree {
                    path,
                    branch: current_branch.take(),
                });
            }
            current_path = Some(PathBuf::from(path));
            current_branch = None;
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            // Extract branch name from refs/heads/name
            current_branch = branch_ref.strip_prefix("refs/heads/").map(String::from);
        }
        // Empty line marks end of a worktree entry
        else if line.is_empty()
            && let Some(path) = current_path.take()
        {
            worktrees.push(GitWorktree {
                path,
                branch: current_branch.take(),
            });
        }
    }

    // Don't forget the last one
    if let Some(path) = current_path {
        worktrees.push(GitWorktree {
            path,
            branch: current_branch,
        });
    }

    Ok(worktrees)
}

/// Check if fzf is available
pub fn has_fzf() -> bool {
    Command::new("fzf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run fzf with given choices, return selected item
pub fn fzf_select(choices: &[String], prompt: &str) -> Result<Option<String>> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("fzf")
        .args([
            "--prompt", prompt,
            "--height", "40%",
            "--reverse",
            "--border", "rounded",
            "--border-label", " 🌳 Grove ",
            "--color", "fg:-1,bg:-1,hl:cyan,fg+:white,bg+:-1,hl+:cyan:bold,info:yellow,prompt:green,pointer:magenta,marker:magenta,border:dim",
            "--pointer", "▶",
            "--marker", "●",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn fzf")?;

    // Write choices to fzf stdin
    if let Some(mut stdin) = child.stdin.take() {
        for choice in choices {
            writeln!(stdin, "{}", choice)?;
        }
    }

    let output = child.wait_with_output()?;

    if output.status.success() {
        let selection = String::from_utf8(output.stdout)?.trim().to_string();
        if selection.is_empty() {
            Ok(None)
        } else {
            Ok(Some(selection))
        }
    } else {
        Ok(None) // User cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wt_dir() {
        let root = PathBuf::from("/repo");
        assert_eq!(wt_dir(&root), PathBuf::from("/repo/.git/wt"));
    }
}
