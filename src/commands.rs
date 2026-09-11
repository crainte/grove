use anyhow::{Result, bail};
use std::env;
use std::path::{Path, PathBuf};

use crate::config::{Config, HookContext, run_hooks};
use crate::git;
use crate::meta::Meta;
use crate::note;
use crate::porcelain;
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
                note!(
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
    // An interactive picker has no meaning for a machine consumer, and fzf
    // would fight over the terminal. Degrade to a listing so the caller can
    // choose and then invoke `go <name>` itself.
    if porcelain::enabled() {
        return list();
    }

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
        note!("{} 📂 Switched to worktree '{}'", "✓".green(), name.cyan());
        note!("  {}", repo_root.display().to_string().dimmed());
        emit_wt(&meta, None, &main_branch, &repo_root, true, true, None)?;
        shell::output_cd(&repo_root);
        return Ok(());
    }

    // Try to find existing worktree
    if let Some(id) = meta.find_by_branch_with_context(name, current_id.as_deref())? {
        let wt_path = meta.worktree_path(&id);
        if wt_path.exists() {
            note!("{} 📂 Switched to worktree '{}'", "✓".green(), name.cyan());
            note!("  {}", wt_path.display().to_string().dimmed());
            emit_wt(
                &meta,
                Some(&id),
                name,
                &wt_path,
                false,
                true,
                Some(&main_branch),
            )?;
            shell::output_cd(&wt_path);
            return Ok(());
        }
        // Worktree directory missing — fallback to default branch
        note!(
            "{} Worktree '{}' directory missing, switching to '{}'",
            "⚠".yellow(),
            name.yellow(),
            main_branch.cyan()
        );
        note!("  {}", repo_root.display().to_string().dimmed());
        emit_wt(&meta, None, &main_branch, &repo_root, true, true, None)?;
        shell::output_cd(&repo_root);
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

    // Run enter hooks after create (create already ran post-create)
    note!("{} 📂 Created worktree '{}'", "✓".green(), name.cyan());
    note!("  {}", wt_path.display().to_string().dimmed());

    emit_wt(
        &meta,
        Some(&id),
        name,
        &wt_path,
        false,
        true,
        Some(&main_branch),
    )?;
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

    note!("Created worktree '{}' at {}", name, wt_path.display());

    let default_branch = git::default_branch(&repo_root).unwrap_or_else(|_| "main".to_string());
    emit_wt(
        &meta,
        Some(&id),
        name,
        &wt_path,
        false,
        false,
        Some(&default_branch),
    )?;
    Ok(())
}

/// Remove worktree
pub fn rm(name: &str, force: bool) -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;
    let config = Config::load(&repo_root)?;

    // Find the worktree
    let id = meta
        .find_by_branch(name)?
        .ok_or_else(|| anyhow::anyhow!("Worktree '{}' not found", name))?;

    let wt_path = meta.worktree_path(&id);

    // Check if we're inside the worktree we're about to remove
    let cwd = std::env::current_dir().ok();
    let removing_current = cwd
        .as_ref()
        .map(|c| c.starts_with(&wt_path))
        .unwrap_or(false);

    // Check for uncommitted changes unless force
    let (modified, untracked) = git::worktree_status(&wt_path).unwrap_or((false, false));
    if !force && wt_path.exists() && (modified || untracked) {
        bail!(
            "Worktree '{}' has uncommitted changes. Use --force to remove anyway.",
            name
        );
    }

    // Build hook context
    let hook_ctx = HookContext {
        path: &wt_path,
        branch: name,
        id: &id,
        repo: &repo_root,
    };

    // Run pre-remove hooks (can abort)
    if !config.hooks.pre_remove.is_empty() {
        note!("  {}", "Running pre-remove hooks...".dimmed());
        run_hooks(&config.hooks.pre_remove, &hook_ctx)?;
    }

    // Children are silently promoted to top level rather than cascade-deleted,
    // so capture them before the parent row disappears.
    let orphaned = meta.children(&id)?;

    note!(
        "{}",
        format!("🗑️  Removing worktree '{}'...", name.cyan().bold()).yellow()
    );

    // Remove git worktree first (branch can't be deleted while worktree exists)
    if wt_path.exists() {
        git::worktree_remove(&repo_root, &wt_path, force)?;
    }

    // Delete the branch
    if git::branch_exists(&repo_root, name)? {
        git::branch_delete(&repo_root, name, force)?;
    }

    // Remove from metadata
    meta.remove_worktree(&id)?;

    note!(
        "{}",
        format!("✓ Removed worktree '{}'", name.cyan()).green()
    );

    porcelain::record(&["removed", &id, name, "explicit"]);
    for (child_id, child) in &orphaned {
        porcelain::record(&["orphaned", child_id, &child.branch]);
    }

    // If we were inside the removed worktree, cd to main repo
    if removing_current {
        shell::output_cd(&repo_root);
    }

    Ok(())
}

