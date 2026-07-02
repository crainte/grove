use anyhow::{Result, bail};
use std::env;
use std::path::{Path, PathBuf};

use crate::git;
use crate::meta::Meta;
use crate::shell;

/// Check if we're in an orphaned worktree and return to main repo if so
/// Returns true if we handled the orphaned case (caller should exit)
pub fn check_orphaned_worktree() -> Result<bool> {
    use colored::Colorize;

    match git::find_repo_root() {
        Ok(_) => Ok(false), // Normal case, not orphaned
        Err(e) => {
            let msg = e.to_string();
            if let Some(repo_path) = msg.strip_prefix("ORPHANED_WORKTREE:") {
                // We're in a deleted worktree - return to main repo
                eprintln!(
                    "{}",
                    "⚠️  Current worktree was removed. Returning to main repo...".yellow()
                );
                shell::output_cd(&PathBuf::from(repo_path));
                Ok(true)
            } else {
                // Some other error - propagate it
                Err(e)
            }
        }
    }
}

/// Go to worktree interactively using fzf
pub fn go_interactive() -> Result<()> {
    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;

    // Check if fzf is available
    if !git::has_fzf() {
        // Fall back to list
        return list();
    }

    // Build list of choices: main branch + all worktrees
    let mut choices = Vec::new();

    // Add main branch
    let main_branch = git::default_branch(&repo_root)?;
    choices.push(main_branch.clone());

    // Add all worktrees
    for (_, info) in meta.top_level()? {
        choices.push(info.branch.clone());
        // Add children recursively
        add_children_to_choices(
            &meta,
            &meta.find_by_branch(&info.branch)?.unwrap(),
            &mut choices,
        )?;
    }

    // Run fzf
    if let Some(selection) = git::fzf_select(&choices, "🌳 Go to worktree > ")? {
        return go(&selection, None);
    }

    Ok(()) // User cancelled
}

fn add_children_to_choices(meta: &Meta, parent_id: &str, choices: &mut Vec<String>) -> Result<()> {
    for (id, info) in meta.children(parent_id)? {
        choices.push(info.branch.clone());
        add_children_to_choices(meta, &id, choices)?;
    }
    Ok(())
}

/// Go to worktree (create if needed)
pub fn go(name: &str, base: Option<&str>) -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;

    // Get current worktree context for child lookup
    let current_id = current_worktree_id(&repo_root)?;

    // Check if requesting the primary worktree (main/master branch)
    let main_branch = git::default_branch(&repo_root)?;
    if name == main_branch {
        eprintln!("{} 📂 Switched to worktree '{}'", "✓".green(), name.cyan());
        eprintln!("  {}", repo_root.display().to_string().dimmed());
        shell::output_cd(&repo_root);
        return Ok(());
    }

    // Try to find existing worktree
    if let Some(id) = meta.find_by_branch_with_context(name, current_id.as_deref())? {
        let wt_path = meta.worktree_path(&id);
        eprintln!("{} 📂 Switched to worktree '{}'", "✓".green(), name.cyan());
        eprintln!("  {}", wt_path.display().to_string().dimmed());
        shell::output_cd(&wt_path);
        return Ok(());
    }

    // Create new worktree
    // If base is explicitly specified, create as top-level (no parent)
    // Otherwise, inherit current worktree as parent (contextual child)
    let parent_id = if base.is_some() {
        None
    } else {
        current_id.as_deref()
    };
    let id = create_worktree(&repo_root, &meta, name, base, parent_id)?;
    let wt_path = meta.worktree_path(&id);

    eprintln!("{} 📂 Created worktree '{}'", "✓".green(), name.cyan());
    eprintln!("  {}", wt_path.display().to_string().dimmed());
    shell::output_cd(&wt_path);
    Ok(())
}

