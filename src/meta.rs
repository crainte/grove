use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

/// Information about a single worktree
#[allow(dead_code)] // Fields used in tests and SQL relationships
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub branch: String,
    pub parent: Option<String>,
    pub created: DateTime<Utc>,
}

/// Metadata database for worktrees in a repository
pub struct Meta {
    conn: Connection,
    repo_root: PathBuf,
}

impl Meta {
    /// Open or create the metadata database
    pub fn open(repo_root: &Path) -> Result<Self> {
        let db_path = Self::db_path(repo_root);

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

        // Enable WAL mode for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Disable FK enforcement - we handle orphan relationships in code
        conn.pragma_update(None, "foreign_keys", "OFF")?;

        // Initialize schema
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS worktrees (
                id TEXT PRIMARY KEY,
                branch TEXT NOT NULL UNIQUE,
                parent TEXT,
                created TEXT NOT NULL
            );
            
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            );
            
            INSERT OR IGNORE INTO meta (key, value) VALUES ('next_id', 1);",
        )?;

        Ok(Self {
            conn,
            repo_root: repo_root.to_path_buf(),
        })
    }

    /// Get the path to the database file
    fn db_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".git/wt/grove.db")
    }

    /// Generate the next worktree ID (base36) atomically
    pub fn next_id(&self) -> Result<String> {
        let id: u32 = self.conn.query_row(
            "UPDATE meta SET value = value + 1 WHERE key = 'next_id' RETURNING value - 1",
            [],
            |row| row.get(0),
        )?;
        Ok(base36_encode(id))
    }

    /// Add a new worktree atomically, returns the assigned ID
    pub fn add_worktree(&self, branch: &str, parent: Option<&str>) -> Result<String> {
        let id = self.next_id()?;
        let created = Utc::now().to_rfc3339();

        self.conn
            .execute(
                "INSERT INTO worktrees (id, branch, parent, created) VALUES (?1, ?2, ?3, ?4)",
                params![id, branch, parent, created],
            )
            .with_context(|| format!("Failed to add worktree '{}'", branch))?;

        Ok(id)
    }

    /// Remove a worktree by ID (children keep their parent reference, becoming orphans)
    pub fn remove_worktree(&self, id: &str) -> Result<Option<WorktreeInfo>> {
        // First get the info
        let info = self.get_worktree(id)?;

        if info.is_some() {
            // Just delete - children keep their parent ID, becoming "orphans"
            // This preserves their depth in the tree display
            self.conn
                .execute("DELETE FROM worktrees WHERE id = ?1", params![id])?;
        }

        Ok(info)
    }

    /// Get worktree info by ID
    pub fn get_worktree(&self, id: &str) -> Result<Option<WorktreeInfo>> {
        let mut stmt = self
            .conn
            .prepare("SELECT branch, parent, created FROM worktrees WHERE id = ?1")?;

        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let branch: String = row.get(0)?;
            let parent: Option<String> = row.get(1)?;
            let created_str: String = row.get(2)?;
            let created = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Some(WorktreeInfo {
                branch,
                parent,
                created,
            }))
        } else {
            Ok(None)
        }
    }

    /// Find worktree ID by branch name
    pub fn find_by_branch(&self, branch: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM worktrees WHERE branch = ?1")?;

        let mut rows = stmt.query(params![branch])?;

        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Find worktree ID by branch name, preferring children of given parent
    pub fn find_by_branch_with_context(
        &self,
        branch: &str,
        parent_id: Option<&str>,
    ) -> Result<Option<String>> {
        // First try to find a child of the current worktree
        if let Some(pid) = parent_id {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM worktrees WHERE branch = ?1 AND parent = ?2")?;

            let mut rows = stmt.query(params![branch, pid])?;

            if let Some(row) = rows.next()? {
                return Ok(Some(row.get(0)?));
            }
        }

        // Fall back to any match
        self.find_by_branch(branch)
    }

    /// Get worktree path
    pub fn worktree_path(&self, id: &str) -> PathBuf {
        self.repo_root.join(".git/wt").join(id)
    }

    /// Get children of a worktree, sorted by branch name
    pub fn children(&self, parent_id: &str) -> Result<Vec<(String, WorktreeInfo)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, branch, parent, created FROM worktrees 
             WHERE parent = ?1 ORDER BY branch",
        )?;

        let rows = stmt.query_map(params![parent_id], |row| {
            let id: String = row.get(0)?;
            let branch: String = row.get(1)?;
            let parent: Option<String> = row.get(2)?;
            let created_str: String = row.get(3)?;
            Ok((id, branch, parent, created_str))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, branch, parent, created_str) = row?;
            let created = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            result.push((
                id,
                WorktreeInfo {
                    branch,
                    parent,
                    created,
                },
            ));
        }

        Ok(result)
    }

    /// Get top-level worktrees (no parent), sorted by branch name
    pub fn top_level(&self) -> Result<Vec<(String, WorktreeInfo)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, branch, parent, created FROM worktrees 
             WHERE parent IS NULL ORDER BY branch",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let branch: String = row.get(1)?;
            let parent: Option<String> = row.get(2)?;
            let created_str: String = row.get(3)?;
            Ok((id, branch, parent, created_str))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, branch, parent, created_str) = row?;
            let created = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            result.push((
                id,
                WorktreeInfo {
                    branch,
                    parent,
                    created,
                },
            ));
        }

        Ok(result)
    }

    /// Get all worktrees
    pub fn all(&self) -> Result<Vec<(String, WorktreeInfo)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, branch, parent, created FROM worktrees ORDER BY branch")?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let branch: String = row.get(1)?;
            let parent: Option<String> = row.get(2)?;
            let created_str: String = row.get(3)?;
            Ok((id, branch, parent, created_str))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, branch, parent, created_str) = row?;
            let created = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            result.push((
                id,
                WorktreeInfo {
                    branch,
                    parent,
                    created,
                },
            ));
        }

        Ok(result)
    }

    /// Get orphaned worktrees (parent ID set but parent doesn't exist)
    /// Returns tuples of (id, info, depth) where depth is how many missing ancestors
    pub fn orphans(&self) -> Result<Vec<(String, WorktreeInfo, usize)>> {
        // Get all worktrees with a parent set
        let mut stmt = self.conn.prepare(
            "SELECT w.id, w.branch, w.parent, w.created
             FROM worktrees w
             WHERE w.parent IS NOT NULL
             AND NOT EXISTS (SELECT 1 FROM worktrees p WHERE p.id = w.parent)
             ORDER BY w.branch",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let branch: String = row.get(1)?;
            let parent: Option<String> = row.get(2)?;
            let created_str: String = row.get(3)?;
            Ok((id, branch, parent, created_str))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, branch, parent, created_str) = row?;
            let created = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            // Calculate depth by counting missing ancestors
            let depth = self.orphan_depth(&parent)?;
            result.push((
                id,
                WorktreeInfo {
                    branch,
                    parent,
                    created,
                },
                depth,
            ));
        }

        Ok(result)
    }

    /// Calculate how deep an orphan is (count missing ancestors + 1)
    fn orphan_depth(&self, parent_id: &Option<String>) -> Result<usize> {
        // For now, just return 1 for any orphan
        // Deeper orphan chain detection would require tracking deleted parent info
        if parent_id.is_some() { Ok(1) } else { Ok(0) }
    }

    /// Remove a worktree by ID (simpler version for clean)
    pub fn remove(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM worktrees WHERE id = ?", params![id])?;
        Ok(())
    }

    /// Sync database with git worktrees
    /// Returns (imported, removed) counts
    pub fn sync(&self, git_worktrees: &[crate::git::GitWorktree]) -> Result<(usize, usize)> {
        let wt_dir = self.repo_root.join(".git/wt");
        let mut imported = 0;
        let mut removed = 0;

        // Import worktrees that exist in git but not in our database
        for wt in git_worktrees {
            // Skip if not under our .git/wt/ directory
            if !wt.path.starts_with(&wt_dir) {
                continue;
            }

            // Extract ID from path (last component)
            let id = match wt.path.file_name().and_then(|s| s.to_str()) {
                Some(id) => id,
                None => continue,
            };

            // Skip if already in database
            if self.get_worktree(id)?.is_some() {
                continue;
            }

            // Get branch name
            let branch = match &wt.branch {
                Some(b) => b.clone(),
                None => continue, // Skip detached HEAD worktrees
            };

            // Import it
            let created = chrono::Utc::now().to_rfc3339();
            self.conn.execute(
                "INSERT OR IGNORE INTO worktrees (id, branch, parent, created) VALUES (?1, ?2, NULL, ?3)",
                rusqlite::params![id, branch, created],
            )?;

            // Update next_id if needed
            if let Ok(id_num) = u32::from_str_radix(id, 36) {
                self.conn.execute(
                    "UPDATE meta SET value = MAX(value, ?1 + 1) WHERE key = 'next_id'",
                    rusqlite::params![id_num],
                )?;
            }

            imported += 1;
        }

        // Remove entries that no longer exist in git
        let our_worktrees: Vec<String> = {
            let mut stmt = self.conn.prepare("SELECT id FROM worktrees")?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        for id in our_worktrees {
            let our_path = wt_dir.join(&id);
            let exists_in_git = git_worktrees.iter().any(|wt| wt.path == our_path);

            if !exists_in_git && !our_path.exists() {
                self.conn
                    .execute("DELETE FROM worktrees WHERE id = ?1", rusqlite::params![id])?;
                removed += 1;
            }
        }

        Ok((imported, removed))
    }
}