/// List worktrees
/// A single row in the worktree listing.
///
/// Collected once, then consumed by either the pretty renderer or the
/// porcelain emitter. Keeping collection separate matters because gathering a
/// row costs two `git` invocations (status + ahead/behind); the previous
/// version duplicated that work across three near-identical loops.
struct Entry {
    /// `None` for the primary worktree (the repo root), which has no grove id.
    id: Option<String>,
    branch: String,
    path: PathBuf,
    parent: Option<String>,
    is_primary: bool,
    is_current: bool,
    is_orphan: bool,
    status: WorktreeStatus,
    /// Ref that `ahead`/`behind` were measured against, if any.
    cmp_base: Option<String>,
    /// Nesting depth; 0 for top level.
    depth: usize,
    /// Whether this entry has children in the tree.
    has_children: bool,
    /// Whether this is the last sibling at its level (drives tree connectors).
    is_last: bool,
}

impl Entry {
    /// Flag characters for porcelain output. See SPEC.md.
    fn flags(&self) -> String {
        let mut flags = String::new();
        if self.is_primary {
            flags.push('p');
        }
        if self.is_current {
            flags.push('c');
        }
        if self.status.has_modified {
            flags.push('m');
        }
        if self.status.has_untracked {
            flags.push('u');
        }
        if !self.status.dir_exists {
            flags.push('x');
        }
        if self.is_orphan {
            flags.push('o');
        }
        if flags.is_empty() {
            porcelain::NONE.to_string()
        } else {
            flags
        }
    }
}

/// Gather status for one worktree.
fn collect_status(
    path: &Path,
    branch: &str,
    compare_to: Option<&str>,
    dir_exists: bool,
) -> (WorktreeStatus, Option<String>) {
    let (has_modified, has_untracked) = git::worktree_status(path).unwrap_or((false, false));
    let (ahead, behind, cmp_base) =
        git::ahead_behind_against(path, branch, compare_to).unwrap_or((0, 0, None));
    (
        WorktreeStatus {
            has_modified,
            has_untracked,
            ahead,
            behind,
            dir_exists,
        },
        cmp_base,
    )
}

/// Recursively collect a worktree's children into `out`.
fn collect_children(
    meta: &Meta,
    parent_id: &str,
    parent_branch: &str,
    current_id: Option<&str>,
    is_orphan: bool,
    depth: usize,
    out: &mut Vec<Entry>,
) -> Result<()> {
    let children = meta.children(parent_id)?;
    let count = children.len();

    for (i, (child_id, child_info)) in children.iter().enumerate() {
        let path = meta.worktree_path(child_id);
        let has_children = !meta.children(child_id)?.is_empty();
        // Children compare against their parent's branch, not the default
        // branch - a child is "ahead" relative to where it forked from.
        let (status, cmp_base) = collect_status(
            &path,
            &child_info.branch,
            Some(parent_branch),
            path.exists(),
        );

        out.push(Entry {
            id: Some(child_id.clone()),
            branch: child_info.branch.clone(),
            path,
            parent: Some(parent_id.to_string()),
            is_primary: false,
            is_current: current_id == Some(child_id.as_str()),
            is_orphan,
            status,
            cmp_base,
            depth,
            has_children,
            is_last: i == count - 1,
        });

        collect_children(
            meta,
            child_id,
            &child_info.branch,
            current_id,
            is_orphan,
            depth + 1,
            out,
        )?;
    }

    Ok(())
}