/// Create worktree without switching
pub fn add(name: &str, base: Option<&str>) -> Result<()> {
    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;

    // Check if worktree already exists
    if meta.find_by_branch(name)?.is_some() {
        bail!("Worktree '{}' already exists", name);
    }

    // Get current worktree context for parent
    // If base is explicitly specified, create as top-level (no parent)
    // Otherwise, inherit current worktree as parent (contextual child)
    let current_id = current_worktree_id(&repo_root)?;
    let parent_id = if base.is_some() {
        None
    } else {
        current_id.as_deref()
    };

    let id = create_worktree(&repo_root, &meta, name, base, parent_id)?;
    let wt_path = meta.worktree_path(&id);

    eprintln!("Created worktree '{}' at {}", name, wt_path.display());
    Ok(())
}

/// Remove worktree
pub fn rm(name: &str, force: bool) -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;

    // Find the worktree
    let id = meta
        .find_by_branch(name)?
        .ok_or_else(|| anyhow::anyhow!("Worktree '{}' not found", name))?;

    let wt_path = meta.worktree_path(&id);

    // Check for uncommitted changes unless force
    if !force && wt_path.exists() && git::is_dirty(&wt_path)? {
        bail!(
            "Worktree '{}' has uncommitted changes. Use --force to remove anyway.",
            name
        );
    }

    eprintln!(
        "{}",
        format!("🗑️  Removing worktree '{}'...", name.cyan().bold()).yellow()
    );

    // Remove git worktree
    if wt_path.exists() {
        git::worktree_remove(&repo_root, &wt_path, force)?;
    }

    // Delete the branch
    if git::branch_exists(&repo_root, name)? {
        git::branch_delete(&repo_root, name, force)?;
    }

    // Remove from metadata
    meta.remove_worktree(&id)?;

    eprintln!(
        "{}",
        format!("✓ Removed worktree '{}'", name.cyan()).green()
    );
    Ok(())
}

