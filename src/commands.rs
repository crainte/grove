use anyhow::{bail, Result};

use crate::shell;

/// Go to worktree (create if needed)
pub fn go(name: &str, base: Option<&str>) -> Result<()> {
    // TODO: Implement
    // 1. Find repo root
    // 2. Load metadata
    // 3. Find worktree by name (context-aware)
    // 4. If exists, output cd command
    // 5. If not, create it then cd
    bail!("go command not yet implemented: {} {:?}", name, base)
}

/// Create worktree without switching
pub fn add(name: &str, base: Option<&str>) -> Result<()> {
    // TODO: Implement
    bail!("add command not yet implemented: {} {:?}", name, base)
}

/// Remove worktree
pub fn rm(name: &str, force: bool) -> Result<()> {
    // TODO: Implement
    bail!("rm command not yet implemented: {} force={}", name, force)
}

/// List worktrees
pub fn list() -> Result<()> {
    // TODO: Implement tree display
    bail!("list command not yet implemented")
}

/// Clean stale worktree references
pub fn prune() -> Result<()> {
    // TODO: Implement - delegate to git worktree prune
    bail!("prune command not yet implemented")
}

/// Remove merged worktrees
pub fn clean(branch: Option<&str>) -> Result<()> {
    // TODO: Implement
    bail!("clean command not yet implemented: {:?}", branch)
}

/// Finish up: cd to main, pull, clean
pub fn done() -> Result<()> {
    // TODO: Implement
    bail!("done command not yet implemented")
}

/// Copy ignored files from main to current worktree
pub fn pull(paths: &[String]) -> Result<()> {
    // TODO: Implement
    bail!("pull command not yet implemented: {:?}", paths)
}

/// Copy ignored files from current worktree to main
pub fn push(paths: &[String]) -> Result<()> {
    // TODO: Implement
    bail!("push command not yet implemented: {:?}", paths)
}

/// Print path to worktree
pub fn path(name: &str) -> Result<()> {
    // TODO: Implement
    bail!("path command not yet implemented: {}", name)
}

// Helper: find git repo root
fn _find_repo_root() -> Result<std::path::PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    
    if !output.status.success() {
        bail!("Not in a git repository");
    }
    
    let path = String::from_utf8(output.stdout)?
        .trim()
        .to_string();
    Ok(std::path::PathBuf::from(path))
}

// Helper: get current worktree ID from cwd
fn _current_worktree_id() -> Result<Option<String>> {
    // TODO: Check if cwd is under .git/wt/<id>/ and extract id
    Ok(None)
}