/// Collect every row of the listing, in display order.
///
/// Returns `(entries, orphan_start)` where `orphan_start` is the index at which
/// orphaned worktrees begin; the pretty renderer draws a visual break there.
fn collect_entries(
    repo_root: &Path,
    meta: &Meta,
    current_id: Option<&str>,
    default_branch: &str,
) -> Result<(Vec<Entry>, usize)> {
    let mut entries = Vec::new();

    let top_level = meta.top_level()?;
    let orphans = meta.orphans()?;

    // Primary worktree (repo root). It always exists and always compares
    // against its own upstream, if any.
    let (status, cmp_base) = collect_status(repo_root, default_branch, None, true);
    entries.push(Entry {
        id: None,
        branch: default_branch.to_string(),
        path: repo_root.to_path_buf(),
        parent: None,
        is_primary: true,
        is_current: current_id.is_none(),
        is_orphan: false,
        status,
        cmp_base,
        depth: 0,
        has_children: false, // main has no children in our model
        is_last: top_level.is_empty() && orphans.is_empty(),
    });

    let top_count = top_level.len();
    for (i, (id, info)) in top_level.iter().enumerate() {
        let path = meta.worktree_path(id);
        let has_children = !meta.children(id)?.is_empty();
        let (status, cmp_base) =
            collect_status(&path, &info.branch, Some(default_branch), path.exists());

        entries.push(Entry {
            id: Some(id.clone()),
            branch: info.branch.clone(),
            path,
            parent: None,
            is_primary: false,
            is_current: current_id == Some(id.as_str()),
            is_orphan: false,
            status,
            cmp_base,
            depth: 0,
            has_children,
            is_last: i == top_count - 1 && orphans.is_empty(),
        });

        collect_children(meta, id, &info.branch, current_id, false, 1, &mut entries)?;
    }

    let orphan_start = entries.len();

    // Orphans have no surviving parent to compare against, so they fall back to
    // upstream only.
    for (i, (id, info, depth)) in orphans.iter().enumerate() {
        let path = meta.worktree_path(id);
        let has_children = !meta.children(id)?.is_empty();
        let (status, cmp_base) = collect_status(&path, &info.branch, None, path.exists());

        entries.push(Entry {
            id: Some(id.clone()),
            branch: info.branch.clone(),
            path,
            // The dangling parent id is retained: it is what makes this an
            // orphan, and consumers may want to show the lost relationship.
            parent: meta.parent_of(id)?,
            is_primary: false,
            is_current: current_id == Some(id.as_str()),
            is_orphan: true,
            status,
            cmp_base,
            depth: depth.saturating_sub(1),
            has_children,
            is_last: i == orphans.len() - 1,
        });

        collect_children(
            meta,
            id,
            &info.branch,
            current_id,
            true,
            depth.saturating_sub(1) + 1,
            &mut entries,
        )?;
    }

    Ok((entries, orphan_start))
}

/// Emit a single `wt` record for a worktree acted on by a command, so callers
/// of `add`/`go` get the same fields they would from `list` without a second
/// invocation.
fn emit_wt(
    meta: &Meta,
    id: Option<&str>,
    branch: &str,
    path: &Path,
    is_primary: bool,
    is_current: bool,
    compare_to: Option<&str>,
) -> Result<()> {
    if !porcelain::enabled() {
        return Ok(());
    }

    let (status, cmp_base) = collect_status(path, branch, compare_to, path.exists());
    let parent = match id {
        Some(id) => meta.parent_of(id)?,
        None => None,
    };

    let entry = Entry {
        id: id.map(String::from),
        branch: branch.to_string(),
        path: path.to_path_buf(),
        parent,
        is_primary,
        is_current,
        is_orphan: false,
        status,
        cmp_base,
        depth: 0,
        has_children: false,
        is_last: true,
    };

    porcelain::record(&[
        "wt",
        entry.id.as_deref().unwrap_or(porcelain::NONE),
        &entry.branch,
        &entry.path.display().to_string(),
        entry.parent.as_deref().unwrap_or(porcelain::NONE),
        &entry.flags(),
        &entry.status.ahead.to_string(),
        &entry.status.behind.to_string(),
        entry.cmp_base.as_deref().unwrap_or(porcelain::NONE),
    ]);

    Ok(())
}

/// Emit a listing as porcelain records.
fn emit_listing(repo_root: &Path, default_branch: &str, entries: &[Entry]) {
    porcelain::record(&["repo", &repo_root.display().to_string(), default_branch]);

    for e in entries {
        porcelain::record(&[
            "wt",
            e.id.as_deref().unwrap_or(porcelain::NONE),
            &e.branch,
            &e.path.display().to_string(),
            e.parent.as_deref().unwrap_or(porcelain::NONE),
            &e.flags(),
            &e.status.ahead.to_string(),
            &e.status.behind.to_string(),
            e.cmp_base.as_deref().unwrap_or(porcelain::NONE),
        ]);
    }
}