/// List worktrees
pub fn list() -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;
    let current_id = current_worktree_id(&repo_root)?;

    // Header
    eprintln!("{}", "🌳 Git Worktrees".bold());
    eprintln!(
        "{}",
        "────────────────────────────────────────────────────────────────".bright_black()
    );
    eprintln!();

    // Get repo name from path
    let repo_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo");

    eprintln!(
        "📁 {} ({})",
        repo_name.bold().white(),
        repo_root.display().to_string().bright_black()
    );

    // Collect all entries: main + worktrees
    let main_branch = git::default_branch(&repo_root).unwrap_or_else(|_| "main".to_string());
    let top_level = meta.top_level()?;
    let orphans = meta.orphans()?;

    // Print main worktree (repo root) - primary gets green marker
    let is_main_current = current_id.is_none();
    let main_is_last = top_level.is_empty() && orphans.is_empty();
    let main_has_children = false; // main worktree doesn't have children in our model
    print_worktree_entry(
        &main_branch,
        &repo_root,
        is_main_current,
        true,
        main_is_last,
        main_has_children,
        "",
        false,
    );

    // Print top-level worktrees
    let top_count = top_level.len();
    for (i, (id, info)) in top_level.iter().enumerate() {
        let is_last = i == top_count - 1; // Orphans are separate section
        let wt_path = meta.worktree_path(id);
        let is_current = current_id.as_deref() == Some(id.as_str());
        let has_children = !meta.children(id)?.is_empty();
        print_worktree_entry(
            &info.branch,
            &wt_path,
            is_current,
            false,
            is_last,
            has_children,
            "",
            false,
        );

        // Children are indented 3 spaces from parent's connector
        // Use │ continuation only if parent is not last sibling
        let child_prefix = if is_last { "   " } else { "│  " };
        print_worktree_children(&meta, id, current_id.as_deref(), child_prefix, false)?;
    }

    // Print orphaned worktrees (parent was deleted) - separate section
    if !orphans.is_empty() {
        // Visual break before orphans
        eprintln!("{}", "┊".truecolor(120, 100, 140));
    }
    for (i, (id, info, depth)) in orphans.iter().enumerate() {
        let is_last = i == orphans.len() - 1;
        let wt_path = meta.worktree_path(id);
        let is_current = current_id.as_deref() == Some(id.as_str());
        let has_children = !meta.children(id)?.is_empty();

        // Indent based on depth (how deep in the tree they were)
        let orphan_prefix = "   ".repeat(depth.saturating_sub(1));
        print_orphan_entry(
            &info.branch,
            &wt_path,
            is_current,
            is_last,
            has_children,
            &orphan_prefix,
        );

        // Children of orphans use normal tree display (3-char indent, matching regular tree)
        let child_prefix = format!("{}{}", orphan_prefix, if is_last { "   " } else { "│  " });
        print_worktree_children(&meta, id, current_id.as_deref(), &child_prefix, true)?;
    }

    // Footer suggestion
    eprintln!();
    eprintln!(
        "{}",
        "💡 Use 'grove go <name>' to switch, 'grove rm <name>' to remove".bright_blue()
    );

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn print_worktree_entry(
    name: &str,
    path: &std::path::Path,
    is_current: bool,
    is_primary: bool,
    is_last: bool,
    has_children: bool,
    prefix: &str,
    is_orphan: bool,
) {
    use colored::Colorize;

    let connector = if is_last { "└─" } else { "├─" };
    // Path line has two continuation indicators:
    // 1. Sibling continuation (3 chars): │ if not last, spaces if last
    // 2. Child continuation (3 chars): │ if has children, spaces if not
    let path_prefix = match (is_last, has_children) {
        (false, true) => "│  │  ",  // sibling + child continuation
        (false, false) => "│     ", // sibling continuation only
        (true, true) => "   │  ",   // child continuation only
        (true, false) => "      ",  // no continuation
    };

    // Colors: dimmer for orphans
    let tree_color = if is_orphan {
        (120, 100, 140)
    } else {
        (180, 160, 200)
    };

    // Marker: filled green if current, hollow green if primary, hollow dim otherwise
    let marker = if is_current {
        "●".green().to_string()
    } else if is_primary {
        "○".green().to_string()
    } else if is_orphan {
        "○".truecolor(150, 150, 150).to_string() // dimmer for orphans
    } else {
        "○".bright_black().to_string()
    };

    let branch_name = if is_current {
        name.bold().cyan().to_string()
    } else if is_orphan {
        name.truecolor(180, 180, 180).to_string() // dimmer for orphans
    } else {
        name.cyan().to_string()
    };

    let here_suffix = if is_current {
        " ← here".green().to_string()
    } else {
        String::new()
    };

    eprintln!(
        "{}{} {} {}{}",
        prefix.truecolor(tree_color.0, tree_color.1, tree_color.2),
        connector.truecolor(tree_color.0, tree_color.1, tree_color.2),
        marker,
        branch_name,
        here_suffix
    );

    // Print path below
    eprintln!(
        "{}{}{}",
        prefix.truecolor(tree_color.0, tree_color.1, tree_color.2),
        path_prefix.truecolor(tree_color.0, tree_color.1, tree_color.2),
        path.display().to_string().bright_black()
    );
}

/// Print an orphaned worktree entry with broken connector
fn print_orphan_entry(
    name: &str,
    path: &std::path::Path,
    is_current: bool,
    is_last: bool,
    has_children: bool,
    prefix: &str,
) {
    use colored::Colorize;

    // Use normal tree connectors (the ┊ separator already shows they're orphaned)
    let connector = if is_last { "└─" } else { "├─" };
    let path_prefix = match (is_last, has_children) {
        (false, true) => "│  │  ",
        (false, false) => "│     ",
        (true, true) => "   │  ",
        (true, false) => "      ",
    };

    let marker = if is_current {
        "●".green().to_string()
    } else {
        "○".truecolor(150, 150, 150).to_string() // dimmer for orphans
    };

    let branch_name = if is_current {
        name.bold().cyan().to_string()
    } else {
        name.truecolor(180, 180, 180).to_string() // dimmer for orphans
    };

    let here_suffix = if is_current {
        " ← here".green().to_string()
    } else {
        String::new()
    };

    eprintln!(
        "{}{} {} {}{}",
        prefix.truecolor(120, 100, 140),    // color prefix too
        connector.truecolor(120, 100, 140), // dimmer purple for orphans
        marker,
        branch_name,
        here_suffix
    );

    // Print path below
    eprintln!(
        "{}{}{}",
        prefix.truecolor(120, 100, 140), // color prefix too
        path_prefix.truecolor(120, 100, 140),
        path.display().to_string().bright_black()
    );
}

