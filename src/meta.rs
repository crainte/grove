use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

/// Information about a single worktree
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub branch: String,
}

/// Outcome of reconciling the database against git's own worktree list.
pub struct SyncReport {
    /// Worktrees present in git but missing from the database.
    pub imported: Vec<(String, WorktreeInfo)>,
    /// (id, branch) pairs dropped from the database because git no longer
    /// knows about them and the directory is gone.
    pub removed: Vec<(String, String)>,
    /// (id, old, new) branch names updated because the branch was renamed
    /// outside grove.
    pub renamed: Vec<(String, String, String)>,
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

        Self::migrate(&conn)?;

        Ok(Self {
            conn,
            repo_root: repo_root.to_path_buf(),
        })
    }

    /// Bring an older database up to the current schema.
    ///
    /// Versions are tracked with `PRAGMA user_version`. The check and the
    /// `ALTER`s share one immediate transaction so two grove processes opening
    /// a v0 database at once cannot both try to add the same column.
    fn migrate(conn: &Connection) -> Result<()> {
        const CURRENT: i64 = 1;

        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version >= CURRENT {
            return Ok(());
        }

        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
            if version < 1 {
                // NULL path means the legacy location, `.git/wt/<id>`.
                conn.execute_batch(
                    "ALTER TABLE worktrees ADD COLUMN path TEXT;
                     ALTER TABLE worktrees ADD COLUMN base TEXT;",
                )?;
            }
            conn.pragma_update(None, "user_version", CURRENT)?;
            Ok(())
        })();

        match result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                conn.execute_batch("ROLLBACK").ok();
                return Err(e);
            }
        }
        Ok(())
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

    /// Add a new worktree atomically, returns the assigned ID.
    ///
    /// `base` is the branch the worktree was created from. `dir` is the
    /// directory that holds worktrees; the worktree lives at `dir/<id>`.
    /// `None` stores no path, which means the legacy `.git/wt/<id>`.
    pub fn add_worktree(
        &self,
        branch: &str,
        parent: Option<&str>,
        base: Option<&str>,
        dir: Option<&Path>,
    ) -> Result<String> {
        let id = self.next_id()?;
        let created = Utc::now().to_rfc3339();
        let path = dir.map(|d| d.join(&id).to_string_lossy().to_string());

        self.conn
            .execute(
                "INSERT INTO worktrees (id, branch, parent, created, path, base)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, branch, parent, created, path, base],
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
            .prepare("SELECT branch FROM worktrees WHERE id = ?1")?;

        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let branch: String = row.get(0)?;
            Ok(Some(WorktreeInfo { branch }))
        } else {
            Ok(None)
        }
    }

    /// Get a worktree's parent ID.
    ///
    /// The parent may no longer exist - that is precisely what makes a worktree
    /// an orphan - so this reports the stored reference without validating it.
    pub fn parent_of(&self, id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT parent FROM worktrees WHERE id = ?1")?;

        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
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

    /// Get worktree path: the stored path, or `.git/wt/<id>` for rows created
    /// before paths were recorded (and for ids not in the database).
    pub fn worktree_path(&self, id: &str) -> PathBuf {
        let stored: Option<String> = self
            .conn
            .query_row(
                "SELECT path FROM worktrees WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        match stored {
            Some(p) => PathBuf::from(p),
            None => self.legacy_path(id),
        }
    }

    fn legacy_path(&self, id: &str) -> PathBuf {
        self.repo_root.join(".git/wt").join(id)
    }

    /// The branch a worktree was created from, if it was recorded.
    pub fn base_of(&self, id: &str) -> Result<Option<String>> {
        let base = self
            .conn
            .query_row(
                "SELECT base FROM worktrees WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(base.flatten())
    }

    /// Find the worktree containing `path`. The deepest match wins, so a
    /// worktree stored inside another directory is never mistaken for it.
    pub fn find_by_path(&self, path: &Path) -> Result<Option<String>> {
        let mut best: Option<(String, usize)> = None;
        for (id, _) in self.all()? {
            let wt = self.worktree_path(&id);
            if path.starts_with(&wt) {
                let depth = wt.components().count();
                if best.as_ref().is_none_or(|(_, d)| depth > *d) {
                    best = Some((id, depth));
                }
            }
        }
        Ok(best.map(|(id, _)| id))
    }

    /// Get children of a worktree, sorted by branch name
    pub fn children(&self, parent_id: &str) -> Result<Vec<(String, WorktreeInfo)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, branch FROM worktrees 
             WHERE parent = ?1 ORDER BY branch",
        )?;

        let rows = stmt.query_map(params![parent_id], |row| {
            let id: String = row.get(0)?;
            let branch: String = row.get(1)?;
            Ok((id, branch))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, branch) = row?;
            result.push((id, WorktreeInfo { branch }));
        }

        Ok(result)
    }

    /// Get top-level worktrees (no parent), sorted by branch name
    pub fn top_level(&self) -> Result<Vec<(String, WorktreeInfo)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, branch FROM worktrees 
             WHERE parent IS NULL ORDER BY branch",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let branch: String = row.get(1)?;
            Ok((id, branch))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, branch) = row?;
            result.push((id, WorktreeInfo { branch }));
        }

        Ok(result)
    }

    /// Get all worktrees
    pub fn all(&self) -> Result<Vec<(String, WorktreeInfo)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, branch FROM worktrees ORDER BY branch")?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let branch: String = row.get(1)?;
            Ok((id, branch))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, branch) = row?;
            result.push((id, WorktreeInfo { branch }));
        }

        Ok(result)
    }

    /// Get orphaned worktrees (parent ID set but parent doesn't exist)
    /// Returns tuples of (id, info, depth) where depth is how many missing ancestors
    pub fn orphans(&self) -> Result<Vec<(String, WorktreeInfo, usize)>> {
        // Get all worktrees with a parent set but parent doesn't exist
        let mut stmt = self.conn.prepare(
            "SELECT w.id, w.branch, w.parent
             FROM worktrees w
             WHERE w.parent IS NOT NULL
             AND NOT EXISTS (SELECT 1 FROM worktrees p WHERE p.id = w.parent)
             ORDER BY w.branch",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let branch: String = row.get(1)?;
            let parent: Option<String> = row.get(2)?;
            Ok((id, branch, parent))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, branch, parent) = row?;
            // Calculate depth by counting missing ancestors
            let depth = self.orphan_depth(&parent)?;
            result.push((id, WorktreeInfo { branch }, depth));
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

    /// Sync database with git worktrees.
    ///
    /// `dirs` are the directories grove stores worktrees in; git worktrees
    /// directly inside one of them (or the legacy `.git/wt`) are imported.
    /// Returns the worktrees that were imported and the ids that were dropped,
    /// rather than bare counts, so callers can report specifics.
    pub fn sync(
        &self,
        git_worktrees: &[crate::git::GitWorktree],
        dirs: &[PathBuf],
    ) -> Result<SyncReport> {
        let legacy_dir = self.repo_root.join(".git/wt");
        let mut imported = Vec::new();
        let renamed = self.refresh_branches(git_worktrees)?;

        // Import worktrees that exist in git but not in our database
        for wt in git_worktrees {
            let Some(parent) = wt.path.parent() else {
                continue;
            };
            if parent != legacy_dir && !dirs.iter().any(|d| d == parent) {
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
            let branch_for_record = branch.clone();

            // Legacy rows keep a NULL path so they resolve as before
            let path = (parent != legacy_dir).then(|| wt.path.to_string_lossy().to_string());

            // Import it
            let created = chrono::Utc::now().to_rfc3339();
            self.conn.execute(
                "INSERT OR IGNORE INTO worktrees (id, branch, parent, created, path)
                 VALUES (?1, ?2, NULL, ?3, ?4)",
                rusqlite::params![id, branch, created, path],
            )?;

            // Update next_id if needed
            if let Ok(id_num) = u32::from_str_radix(id, 36) {
                self.conn.execute(
                    "UPDATE meta SET value = MAX(value, ?1 + 1) WHERE key = 'next_id'",
                    rusqlite::params![id_num],
                )?;
            }

            imported.push((
                id.to_string(),
                WorktreeInfo {
                    branch: branch_for_record,
                },
            ));
        }

        let removed = self.remove_stale(git_worktrees)?;
        Ok(SyncReport {
            imported,
            removed,
            renamed,
        })
    }

    /// Update stored branch names to what git has checked out in each
    /// worktree. Branches renamed with `git branch -m` otherwise leave stale
    /// names behind, which every git call made with them then fails on.
    /// Returns the (id, old, new) triples updated.
    pub fn refresh_branches(
        &self,
        git_worktrees: &[crate::git::GitWorktree],
    ) -> Result<Vec<(String, String, String)>> {
        let mut renamed = Vec::new();
        for (id, info) in self.all()? {
            let path = self.worktree_path(&id);
            let Some(live) = git_worktrees
                .iter()
                .find(|wt| wt.path == path)
                .and_then(|wt| wt.branch.as_ref())
            else {
                continue;
            };
            if *live == info.branch {
                continue;
            }
            // OR IGNORE: another row may still hold the name (e.g. two
            // branches swapped); it is corrected on the next refresh.
            let changed = self.conn.execute(
                "UPDATE OR IGNORE worktrees SET branch = ?1 WHERE id = ?2",
                params![live, id],
            )?;
            if changed > 0 {
                renamed.push((id, info.branch, live.clone()));
            }
        }
        Ok(renamed)
    }

    /// Drop rows whose directory is gone and which git no longer lists as a
    /// worktree. Returns the (id, branch) pairs removed.
    pub fn remove_stale(
        &self,
        git_worktrees: &[crate::git::GitWorktree],
    ) -> Result<Vec<(String, String)>> {
        let mut removed = Vec::new();
        for (id, info) in self.all()? {
            let our_path = self.worktree_path(&id);
            let exists_in_git = git_worktrees.iter().any(|wt| wt.path == our_path);

            if !exists_in_git && !our_path.exists() {
                self.conn
                    .execute("DELETE FROM worktrees WHERE id = ?1", params![id])?;
                removed.push((id, info.branch));
            }
        }
        Ok(removed)
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
        let id = meta.add_worktree("feature/test", None, None, None).unwrap();
        assert_eq!(id, "1");

        let info = meta.get_worktree("1").unwrap().unwrap();
        assert_eq!(info.branch, "feature/test");
    }

    #[test]
    fn test_add_worktree_with_parent() {
        let (_dir, meta) = setup();
        let parent_id = meta.add_worktree("parent", None, None, None).unwrap();
        let child_id = meta
            .add_worktree("child", Some(&parent_id), None, None)
            .unwrap();

        // Verify parent-child relationship via children()
        let children = meta.children(&parent_id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].0, child_id);
        assert_eq!(children[0].1.branch, "child");
    }

    #[test]
    fn test_find_by_branch() {
        let (_dir, meta) = setup();
        meta.add_worktree("feature/test", None, None, None).unwrap();

        assert_eq!(
            meta.find_by_branch("feature/test").unwrap(),
            Some("1".to_string())
        );
        assert_eq!(meta.find_by_branch("nonexistent").unwrap(), None);
    }

    #[test]
    fn test_remove_worktree() {
        let (_dir, meta) = setup();
        meta.add_worktree("test", None, None, None).unwrap();

        let removed = meta.remove_worktree("1").unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().branch, "test");

        assert!(meta.get_worktree("1").unwrap().is_none());
    }

    #[test]
    fn test_top_level() {
        let (_dir, meta) = setup();
        meta.add_worktree("beta", None, None, None).unwrap();
        meta.add_worktree("alpha", None, None, None).unwrap();

        let top = meta.top_level().unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].1.branch, "alpha"); // sorted
        assert_eq!(top[1].1.branch, "beta");
    }

    #[test]
    fn test_children() {
        let (_dir, meta) = setup();
        let parent = meta.add_worktree("parent", None, None, None).unwrap();
        meta.add_worktree("child-b", Some(&parent), None, None)
            .unwrap();
        meta.add_worktree("child-a", Some(&parent), None, None)
            .unwrap();

        let children = meta.children(&parent).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].1.branch, "child-a"); // sorted
        assert_eq!(children[1].1.branch, "child-b");
    }

    #[test]
    fn test_new_row_uses_stored_path_and_base() {
        let (dir, meta) = setup();
        let wt_dir = dir.path().join(".wt");
        let id = meta
            .add_worktree("feat", None, Some("develop"), Some(&wt_dir))
            .unwrap();
        assert_eq!(meta.worktree_path(&id), wt_dir.join(&id));
        assert_eq!(meta.base_of(&id).unwrap(), Some("develop".to_string()));
    }

    #[test]
    fn test_legacy_row_falls_back_to_git_wt() {
        let (dir, meta) = setup();
        let id = meta.add_worktree("feat", None, None, None).unwrap();
        assert_eq!(
            meta.worktree_path(&id),
            dir.path().join(".git/wt").join(&id)
        );
        assert_eq!(meta.base_of(&id).unwrap(), None);
    }

    #[test]
    fn test_migrates_v0_schema_idempotently() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".git/wt")).unwrap();
        {
            let conn = Connection::open(dir.path().join(".git/wt/grove.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE worktrees (id TEXT PRIMARY KEY, branch TEXT NOT NULL UNIQUE,
                     parent TEXT, created TEXT NOT NULL);
                 CREATE TABLE meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
                 INSERT INTO meta VALUES ('next_id', 2);
                 INSERT INTO worktrees VALUES ('1', 'old', NULL, 'now');",
            )
            .unwrap();
        }
        drop(Meta::open(dir.path()).unwrap());
        let meta = Meta::open(dir.path()).unwrap();
        assert_eq!(meta.worktree_path("1"), dir.path().join(".git/wt/1"));
        assert_eq!(meta.base_of("1").unwrap(), None);
        let id = meta
            .add_worktree("new", None, Some("main"), Some(&dir.path().join(".wt")))
            .unwrap();
        assert_eq!(id, "2");
        assert_eq!(meta.worktree_path(&id), dir.path().join(".wt/2"));
    }

    #[test]
    fn test_find_by_path_prefers_longest_match() {
        let (dir, meta) = setup();
        let wt_dir = dir.path().join(".wt");
        let a = meta.add_worktree("a", None, None, Some(&wt_dir)).unwrap();
        let legacy = meta.add_worktree("b", None, None, None).unwrap();
        assert_eq!(
            meta.find_by_path(&wt_dir.join(&a).join("src/deep"))
                .unwrap(),
            Some(a)
        );
        assert_eq!(
            meta.find_by_path(&dir.path().join(".git/wt").join(&legacy))
                .unwrap(),
            Some(legacy)
        );
        assert_eq!(meta.find_by_path(dir.path()).unwrap(), None);
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
        let parent = meta.add_worktree("parent", None, None, None).unwrap();
        let child_id = meta
            .add_worktree("child", Some(&parent), None, None)
            .unwrap();

        // When we have context (parent), we should find the child
        let found = meta
            .find_by_branch_with_context("child", Some(&parent))
            .unwrap();
        assert_eq!(found, Some(child_id.clone()));

        // Without context, we still find it
        let found = meta.find_by_branch_with_context("child", None).unwrap();
        assert_eq!(found, Some(child_id));
    }

    #[test]
    fn test_refresh_branches_follows_renames() {
        let (_dir, meta) = setup();
        let renamed = meta.add_worktree("old", None, None, None).unwrap();
        let same = meta.add_worktree("same", None, None, None).unwrap();
        let gone = meta.add_worktree("gone", None, None, None).unwrap();
        let worktrees = vec![
            crate::git::GitWorktree {
                path: meta.worktree_path(&renamed),
                branch: Some("prefix/old".into()),
            },
            crate::git::GitWorktree {
                path: meta.worktree_path(&same),
                branch: Some("same".into()),
            },
        ];

        let changes = meta.refresh_branches(&worktrees).unwrap();

        assert_eq!(
            changes,
            vec![(renamed.clone(), "old".to_string(), "prefix/old".to_string())]
        );
        assert_eq!(
            meta.get_worktree(&renamed).unwrap().unwrap().branch,
            "prefix/old"
        );
        assert_eq!(meta.get_worktree(&same).unwrap().unwrap().branch, "same");
        // Not listed by git: left for remove_stale to decide
        assert_eq!(meta.get_worktree(&gone).unwrap().unwrap().branch, "gone");
    }
}