/// List worktrees
pub fn list() -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    let meta = Meta::open(&repo_root)?;
    let current_id = current_worktree_id(&repo_root)?;
    let default_branch = git::default_branch(&repo_root).unwrap_or_else(|_| "main".to_string());

    let (entries, orphan_start) =
        collect_entries(&repo_root, &meta, current_id.as_deref(), &default_branch)?;

    if porcelain::enabled() {
        emit_listing(&repo_root, &default_branch, &entries);
        return Ok(());
    }

    // Header
    note!("{}", "🌳 Git Worktrees".bold());
    note!(
        "{}",
        "────────────────────────────────────────────────────────────────".bright_black()
    );
    note!();

    // Get repo name from path
    let repo_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo");

    note!(
        "📁 {} ({})",
        repo_name.bold().white(),
        repo_root.display().to_string().bright_black()
    );

    render_entries(&entries, orphan_start);

    // Footer suggestion
    note!();
    note!(
        "{}",
        "💡 Use 'grove go <name>' to switch, 'grove rm <name>' to remove".bright_blue()
    );

    Ok(())
}

/// Render collected entries as the human-facing tree.
///
/// Tree prefixes are rebuilt from each entry's depth and last-sibling flag. A
/// stack of "does this ancestor level still have siblings below it" decides
/// whether to draw a continuation bar.
fn render_entries(entries: &[Entry], orphan_start: usize) {
    use colored::Colorize;

    // continuation[d] == true means depth d still has siblings to come, so
    // deeper rows draw a `│` at that column.
    let mut continuation: Vec<bool> = Vec::new();

    for (i, e) in entries.iter().enumerate() {
        if i == orphan_start && orphan_start < entries.len() {
            // Visual break before orphans
            note!("{}", "┊".truecolor(120, 100, 140));
        }

        continuation.truncate(e.depth);
        let mut prefix = String::new();
        for has_more in &continuation {
            prefix.push_str(if *has_more { "│  " } else { "   " });
        }
        continuation.push(!e.is_last);

        print_worktree_entry(
            &e.branch,
            &e.path,
            e.is_current,
            e.is_last,
            e.has_children,
            &prefix,
            e.is_orphan,
            &e.status,
        );
    }
}

/// Status information for a worktree
struct WorktreeStatus {
    has_modified: bool,
    has_untracked: bool,
    ahead: u32,
    behind: u32,
    dir_exists: bool,
}

#[allow(clippy::too_many_arguments)]
fn print_worktree_entry(
    name: &str,
    path: &std::path::Path,
    is_current: bool,
    is_last: bool,
    has_children: bool,
    prefix: &str,
    is_orphan: bool,
    status: &WorktreeStatus,
) {
    use colored::Colorize;

    let connector = if is_last { "└─" } else { "├─" };
    let path_prefix = match (is_last, has_children) {
        (false, true) => "│  │  ",
        (false, false) => "│     ",
        (true, true) => "   │  ",
        (true, false) => "      ",
    };

    // Tree line colors: dimmer for orphans
    let tree_color = if is_orphan {
        (120, 100, 140)
    } else {
        (180, 160, 200)
    };

    // Marker: ● green if current, ✗ red if dir missing, ○ otherwise
    let marker = if !status.dir_exists {
        "✗".red().to_string()
    } else if is_current {
        "●".green().to_string()
    } else if is_orphan {
        "○".truecolor(150, 150, 150).to_string()
    } else {
        "○".bright_black().to_string()
    };

    let branch_name = if is_current {
        name.bold().cyan().to_string()
    } else if is_orphan {
        name.truecolor(180, 180, 180).to_string()
    } else {
        name.cyan().to_string()
    };

    let here_suffix = if is_current {
        " ← here".green().to_string()
    } else {
        String::new()
    };

    note!(
        "{}{} {} {}{}",
        prefix.truecolor(tree_color.0, tree_color.1, tree_color.2),
        connector.truecolor(tree_color.0, tree_color.1, tree_color.2),
        marker,
        branch_name,
        here_suffix
    );

    // Build status string: ↑N ↓M !? · path
    let mut status_parts = Vec::new();
    if status.ahead > 0 {
        status_parts.push(format!("↑{}", status.ahead).green().to_string());
    }
    if status.behind > 0 {
        status_parts.push(
            format!("↓{}", status.behind)
                .truecolor(255, 165, 0)
                .to_string(),
        ); // orange
    }
    // Dirty indicators: ! for modified (red), ? for untracked (cyan)
    let dirty_str = match (status.has_modified, status.has_untracked) {
        (true, true) => format!("{}{}", "!".red(), "?".cyan()),
        (true, false) => "!".red().to_string(),
        (false, true) => "?".cyan().to_string(),
        (false, false) => String::new(),
    };
    if !dirty_str.is_empty() {
        status_parts.push(dirty_str);
    }

    let status_str = if status_parts.is_empty() {
        String::new()
    } else {
        format!("{} · ", status_parts.join(" "))
    };

    note!(
        "{}{}{}{}",
        prefix.truecolor(tree_color.0, tree_color.1, tree_color.2),
        path_prefix.truecolor(tree_color.0, tree_color.1, tree_color.2),
        status_str,
        path.display().to_string().bright_black()
    );
}