fn print_worktree_children(
    meta: &Meta,
    parent_id: &str,
    current_id: Option<&str>,
    prefix: &str,
    is_orphan: bool,
) -> Result<()> {
    let children = meta.children(parent_id)?;
    let child_count = children.len();

    for (i, (child_id, child_info)) in children.iter().enumerate() {
        let is_last = i == child_count - 1;
        let is_current = current_id == Some(child_id.as_str());
        let wt_path = meta.worktree_path(child_id);
        let has_children = !meta.children(child_id)?.is_empty();

        print_worktree_entry(
            &child_info.branch,
            &wt_path,
            is_current,
            false,
            is_last,
            has_children,
            prefix,
            is_orphan,
        );

        // Recurse for nested children - 3 char indent
        let next_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });
        print_worktree_children(meta, child_id, current_id, &next_prefix, is_orphan)?;
    }

    Ok(())
}

/// Clean stale worktree references
pub fn prune() -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    eprintln!("{}", "🧹 Pruning stale worktree references...".yellow());
    git::worktree_prune(&repo_root)?;
    eprintln!("{}", "✓ Pruned".green());
    Ok(())
}

/// Sync database with git worktrees
pub fn sync() -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;

    // Get worktrees from git
    let git_worktrees = git::worktree_list(&repo_root)?;

    // Sync
    let (imported, removed) = meta.sync(&git_worktrees)?;

    if imported > 0 || removed > 0 {
        eprintln!(
            "{}",
            format!("✓ Synced: {} imported, {} removed", imported, removed).green()
        );
    } else {
        eprintln!("{}", "✓ Already in sync".green());
    }

    Ok(())
}

/// Remove merged worktrees
pub fn clean(target_branch: Option<&str>) -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;
    let main_branch = git::default_branch(&repo_root)?;
    let target = target_branch.unwrap_or(&main_branch);

    // Track if we're removing the current worktree
    let current_id = current_worktree_id(&repo_root)?;
    let mut removed_current = false;

    // Find all worktrees merged into target
    let mut removed = 0;
    let mut skipped = 0;

    for (id, info) in meta.all()? {
        // Check if branch is merged
        if git::is_branch_merged(&repo_root, &info.branch, target)? {
            let wt_path = meta.worktree_path(&id);

            // Check for uncommitted changes
            if wt_path.exists() && git::is_dirty(&wt_path)? {
                eprintln!(
                    "{}",
                    format!("⚠ Skipping '{}': has uncommitted changes", info.branch).yellow()
                );
                skipped += 1;
                continue;
            }

            // Track if this is the current worktree
            if current_id.as_deref() == Some(id.as_str()) {
                removed_current = true;
            }

            // Remove the worktree
            eprintln!(
                "{}",
                format!("✓ Removing merged worktree '{}'", info.branch).green()
            );

            if wt_path.exists() {
                git::worktree_remove(&repo_root, &wt_path, false)?;
            }
            git::branch_delete(&repo_root, &info.branch, false)?;
            meta.remove(&id)?;
            removed += 1;
        }
    }

    if removed == 0 && skipped == 0 {
        eprintln!("{}", "✓ No merged worktrees to clean".green());
    } else {
        eprintln!(
            "{}",
            format!(
                "✓ Cleaned {} worktree(s){}",
                removed,
                if skipped > 0 {
                    format!(", skipped {}", skipped)
                } else {
                    String::new()
                }
            )
            .green()
        );
    }

    // If we removed the current worktree, cd to main repo
    if removed_current {
        shell::output_cd(&repo_root);
    }

    Ok(())
}