/// Encode a number as base36 (0-9, a-z)
fn base36_encode(mut n: u32) -> String {
    if n == 0 {
        return "0".to_string();
    }
    const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut result = Vec::new();
    while n > 0 {
        result.push(CHARS[(n % 36) as usize]);
        n /= 36;
    }
    result.reverse();
    String::from_utf8(result).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Meta) {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".git/wt")).unwrap();
        let meta = Meta::open(dir.path()).unwrap();
        (dir, meta)
    }

    #[test]
    fn test_next_id_increments() {
        let (_dir, meta) = setup();
        assert_eq!(meta.next_id().unwrap(), "1");
        assert_eq!(meta.next_id().unwrap(), "2");
        assert_eq!(meta.next_id().unwrap(), "3");
    }

    #[test]
    fn test_add_worktree() {
        let (_dir, meta) = setup();
        let id = meta.add_worktree("feature/test", None).unwrap();
        assert_eq!(id, "1");

        let info = meta.get_worktree("1").unwrap().unwrap();
        assert_eq!(info.branch, "feature/test");
        assert!(info.parent.is_none());
    }

    #[test]
    fn test_add_worktree_with_parent() {
        let (_dir, meta) = setup();
        let parent_id = meta.add_worktree("parent", None).unwrap();
        let child_id = meta.add_worktree("child", Some(&parent_id)).unwrap();

        let info = meta.get_worktree(&child_id).unwrap().unwrap();
        assert_eq!(info.parent, Some(parent_id));
    }

    #[test]
    fn test_find_by_branch() {
        let (_dir, meta) = setup();
        meta.add_worktree("feature/test", None).unwrap();

        assert_eq!(
            meta.find_by_branch("feature/test").unwrap(),
            Some("1".to_string())
        );
        assert_eq!(meta.find_by_branch("nonexistent").unwrap(), None);
    }

    #[test]
    fn test_remove_worktree() {
        let (_dir, meta) = setup();
        meta.add_worktree("test", None).unwrap();

        let removed = meta.remove_worktree("1").unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().branch, "test");

        assert!(meta.get_worktree("1").unwrap().is_none());
    }

    #[test]
    fn test_top_level() {
        let (_dir, meta) = setup();
        meta.add_worktree("beta", None).unwrap();
        meta.add_worktree("alpha", None).unwrap();

        let top = meta.top_level().unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].1.branch, "alpha"); // sorted
        assert_eq!(top[1].1.branch, "beta");
    }

    #[test]
    fn test_children() {
        let (_dir, meta) = setup();
        let parent = meta.add_worktree("parent", None).unwrap();
        meta.add_worktree("child-b", Some(&parent)).unwrap();
        meta.add_worktree("child-a", Some(&parent)).unwrap();

        let children = meta.children(&parent).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].1.branch, "child-a"); // sorted
        assert_eq!(children[1].1.branch, "child-b");
    }

    #[test]
    fn test_base36_encode() {
        assert_eq!(base36_encode(0), "0");
        assert_eq!(base36_encode(9), "9");
        assert_eq!(base36_encode(10), "a");
        assert_eq!(base36_encode(35), "z");
        assert_eq!(base36_encode(36), "10");
        assert_eq!(base36_encode(4329), "3c9");
    }

    #[test]
    fn test_find_by_branch_with_context() {
        let (_dir, meta) = setup();
        let parent = meta.add_worktree("parent", None).unwrap();
        let child_id = meta.add_worktree("child", Some(&parent)).unwrap();

        // When we have context (parent), we should find the child
        let found = meta
            .find_by_branch_with_context("child", Some(&parent))
            .unwrap();
        assert_eq!(found, Some(child_id.clone()));

        // Without context, we still find it
        let found = meta.find_by_branch_with_context("child", None).unwrap();
        assert_eq!(found, Some(child_id));
    }
}
