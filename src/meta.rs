use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata for all worktrees in a repository
#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
    pub version: u32,
    pub worktrees: HashMap<String, WorktreeInfo>,
    pub next_id: u32,
}

/// Information about a single worktree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub branch: String,
    pub parent: Option<String>,
    pub created: DateTime<Utc>,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            version: 1,
            worktrees: HashMap::new(),
            next_id: 1,
        }
    }
}

impl Meta {
    /// Load metadata from .git/wt/meta.json, or create default if not exists
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = Self::meta_path(repo_root);
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse {}", path.display()))
        } else {
            Ok(Self::default())
        }
    }

    /// Save metadata to .git/wt/meta.json
    pub fn save(&self, repo_root: &Path) -> Result<()> {
        let path = Self::meta_path(repo_root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Get the path to meta.json
    fn meta_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".git/wt/meta.json")
    }

    /// Generate the next worktree ID (base36)
    pub fn next_id(&mut self) -> String {
        let id = base36_encode(self.next_id);
        self.next_id += 1;
        id
    }

    /// Add a new worktree
    pub fn add_worktree(&mut self, branch: &str, parent: Option<&str>) -> String {
        let id = self.next_id();
        self.worktrees.insert(
            id.clone(),
            WorktreeInfo {
                branch: branch.to_string(),
                parent: parent.map(String::from),
                created: Utc::now(),
            },
        );
        id
    }

    /// Remove a worktree by ID
    pub fn remove_worktree(&mut self, id: &str) -> Option<WorktreeInfo> {
        self.worktrees.remove(id)
    }

    /// Find worktree ID by branch name
    pub fn find_by_branch(&self, branch: &str) -> Option<&str> {
        self.worktrees
            .iter()
            .find(|(_, info)| info.branch == branch)
            .map(|(id, _)| id.as_str())
    }

    /// Find worktree ID by branch name, preferring children of given parent
    pub fn find_by_branch_with_context(&self, branch: &str, parent_id: Option<&str>) -> Option<&str> {
        // First try to find a child of the current worktree
        if let Some(pid) = parent_id {
            if let Some((id, _)) = self.worktrees.iter().find(|(_, info)| {
                info.branch == branch && info.parent.as_deref() == Some(pid)
            }) {
                return Some(id.as_str());
            }
        }
        // Fall back to any match
        self.find_by_branch(branch)
    }

    /// Get worktree path
    pub fn worktree_path(&self, repo_root: &Path, id: &str) -> PathBuf {
        repo_root.join(".git/wt").join(id)
    }

    /// Get children of a worktree
    pub fn children(&self, parent_id: &str) -> Vec<(&str, &WorktreeInfo)> {
        self.worktrees
            .iter()
            .filter(|(_, info)| info.parent.as_deref() == Some(parent_id))
            .map(|(id, info)| (id.as_str(), info))
            .collect()
    }

    /// Get top-level worktrees (no parent)
    pub fn top_level(&self) -> Vec<(&str, &WorktreeInfo)> {
        self.worktrees
            .iter()
            .filter(|(_, info)| info.parent.is_none())
            .map(|(id, info)| (id.as_str(), info))
            .collect()
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

    #[test]
    fn test_base36_encode() {
        assert_eq!(base36_encode(0), "0");
        assert_eq!(base36_encode(1), "1");
        assert_eq!(base36_encode(9), "9");
        assert_eq!(base36_encode(10), "a");
        assert_eq!(base36_encode(35), "z");
        assert_eq!(base36_encode(36), "10");
        assert_eq!(base36_encode(100), "2s");
    }

    #[test]
    fn test_meta_default() {
        let meta = Meta::default();
        assert_eq!(meta.version, 1);
        assert!(meta.worktrees.is_empty());
        assert_eq!(meta.next_id, 1);
    }

    #[test]
    fn test_add_worktree() {
        let mut meta = Meta::default();
        let id1 = meta.add_worktree("feature/auth", None);
        let id2 = meta.add_worktree("sub-task", Some(&id1));
        
        assert_eq!(id1, "1");
        assert_eq!(id2, "2");
        assert_eq!(meta.worktrees.len(), 2);
        assert_eq!(meta.worktrees[&id1].branch, "feature/auth");
        assert_eq!(meta.worktrees[&id2].parent, Some(id1.clone()));
    }

    #[test]
    fn test_find_by_branch() {
        let mut meta = Meta::default();
        let id = meta.add_worktree("feature/test", None);
        
        assert_eq!(meta.find_by_branch("feature/test"), Some(id.as_str()));
        assert_eq!(meta.find_by_branch("nonexistent"), None);
    }

    #[test]
    fn test_children() {
        let mut meta = Meta::default();
        let parent_id = meta.add_worktree("parent", None);
        meta.add_worktree("child1", Some(&parent_id));
        meta.add_worktree("child2", Some(&parent_id));
        meta.add_worktree("other", None);
        
        let children = meta.children(&parent_id);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_serialization() {
        let mut meta = Meta::default();
        meta.add_worktree("test", None);
        
        let json = serde_json::to_string(&meta).unwrap();
        let restored: Meta = serde_json::from_str(&json).unwrap();
        
        assert_eq!(restored.version, meta.version);
        assert_eq!(restored.next_id, meta.next_id);
    }
}