/// Finish up: cd to main, pull, clean
pub fn done() -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;

    // Pull latest on main
    eprintln!("{}", "⟳ Pulling latest...".cyan());
    git::pull(&repo_root)?;

    // Clean merged worktrees
    eprintln!("{}", "⟳ Cleaning merged worktrees...".cyan());
    clean(None)?;

    // cd to main
    shell::output_cd(&repo_root);
    Ok(())
}

/// Copy ignored files from main to current worktree
pub fn pull(paths: &[String]) -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let current_id = current_worktree_id(&repo_root)?;

    // Must be in a worktree, not main
    let current_id =
        current_id.ok_or_else(|| anyhow::anyhow!("Cannot pull: already in primary worktree"))?;

    let meta = Meta::open(&repo_root)?;
    let current_wt = meta.worktree_path(&current_id);

    // Source is repo root (main worktree)
    let src = &repo_root;
    let dest = &current_wt;

    if paths.is_empty() {
        eprintln!("{}", "📥 Pulling ignored files from main...".cyan());
    } else {
        eprintln!("{} {}", "📥 Pulling from main:".cyan(), paths.join(", "));
    }

    let (count, files) = crate::copyfiles::sync_ignored(src, dest, paths)?;

    if !files.is_empty() {
        for line in summarize_files(&files) {
            eprintln!("  {}", line.dimmed());
        }
    }

    eprintln!("{}", format!("✓ Pulled {} file(s)", count).green());
    Ok(())
}

/// Copy ignored files from current worktree to main
pub fn push(paths: &[String]) -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let current_id = current_worktree_id(&repo_root)?;

    // Must be in a worktree, not main
    let current_id =
        current_id.ok_or_else(|| anyhow::anyhow!("Cannot push: already in primary worktree"))?;

    let meta = Meta::open(&repo_root)?;
    let current_wt = meta.worktree_path(&current_id);

    // Source is current worktree, dest is repo root
    let src = &current_wt;
    let dest = &repo_root;

    if paths.is_empty() {
        eprintln!("{}", "📤 Pushing ignored files to main...".cyan());
    } else {
        eprintln!("{} {}", "📤 Pushing to main:".cyan(), paths.join(", "));
    }

    let (count, files) = crate::copyfiles::sync_ignored(src, dest, paths)?;

    if !files.is_empty() {
        for line in summarize_files(&files) {
            eprintln!("  {}", line.dimmed());
        }
    }

    eprintln!("{}", format!("✓ Pushed {} file(s)", count).green());
    Ok(())
}

/// Print path to worktree
pub fn path(name: &str) -> Result<()> {
    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;

    let id = meta
        .find_by_branch(name)?
        .ok_or_else(|| anyhow::anyhow!("Worktree '{}' not found", name))?;

    let wt_path = meta.worktree_path(&id);
    println!("{}", wt_path.display());
    Ok(())
}

// === Helper functions ===