/// Clean stale worktree references
pub fn prune() -> Result<()> {
    use colored::Colorize;

    let repo_root = git::find_repo_root()?;
    note!("{}", "🧹 Pruning stale worktree references...".yellow());
    git::worktree_prune(&repo_root)?;
    note!("{}", "✓ Pruned".green());
    porcelain::record(&["pruned"]);
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
    let report = meta.sync(&git_worktrees)?;

    if !report.imported.is_empty() || !report.removed.is_empty() {
        note!(
            "{}",
            format!(
                "✓ Synced: {} imported, {} removed",
                report.imported.len(),
                report.removed.len()
            )
            .green()
        );
    } else {
        note!("{}", "✓ Already in sync".green());
    }

    for (id, info) in &report.imported {
        porcelain::record(&["imported", id, &info.branch]);
    }
    for (id, branch) in &report.removed {
        porcelain::record(&["removed", id, branch, "stale"]);
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
        // Determine target: use branch's upstream if available, otherwise fall back to target
        let check_against =
            git::upstream_branch(&repo_root, &info.branch).unwrap_or_else(|| target.to_string());

        // Check if branch is merged
        if git::is_branch_merged(&repo_root, &info.branch, &check_against)? {
            let wt_path = meta.worktree_path(&id);

            // Check for uncommitted changes
            let (modified, untracked) = git::worktree_status(&wt_path).unwrap_or((false, false));
            if wt_path.exists() && (modified || untracked) {
                note!(
                    "{}",
                    format!("⚠ Skipping '{}': has uncommitted changes", info.branch).yellow()
                );
                porcelain::record(&["skipped", &id, &info.branch, "dirty"]);
                skipped += 1;
                continue;
            }

            // Track if this is the current worktree
            if current_id.as_deref() == Some(id.as_str()) {
                removed_current = true;
            }

            // Remove the worktree
            note!(
                "{}",
                format!(
                    "✓ Removing '{}' (merged into {})",
                    info.branch, check_against
                )
                .green()
            );

            if wt_path.exists() {
                git::worktree_remove(&repo_root, &wt_path, false)?;
            } else {
                // Directory is already gone - prune the stale git worktree entry
                // so we can delete the branch without "branch is used by worktree" error
                git::worktree_prune(&repo_root)?;
            }
            // Force delete: we've verified the branch is merged via tree comparison,
            // but git -d doesn't recognize squash merges as merged
            git::branch_delete(&repo_root, &info.branch, true)?;
            meta.remove(&id)?;
            porcelain::record(&[
                "removed",
                &id,
                &info.branch,
                &format!("merged:{}", check_against),
            ]);
            removed += 1;
        }
    }

    if removed == 0 && skipped == 0 {
        note!("{}", "✓ No merged worktrees to clean".green());
    } else {
        note!(
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

    // Change to main repo first (so clean() doesn't think we're in a worktree being removed)
    env::set_current_dir(&repo_root)?;

    // Fetch all remotes (updates tracking branches for merge detection)
    note!("{}", "⟳ Fetching all remotes...".cyan());
    git::fetch_all(&repo_root)?;
    porcelain::record(&["fetched"]);

    // Pull latest on main
    note!("{}", "⟳ Pulling latest...".cyan());
    git::pull(&repo_root)?;
    porcelain::record(&["pulled"]);

    // Clean merged worktrees
    note!("{}", "⟳ Cleaning merged worktrees...".cyan());
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
        note!("{}", "📥 Pulling ignored files from main...".cyan());
    } else {
        note!("{} {}", "📥 Pulling from main:".cyan(), paths.join(", "));
    }

    let (report, files) = crate::copyfiles::sync_ignored(src, dest, paths)?;

    if !files.is_empty() {
        for line in summarize_files(&files) {
            note!("  {}", line.dimmed());
        }
    }

    note!("{}", format!("✓ Pulled {} file(s)", report.copied).green());
    emit_copy_report(&report);
    warn_copy_failures(&report);
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
        note!("{}", "📤 Pushing ignored files to main...".cyan());
    } else {
        note!("{} {}", "📤 Pushing to main:".cyan(), paths.join(", "));
    }

    let (report, files) = crate::copyfiles::sync_ignored(src, dest, paths)?;

    if !files.is_empty() {
        for line in summarize_files(&files) {
            note!("  {}", line.dimmed());
        }
    }

    note!("{}", format!("✓ Pushed {} file(s)", report.copied).green());
    emit_copy_report(&report);
    warn_copy_failures(&report);
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
    if porcelain::enabled() {
        porcelain::record(&["path", &wt_path.display().to_string()]);
    } else {
        println!("{}", wt_path.display());
    }
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
    use colored::Colorize;

    // Load config
    let config = Config::load(repo_root)?;

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

    // Build hook context (path doesn't exist yet for pre-create)
    let hook_ctx = HookContext {
        path: &wt_path,
        branch,
        id: &id,
        repo: repo_root,
    };

    // Create the git worktree
    note!(
        "{}",
        format!("🌱 Creating worktree '{}'...", branch.cyan().bold()).yellow()
    );
    note!("  {}", format!("Base: {}", base_branch).dimmed());

    if let Err(e) = git::worktree_add(repo_root, &wt_path, branch, &base_branch) {
        // Rollback metadata on failure
        meta.remove_worktree(&id)?;
        return Err(e);
    }

    // Ensure git hooks work in worktrees
    if git::ensure_hooks_path(repo_root)? {
        note!(
            "  {}",
            "Configured core.hooksPath for worktree hooks".dimmed()
        );
    }

    // Copy ignored files from the primary worktree.
    // `copyignored` takes everything and so supersedes the `copy` patterns.
    // Both draw from `git ls-files --others --ignored`, which never lists
    // `.git`, so neither can reach outside the ignored set.
    let files = if config.copy_ignored {
        crate::copyfiles::list_ignored_files(repo_root)?
    } else if !config.copy.is_empty() {
        crate::copyfiles::list_matching_files(repo_root, &config.copy)?
    } else {
        Vec::new()
    };

    if !files.is_empty() {
        let summary = summarize_files(&files);
        note!("  {}", format!("Copying {} files...", files.len()).dimmed());
        for line in &summary {
            note!("    {}", line.dimmed());
        }

        let report = crate::copyfiles::copy_files_parallel(&files, repo_root, &wt_path)?;
        if report.copied > 0 {
            note!("  {}", format!("✓ Copied {} files", report.copied).dimmed());
        }
        warn_copy_failures(&report);
    }

    // Run post-create hooks
    if !config.hooks.post_create.is_empty() {
        note!("  {}", "Running post-create hooks...".dimmed());
        run_hooks(&config.hooks.post_create, &hook_ctx)?;
    }

    note!("{}", "✓ 🌳 Worktree created!".green());
    porcelain::record(&["created", &id, branch, &wt_path.display().to_string()]);
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

/// Emit copy results as porcelain records.
fn emit_copy_report(report: &crate::copyfiles::CopyReport) {
    porcelain::record(&["copied", &report.copied.to_string()]);
    for (path, err) in &report.failed {
        porcelain::record(&["copyfail", path, &err.to_string()]);
    }
}

/// Warn about files that could not be copied
///
/// A partial copy is recoverable - the worktree is still usable - so this warns
/// rather than failing the command.
fn warn_copy_failures(report: &crate::copyfiles::CopyReport) {
    use colored::Colorize;

    if report.failed.is_empty() {
        return;
    }

    note!(
        "{}",
        format!("⚠ {} file(s) could not be copied", report.failed.len()).yellow()
    );

    for (path, err) in report.failed.iter().take(5) {
        note!("    {}", format!("{}: {}", path, err).dimmed());
    }

    if report.failed.len() > 5 {
        note!(
            "    {}",
            format!("+ {} more", report.failed.len() - 5).dimmed()
        );
    }
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

    if include_main && let Ok(main_branch) = git::default_branch(&repo_root) {
        println!("{}", main_branch);
    }

    // Output all grove worktree branch names
    for (_, info) in meta.all()? {
        println!("{}", info.branch);
    }

    Ok(())
}