/// Create a new worktree
fn create_worktree(
    repo_root: &Path,
    meta: &Meta,
    branch: &str,
    base: Option<&str>,
    parent_id: Option<&str>,
) -> Result<String> {
    // Determine base branch
    let base_branch = match base {
        Some(b) => b.to_string(),
        None => git::default_branch(repo_root)?,
    };

    // Check if branch already exists
    if git::branch_exists(repo_root, branch)? {
        bail!(
            "Branch '{}' already exists. Use a different name or check it out.",
            branch
        );
    }

    // Add to metadata first to get the ID (atomic in SQLite)
    let id = meta.add_worktree(branch, parent_id)?;
    let wt_path = meta.worktree_path(&id);

    // Create the git worktree
    use colored::Colorize;
    eprintln!(
        "{}",
        format!("🌱 Creating worktree '{}'...", branch.cyan().bold()).yellow()
    );
    eprintln!("  {}", format!("Base: {}", base_branch).dimmed());

    if let Err(e) = git::worktree_add(repo_root, &wt_path, branch, &base_branch) {
        // Rollback metadata on failure
        meta.remove_worktree(&id)?;
        return Err(e);
    }

    // Copy ignored files from main worktree (if enabled)
    if git::copyignored_enabled(repo_root) {
        use colored::Colorize;
        let ignored = crate::copyfiles::list_ignored_files(repo_root)?;
        if !ignored.is_empty() {
            // Show what we're copying
            let summary = summarize_files(&ignored);
            eprintln!(
                "  {}",
                format!("Copying {} ignored files...", ignored.len()).dimmed()
            );
            for line in &summary {
                eprintln!("    {}", line.dimmed());
            }

            let copied = crate::copyfiles::copy_files_parallel(&ignored, repo_root, &wt_path)?;
            if copied > 0 {
                eprintln!("  {}", format!("✓ Copied {} files", copied).dimmed());
            }
        }
    }

    eprintln!("{}", "✓ 🌳 Worktree created!".green());
    Ok(id)
}

/// Get the current worktree ID from cwd (if in a worktree)
fn current_worktree_id(repo_root: &Path) -> Result<Option<String>> {
    let cwd = env::current_dir()?;
    let wt_dir = git::wt_dir(repo_root);

    // Check if cwd is under .git/wt/<id>/
    if let Ok(relative) = cwd.strip_prefix(&wt_dir) {
        // First component is the worktree ID
        if let Some(id) = relative.iter().next() {
            return Ok(Some(id.to_string_lossy().to_string()));
        }
    }

    Ok(None)
}

/// Summarize a list of files for display
/// Shows all root files + directory summaries
fn summarize_files(files: &[String]) -> Vec<String> {
    use std::collections::HashMap;

    let mut root_files: Vec<&str> = Vec::new();
    let mut dir_counts: HashMap<&str, usize> = HashMap::new();

    for file in files {
        if let Some(slash_pos) = file.find('/') {
            // File in a directory - count by top-level dir
            let dir = &file[..slash_pos];
            *dir_counts.entry(dir).or_insert(0) += 1;
        } else {
            // Root file - show individually
            root_files.push(file);
        }
    }

    let mut result = Vec::new();

    // Show root files first (sorted alphabetically)
    root_files.sort();
    for f in &root_files {
        result.push(f.to_string());
    }

    // Show directories sorted by count (largest first), limit to top 5
    let mut dirs: Vec<_> = dir_counts.iter().collect();
    dirs.sort_by(|a, b| b.1.cmp(a.1));

    for (dir, count) in dirs.iter().take(5) {
        result.push(format!("{}/ ({} files)", dir, count));
    }

    // If there are more directories
    if dirs.len() > 5 {
        result.push(format!("+ {} more directories", dirs.len() - 5));
    }

    result
}

/// Output worktree names for shell completion
pub fn complete(cmd: Option<&str>) -> Result<()> {
    let repo_root = match git::find_repo_root() {
        Ok(r) => r,
        Err(_) => return Ok(()), // Silently fail if not in a repo
    };

    let meta = match Meta::open(&repo_root) {
        Ok(m) => m,
        Err(_) => return Ok(()), // Silently fail if no metadata
    };

    // For 'rm', only show grove worktrees (can't remove main repo)
    // For everything else (go, path, first arg), include main branch too
    let include_main = cmd != Some("rm");

    if include_main {
        if let Ok(main_branch) = git::default_branch(&repo_root) {
            println!("{}", main_branch);
        }
    }

    // Output all grove worktree branch names
    for (_, info) in meta.all()? {
        println!("{}", info.branch);
    }

    Ok(())
}
