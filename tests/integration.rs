use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::process::Command as StdCommand;
use tempfile::TempDir;

/// Create a temporary git repository for testing
fn setup_git_repo() -> TempDir {
    let dir = TempDir::new().unwrap();

    // Initialize git repo
    StdCommand::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to init git repo");

    // Configure git user for commits
    StdCommand::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to config git");

    StdCommand::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to config git");

    // Create initial commit (required for worktrees)
    fs::write(dir.path().join("README.md"), "# Test Repo").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .expect("Failed to git add");

    StdCommand::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to git commit");

    // Rename branch to main (git might default to master)
    StdCommand::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to rename branch");

    dir
}

fn grove() -> Command {
    let mut cmd = Command::cargo_bin("grove").unwrap();
    // Never read the developer's real global config - tests must be hermetic.
    // The path does not exist, so `load_global` falls back to defaults.
    cmd.env("GROVE_CONFIG_DIR", "/nonexistent/grove-test-config");
    cmd
}

/// Commit a .gitignore so subsequent files are treated as ignored
fn commit_gitignore(repo: &TempDir, contents: &str) {
    fs::write(repo.path().join(".gitignore"), contents).unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "add gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();
}

/// Resolve the on-disk path of a named worktree
fn worktree_path(repo: &TempDir, name: &str) -> std::path::PathBuf {
    let output = grove()
        .args(["path", name])
        .current_dir(repo.path())
        .output()
        .unwrap();
    std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
}

// =============================================================================
// LIST COMMAND TESTS
// =============================================================================

#[test]
fn test_list_shows_header() {
    let repo = setup_git_repo();

    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("🌳 Git Worktrees"))
        .stderr(predicate::str::contains("────"));
}

#[test]
fn test_list_shows_repo_info() {
    let repo = setup_git_repo();

    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("📁"))
        .stderr(predicate::str::contains("main"));
}

#[test]
fn test_list_shows_current_marker() {
    let repo = setup_git_repo();

    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("● main"))
        .stderr(predicate::str::contains("← here"));
}

#[test]
fn test_list_shows_path_below_worktree() {
    let repo = setup_git_repo();

    // Create a worktree first
    grove()
        .args(["add", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .success();

    // List should show the path below the worktree name
    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("feature-test"))
        .stderr(predicate::str::contains(".git/wt/"));
}

#[test]
fn test_list_shows_suggestion() {
    let repo = setup_git_repo();

    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("grove go"))
        .stderr(predicate::str::contains("grove rm"));
}

#[test]
fn test_list_tree_connectors_single() {
    let repo = setup_git_repo();

    // With only main, should use └─
    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("└─"));
}

#[test]
fn test_list_tree_connectors_multiple() {
    let repo = setup_git_repo();

    // Create two worktrees
    grove()
        .args(["add", "first"])
        .current_dir(repo.path())
        .assert()
        .success();
    grove()
        .args(["add", "second"])
        .current_dir(repo.path())
        .assert()
        .success();

    // With multiple items: ├─ for non-last, └─ for last only
    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("├─"))
        .stderr(predicate::str::contains("└─"));
}

// =============================================================================
// ADD COMMAND TESTS
// =============================================================================

#[test]
fn test_add_creates_worktree() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Creating worktree"));

    // Verify worktree exists
    let wt_dir = repo.path().join(".git/wt");
    assert!(wt_dir.exists(), "Worktree directory should exist");

    // Verify database exists
    let db_path = wt_dir.join("grove.db");
    assert!(db_path.exists(), "grove.db should exist");

    // Verify branch was created
    let output = StdCommand::new("git")
        .args(["branch", "--list", "feature-test"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("feature-test"),
        "Branch should be created"
    );
}

#[test]
fn test_add_with_base_branch() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature-test", "main"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Base: main"));
}

#[test]
fn test_add_copies_ignored_files_when_enabled() {
    let repo = setup_git_repo();

    // Enable copy via .grove.toml
    fs::write(repo.path().join(".grove.toml"), "copy = [\".env\"]").unwrap();

    // Create .gitignore
    fs::write(repo.path().join(".gitignore"), ".env\n").unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "add gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Create ignored file
    fs::write(repo.path().join(".env"), "SECRET=123").unwrap();

    // Create worktree - should auto-copy ignored files
    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Copying"));

    // Verify file was auto-copied
    let output = grove()
        .args(["path", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let wt_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(std::path::Path::new(&wt_path).join(".env").exists());
}

#[test]
fn test_add_does_not_copy_ignored_files_by_default() {
    let repo = setup_git_repo();

    // No .grove.toml at all: no copy patterns, copyignored unset
    commit_gitignore(&repo, ".env\n");
    fs::write(repo.path().join(".env"), "SECRET=123").unwrap();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Copying").not());

    assert!(!worktree_path(&repo, "feature").join(".env").exists());
}

#[test]
fn test_add_does_not_copy_ignored_files_when_copyignored_false() {
    let repo = setup_git_repo();

    fs::write(repo.path().join(".grove.toml"), "copyignored = false").unwrap();
    commit_gitignore(&repo, ".env\n");
    fs::write(repo.path().join(".env"), "SECRET=123").unwrap();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Copying").not());

    assert!(!worktree_path(&repo, "feature").join(".env").exists());
}

#[test]
fn test_add_copies_all_ignored_files_with_copyignored() {
    let repo = setup_git_repo();

    // copyignored with no copy patterns at all
    fs::write(repo.path().join(".grove.toml"), "copyignored = true").unwrap();
    commit_gitignore(&repo, ".env\nlogs/\n");

    fs::write(repo.path().join(".env"), "SECRET=123").unwrap();
    fs::create_dir(repo.path().join("logs")).unwrap();
    fs::write(repo.path().join("logs/app.log"), "boot").unwrap();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Copying"));

    let wt = worktree_path(&repo, "feature");
    assert!(wt.join(".env").exists());
    assert!(wt.join("logs/app.log").exists());
}

#[test]
fn test_copyignored_supersedes_copy_patterns() {
    let repo = setup_git_repo();

    // A narrow copy pattern must not restrict copyignored
    fs::write(
        repo.path().join(".grove.toml"),
        "copyignored = true\ncopy = [\".env\"]\n",
    )
    .unwrap();
    commit_gitignore(&repo, ".env\nlogs/\n");

    fs::write(repo.path().join(".env"), "SECRET=123").unwrap();
    fs::create_dir(repo.path().join("logs")).unwrap();
    fs::write(repo.path().join("logs/app.log"), "boot").unwrap();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let wt = worktree_path(&repo, "feature");
    assert!(wt.join(".env").exists());
    assert!(
        wt.join("logs/app.log").exists(),
        "copyignored must copy files outside the copy patterns"
    );
}

#[test]
fn test_copyignored_does_not_copy_git_dir() {
    let repo = setup_git_repo();

    fs::write(repo.path().join(".grove.toml"), "copyignored = true").unwrap();
    commit_gitignore(&repo, ".env\n");
    fs::write(repo.path().join(".env"), "SECRET=123").unwrap();

    // An existing sibling worktree lives under .git/wt/, so a naive
    // "copy everything" would recursively drag it into the new worktree.
    grove()
        .args(["add", "sibling"])
        .current_dir(repo.path())
        .assert()
        .success();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let wt = worktree_path(&repo, "feature");
    assert!(wt.join(".env").exists());
    assert!(
        !wt.join(".git/wt").exists(),
        "must never copy .git contents into a worktree"
    );
}

#[test]
fn test_add_duplicate_fails() {
    let repo = setup_git_repo();

    // Create first worktree
    grove()
        .args(["add", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .success();

    // Try to create duplicate
    grove()
        .args(["add", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_add_shows_in_list() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .success();

    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("feature-test"));
}

// =============================================================================
// GO COMMAND TESTS
// =============================================================================

#[test]
fn test_go_creates_if_not_exists() {
    let repo = setup_git_repo();

    grove()
        .args(["go", "new-feature"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Creating worktree"))
        .stdout(predicate::str::contains("__grove_cd:"));
}

#[test]
fn test_go_existing_outputs_cd() {
    let repo = setup_git_repo();

    // Create worktree
    grove()
        .args(["add", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .success();

    // Go to it
    grove()
        .args(["go", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("__grove_cd:"))
        .stdout(predicate::str::contains(".git/wt/"));
}

#[test]
fn test_go_shorthand() {
    let repo = setup_git_repo();

    // grove <name> should work same as grove go <name>
    grove()
        .arg("new-feature")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Creating worktree"))
        .stdout(predicate::str::contains("__grove_cd:"));
}

#[test]
fn test_go_missing_directory_falls_back_to_default_branch() {
    let repo = setup_git_repo();

    // Create worktree
    grove()
        .args(["add", "will-vanish"])
        .current_dir(repo.path())
        .assert()
        .success();

    // Manually delete the worktree directory (simulating external removal)
    let wt_path = repo.path().join(".git/wt/1");
    std::fs::remove_dir_all(&wt_path).unwrap();

    // Try to go to it — should fallback to default branch with warning
    grove()
        .args(["go", "will-vanish"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("directory missing"))
        .stderr(predicate::str::contains("switching to"))
        // Should cd to repo root, not the missing wt path
        .stdout(predicate::str::contains("__grove_cd:"))
        .stdout(predicate::str::contains(repo.path().to_str().unwrap()));
}

// =============================================================================
// RM COMMAND TESTS
// =============================================================================

#[test]
fn test_rm_removes_worktree() {
    let repo = setup_git_repo();

    // Create worktree
    grove()
        .args(["add", "to-remove"])
        .current_dir(repo.path())
        .assert()
        .success();

    // Remove it
    grove()
        .args(["rm", "to-remove"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed"));

    // Verify not in list
    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("to-remove").not());
}

#[test]
fn test_rm_nonexistent_fails() {
    let repo = setup_git_repo();

    grove()
        .args(["rm", "nonexistent"])
        .current_dir(repo.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_rm_deletes_branch() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "to-remove"])
        .current_dir(repo.path())
        .assert()
        .success();

    grove()
        .args(["rm", "to-remove"])
        .current_dir(repo.path())
        .assert()
        .success();

    // Verify branch is deleted
    let output = StdCommand::new("git")
        .args(["branch", "--list", "to-remove"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "Branch should be deleted"
    );
}

#[test]
fn test_rm_current_worktree_outputs_cd_to_main() {
    let repo = setup_git_repo();

    // Create worktree
    grove()
        .args(["add", "current-wt"])
        .current_dir(repo.path())
        .assert()
        .success();

    let wt_path = repo.path().join(".git/wt/1");

    // Remove while "inside" that worktree - should output __grove_cd: to main repo
    grove()
        .args(["rm", "current-wt"])
        .current_dir(&wt_path)
        .assert()
        .success()
        .stdout(predicates::str::starts_with("__grove_cd:"));
}

// =============================================================================
// PATH COMMAND TESTS
// =============================================================================

#[test]
fn test_path_outputs_worktree_path() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .success();

    grove()
        .args(["path", "feature-test"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(".git/wt/"));
}

#[test]
fn test_path_nonexistent_fails() {
    let repo = setup_git_repo();

    grove()
        .args(["path", "nonexistent"])
        .current_dir(repo.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

// =============================================================================
// NESTED WORKTREE TESTS
// =============================================================================

#[test]
fn test_nested_worktree_sets_parent() {
    let repo = setup_git_repo();

    // Create parent worktree
    grove()
        .args(["add", "parent-feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    // Get the worktree path
    let output = grove()
        .args(["path", "parent-feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let parent_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Create child worktree from inside parent
    grove()
        .args(["add", "child-task"])
        .current_dir(&parent_path)
        .assert()
        .success();

    // List should show hierarchy (child indented under parent)
    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("parent-feature"))
        .stderr(predicate::str::contains("child-task"));
}

// =============================================================================
// CONTEXT-AWARE LOOKUP TESTS
// =============================================================================

#[test]
fn test_context_aware_lookup_prefers_child() {
    let repo = setup_git_repo();

    // Create two worktrees with same name but different parents
    grove()
        .args(["add", "parent-a"])
        .current_dir(repo.path())
        .assert()
        .success();

    let output = grove()
        .args(["path", "parent-a"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let parent_a_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Create child "sub" under parent-a
    grove()
        .args(["add", "sub"])
        .current_dir(&parent_a_path)
        .assert()
        .success();

    // From inside parent-a, "grove go sub" should find the child
    grove()
        .args(["go", "sub"])
        .current_dir(&parent_a_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("__grove_cd:"));
}

// =============================================================================
// PRUNE COMMAND TESTS
// =============================================================================

#[test]
fn test_prune_succeeds() {
    let repo = setup_git_repo();

    grove()
        .arg("prune")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Pruned"));
}

// =============================================================================
// SPECIAL BRANCH NAME TESTS
// =============================================================================

#[test]
fn test_branch_with_slash() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature/auth"])
        .current_dir(repo.path())
        .assert()
        .success();

    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("feature/auth"));
}

#[test]
fn test_branch_with_special_chars() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "fix/JIRA-123_some-bug"])
        .current_dir(repo.path())
        .assert()
        .success();

    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("fix/JIRA-123_some-bug"));
}

// =============================================================================
// PULL/PUSH TESTS (ignored file sync)
// =============================================================================

#[test]
fn test_pull_copies_ignored_files_from_main() {
    let repo = setup_git_repo();

    // Create .gitignore
    fs::write(repo.path().join(".gitignore"), "*.log\n.env\n").unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "add gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Create ignored files in main
    fs::write(repo.path().join("app.log"), "log content").unwrap();
    fs::write(repo.path().join(".env"), "SECRET=123").unwrap();

    // Create worktree
    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let output = grove()
        .args(["path", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let wt_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Pull ignored files
    grove()
        .arg("pull")
        .current_dir(&wt_path)
        .assert()
        .success()
        .stderr(predicate::str::contains("Pulled"));

    // Verify files were copied
    assert!(std::path::Path::new(&wt_path).join("app.log").exists());
    assert!(std::path::Path::new(&wt_path).join(".env").exists());
}

#[test]
fn test_pull_with_specific_paths() {
    let repo = setup_git_repo();

    // Create .gitignore
    fs::write(repo.path().join(".gitignore"), "*.log\n.env\n").unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "add gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Create worktree BEFORE creating ignored files (so nothing to auto-copy)
    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let output = grove()
        .args(["path", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let wt_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // NOW create ignored files in main (after worktree exists)
    fs::write(repo.path().join("app.log"), "log").unwrap();
    fs::write(repo.path().join(".env"), "SECRET=123").unwrap();

    // Pull only .env
    grove()
        .args(["pull", ".env"])
        .current_dir(&wt_path)
        .assert()
        .success();

    // Only .env should be copied
    assert!(std::path::Path::new(&wt_path).join(".env").exists());
    assert!(!std::path::Path::new(&wt_path).join("app.log").exists());
}

#[test]
fn test_push_copies_ignored_files_to_main() {
    let repo = setup_git_repo();

    // Create .gitignore
    fs::write(repo.path().join(".gitignore"), "*.log\n").unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "add gitignore"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Create worktree
    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let output = grove()
        .args(["path", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let wt_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Create ignored file in worktree
    fs::write(
        std::path::Path::new(&wt_path).join("debug.log"),
        "debug output",
    )
    .unwrap();

    // Push ignored files
    grove()
        .arg("push")
        .current_dir(&wt_path)
        .assert()
        .success()
        .stderr(predicate::str::contains("Pushed"));

    // Verify file was copied to main
    assert!(repo.path().join("debug.log").exists());
}

#[test]
fn test_pull_from_main_worktree_fails() {
    let repo = setup_git_repo();

    // Pull from main should fail (nothing to pull from)
    grove()
        .arg("pull")
        .current_dir(repo.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("main").or(predicate::str::contains("primary")));
}

// =============================================================================
// SYNC TESTS
// =============================================================================

#[test]
fn test_sync_imports_existing_git_worktrees() {
    let repo = setup_git_repo();

    // Create worktree directly with git (bypassing grove)
    let wt_path = repo.path().join(".git/wt/legacy");
    fs::create_dir_all(repo.path().join(".git/wt")).unwrap();
    StdCommand::new("git")
        .args([
            "worktree",
            "add",
            wt_path.to_str().unwrap(),
            "-b",
            "legacy-branch",
        ])
        .current_dir(repo.path())
        .output()
        .expect("Failed to create git worktree");

    // Run sync
    grove()
        .arg("sync")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Synced").or(predicate::str::contains("imported")));

    // Now grove should see it
    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("legacy-branch"));
}

#[test]
fn test_sync_removes_stale_entries() {
    let repo = setup_git_repo();

    // Create worktree with grove
    grove()
        .args(["add", "will-delete"])
        .current_dir(repo.path())
        .assert()
        .success();

    // Remove it directly with git (bypassing grove)
    let output = grove()
        .args(["path", "will-delete"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let wt_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    StdCommand::new("git")
        .args(["worktree", "remove", &wt_path])
        .current_dir(repo.path())
        .output()
        .expect("Failed to remove git worktree");

    // Run sync to clean up stale entry
    grove()
        .arg("sync")
        .current_dir(repo.path())
        .assert()
        .success();

    // Grove should no longer see it
    grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("will-delete").not());
}

// =============================================================================
// CONFIG TESTS
// =============================================================================

#[test]
fn test_config_copy_patterns() {
    let dir = setup_git_repo();

    // Create local config with copy patterns
    fs::write(dir.path().join(".grove.toml"), "copy = [\"secret.env\"]").unwrap();

    // Create a file that would be ignored
    fs::write(dir.path().join(".gitignore"), "secret.env\n").unwrap();
    fs::write(dir.path().join("secret.env"), "SECRET=value").unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "Add gitignore"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create worktree - should copy the matching ignored file
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check that secret.env was copied
    let wt_path = dir.path().join(".git/wt");
    let entries: Vec<_> = fs::read_dir(&wt_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() != "grove.db")
        .collect();
    assert_eq!(entries.len(), 1);
    let wt_dir = entries[0].path();
    assert!(
        wt_dir.join("secret.env").exists(),
        "secret.env should be copied"
    );
}

#[test]
fn test_config_copy_glob_pattern() {
    let dir = setup_git_repo();

    // Create local config with glob pattern
    fs::write(dir.path().join(".grove.toml"), "copy = [\".env*\"]").unwrap();

    // Create files that would be ignored
    fs::write(dir.path().join(".gitignore"), ".env*\n").unwrap();
    fs::write(dir.path().join(".env"), "VAR=value").unwrap();
    fs::write(dir.path().join(".env.local"), "LOCAL=value").unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "Add gitignore"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create worktree - should copy both .env files
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check that both files were copied
    let wt_path = dir.path().join(".git/wt");
    let entries: Vec<_> = fs::read_dir(&wt_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() != "grove.db")
        .collect();
    assert_eq!(entries.len(), 1);
    let wt_dir = entries[0].path();
    assert!(wt_dir.join(".env").exists(), ".env should be copied");
    assert!(
        wt_dir.join(".env.local").exists(),
        ".env.local should be copied"
    );
}

#[test]
fn test_config_copy_empty() {
    let dir = setup_git_repo();

    // No copy config

    // Create a file that would be ignored
    fs::write(dir.path().join(".gitignore"), "secret.env\n").unwrap();
    fs::write(dir.path().join("secret.env"), "SECRET=value").unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "Add gitignore"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create worktree - should NOT copy ignored files (no copy config)
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check that secret.env was NOT copied
    let wt_path = dir.path().join(".git/wt");
    let entries: Vec<_> = fs::read_dir(&wt_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() != "grove.db")
        .collect();
    assert_eq!(entries.len(), 1);
    let wt_dir = entries[0].path();
    assert!(
        !wt_dir.join("secret.env").exists(),
        "secret.env should NOT be copied without copy config"
    );
}

#[test]
#[cfg(unix)]
fn test_add_copies_symlinked_directory() {
    use std::os::unix::fs::symlink;

    let dir = setup_git_repo();

    // Mirror the Terraform plugin-cache layout: a real directory plus a symlink
    // pointing at it by absolute path. `git ls-files --others --ignored` reports
    // the symlink as a single entry and does not descend into it.
    fs::create_dir(dir.path().join("cache")).unwrap();
    fs::write(dir.path().join("cache/bin.txt"), "provider-binary").unwrap();
    fs::create_dir(dir.path().join("providers")).unwrap();
    symlink(
        dir.path().join("cache"),
        dir.path().join("providers/current"),
    )
    .unwrap();

    fs::write(dir.path().join(".gitignore"), "cache/\nproviders/\n").unwrap();
    fs::write(dir.path().join(".grove.toml"), "copy = [\"providers/\"]").unwrap();
    StdCommand::new("git")
        .args(["add", ".gitignore", ".grove.toml"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "Add gitignore"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    let wt_path = dir.path().join(".git/wt");
    let entries: Vec<_> = fs::read_dir(&wt_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() != "grove.db")
        .collect();
    assert_eq!(entries.len(), 1);
    let wt_dir = entries[0].path();

    let copied_link = wt_dir.join("providers/current");
    let meta = fs::symlink_metadata(&copied_link)
        .expect("providers/current should exist in the new worktree");
    assert!(
        meta.file_type().is_symlink(),
        "providers/current should be preserved as a symlink, not dereferenced"
    );
    assert_eq!(
        fs::read_to_string(copied_link.join("bin.txt")).expect("link should resolve"),
        "provider-binary"
    );
}

// =============================================================================
// HOOK TESTS
// =============================================================================

#[test]
fn test_hook_post_create_runs() {
    let dir = setup_git_repo();

    // Create local config with a post-create hook that creates a marker file
    let config = r#"
[[hooks.post-create]]
marker = "touch {{path}}/hook-ran.marker"
"#;
    fs::write(dir.path().join(".grove.toml"), config).unwrap();

    // Create worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check that hook ran (marker file exists)
    let wt_path = dir.path().join(".git/wt");
    let entries: Vec<_> = fs::read_dir(&wt_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() != "grove.db")
        .collect();
    assert_eq!(entries.len(), 1);
    let wt_dir = entries[0].path();
    assert!(
        wt_dir.join("hook-ran.marker").exists(),
        "post-create hook should have run"
    );
}

#[test]
fn test_hook_template_variables() {
    let dir = setup_git_repo();

    // Create hook that writes all template variables to a file
    let config = r#"
[[hooks.post-create]]
info = "echo 'path={{path}} branch={{branch}} id={{id}} repo={{repo}}' > {{path}}/vars.txt"
"#;
    fs::write(dir.path().join(".grove.toml"), config).unwrap();

    // Create worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Read the vars file and check contents
    let wt_path = dir.path().join(".git/wt");
    let entries: Vec<_> = fs::read_dir(&wt_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() != "grove.db")
        .collect();
    let wt_dir = entries[0].path().canonicalize().unwrap();
    let vars_content = fs::read_to_string(wt_dir.join("vars.txt")).unwrap();

    assert!(
        vars_content.contains("branch=feature"),
        "should contain branch"
    );
    assert!(
        vars_content.contains(&format!("path={}", wt_dir.display())),
        "should contain path"
    );
    assert!(vars_content.contains("id="), "should contain id");
    assert!(vars_content.contains("repo="), "should contain repo");
}

#[test]
fn test_hook_blocks_run_sequentially() {
    let dir = setup_git_repo();

    // Create config with multiple blocks that write to a file in order
    let config = r#"
[[hooks.post-create]]
first = "echo 'first' >> {{path}}/order.txt"

[[hooks.post-create]]
second = "echo 'second' >> {{path}}/order.txt"

[[hooks.post-create]]
third = "echo 'third' >> {{path}}/order.txt"
"#;
    fs::write(dir.path().join(".grove.toml"), config).unwrap();

    // Create worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check order
    let wt_path = dir.path().join(".git/wt");
    let entries: Vec<_> = fs::read_dir(&wt_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() != "grove.db")
        .collect();
    let wt_dir = entries[0].path();
    let order = fs::read_to_string(wt_dir.join("order.txt")).unwrap();

    assert_eq!(
        order.trim(),
        "first\nsecond\nthird",
        "blocks should run in order"
    );
}

#[test]
fn test_hook_tasks_in_block_run_parallel() {
    let dir = setup_git_repo();

    // Create config with multiple tasks in one block
    // Each task sleeps then writes - if sequential, would take 2+ seconds
    // If parallel, should complete in ~1 second
    let config = r#"
[[hooks.post-create]]
task1 = "sleep 0.5 && echo 'task1' >> {{path}}/parallel.txt"
task2 = "sleep 0.5 && echo 'task2' >> {{path}}/parallel.txt"
"#;
    fs::write(dir.path().join(".grove.toml"), config).unwrap();

    let start = std::time::Instant::now();

    // Create worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    let elapsed = start.elapsed();

    // Should complete in under 1.5 seconds if parallel (0.5s tasks + overhead)
    // Would take 2+ seconds if sequential
    assert!(
        elapsed.as_secs_f64() < 1.5,
        "tasks should run in parallel, took {:?}",
        elapsed
    );

    // Both tasks should have run
    let wt_path = dir.path().join(".git/wt");
    let entries: Vec<_> = fs::read_dir(&wt_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() != "grove.db")
        .collect();
    let wt_dir = entries[0].path();
    let content = fs::read_to_string(wt_dir.join("parallel.txt")).unwrap();
    assert!(content.contains("task1"), "task1 should have run");
    assert!(content.contains("task2"), "task2 should have run");
}

#[test]
fn test_hook_pre_remove_runs() {
    let dir = setup_git_repo();

    // Create worktree first
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Now create config with pre-remove hook
    // Hook writes to main repo since worktree will be deleted
    let config = r#"
[[hooks.pre-remove]]
backup = "echo 'removing {{branch}}' > {{repo}}/removed.log"
"#;
    fs::write(dir.path().join(".grove.toml"), config).unwrap();

    // Remove worktree
    grove()
        .args(["rm", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check hook ran
    let log = fs::read_to_string(dir.path().join("removed.log")).unwrap();
    assert!(
        log.contains("removing feature"),
        "pre-remove hook should have run"
    );
}

// =============================================================================
// CLEAN COMMAND TESTS
// =============================================================================

#[test]
fn test_clean_removes_merged_worktree() {
    let dir = setup_git_repo();

    // Create and add worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Make a commit on the feature branch
    let wt_path = dir.path().join(".git/wt/1");
    fs::write(wt_path.join("feature.txt"), "feature work").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "feature commit"])
        .current_dir(&wt_path)
        .output()
        .unwrap();

    // Merge into main (regular merge)
    StdCommand::new("git")
        .args(["merge", "feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Clean should remove merged worktree
    grove()
        .args(["clean"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("Removing 'feature'"))
        .stderr(predicates::str::contains("merged into main"));

    // Worktree should be gone
    assert!(!wt_path.exists());
}

#[test]
fn test_clean_detects_squash_merge() {
    let dir = setup_git_repo();

    // Create and add worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Make commits on the feature branch
    let wt_path = dir.path().join(".git/wt/1");
    fs::write(wt_path.join("feature.txt"), "feature work").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "feature commit 1"])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    fs::write(wt_path.join("feature2.txt"), "more work").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "feature commit 2"])
        .current_dir(&wt_path)
        .output()
        .unwrap();

    // Squash merge into main (simulates GitHub squash merge)
    StdCommand::new("git")
        .args(["merge", "--squash", "feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "feat: feature (#1)"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Clean should detect squash-merged branch and remove it
    grove()
        .args(["clean"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("Removing 'feature'"))
        .stderr(predicates::str::contains("merged into main"));

    // Worktree should be gone
    assert!(!wt_path.exists());
}

#[test]
fn test_clean_skips_dirty_worktree() {
    let dir = setup_git_repo();

    // Create and add worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    let wt_path = dir.path().join(".git/wt/1");

    // Make uncommitted changes (dirty)
    fs::write(wt_path.join("dirty.txt"), "uncommitted").unwrap();

    // Clean should skip dirty worktree even though it has no commits (would be "merged")
    grove()
        .args(["clean"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "Skipping 'feature': has uncommitted changes",
        ));

    // Worktree should still exist
    assert!(wt_path.exists());
}

#[test]
fn test_clean_skips_unmerged_worktree() {
    let dir = setup_git_repo();

    // Create and add worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Make a commit on the feature branch (but don't merge)
    let wt_path = dir.path().join(".git/wt/1");
    fs::write(wt_path.join("feature.txt"), "feature work").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "feature commit"])
        .current_dir(&wt_path)
        .output()
        .unwrap();

    // Clean should not touch unmerged worktree
    grove()
        .args(["clean"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("No merged worktrees to clean"));

    // Worktree should still exist
    assert!(wt_path.exists());
}

#[test]
fn test_clean_checks_upstream_branch() {
    let dir = setup_git_repo();

    // Create a "develop" branch from main
    StdCommand::new("git")
        .args(["branch", "develop"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create a worktree for feature branch
    grove()
        .args(["add", "feature", "develop"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Make a commit on the feature branch
    let wt_path = dir.path().join(".git/wt/1");
    fs::write(wt_path.join("feature.txt"), "feature work").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "feature commit"])
        .current_dir(&wt_path)
        .output()
        .unwrap();

    // Set feature to track develop (simulates pushing and setting upstream)
    StdCommand::new("git")
        .args(["branch", "--set-upstream-to=develop", "feature"])
        .current_dir(&wt_path)
        .output()
        .unwrap();

    // Merge feature into develop (not main)
    StdCommand::new("git")
        .args(["checkout", "develop"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["merge", "feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["checkout", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Clean should detect feature is merged into its upstream (develop)
    grove()
        .args(["clean"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("Removing 'feature'"))
        .stderr(predicates::str::contains("merged into develop"));

    // Worktree should be gone
    assert!(!wt_path.exists());
}

#[test]
fn test_done_from_merged_worktree_outputs_single_cd() {
    // Set up a bare repo to act as remote
    let remote_dir = TempDir::new().unwrap();
    StdCommand::new("git")
        .args(["init", "--bare"])
        .current_dir(remote_dir.path())
        .output()
        .unwrap();

    let dir = setup_git_repo();

    // Add remote and push main
    StdCommand::new("git")
        .args([
            "remote",
            "add",
            "origin",
            remote_dir.path().to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["push", "-u", "origin", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Create and add worktree
    grove()
        .args(["add", "feature"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Make a commit on the feature branch
    let wt_path = dir.path().join(".git/wt/1");
    fs::write(wt_path.join("feature.txt"), "feature work").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&wt_path)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "feature commit"])
        .current_dir(&wt_path)
        .output()
        .unwrap();

    // Merge into main (regular merge)
    StdCommand::new("git")
        .args(["merge", "feature"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Run 'done' from INSIDE the worktree that will be cleaned
    let output = grove()
        .args(["done"])
        .current_dir(&wt_path)
        .assert()
        .success()
        .stderr(predicates::str::contains("Removing 'feature'"))
        .get_output()
        .stdout
        .clone();

    // Should output exactly one __grove_cd: line
    let stdout = String::from_utf8_lossy(&output);
    let cd_count = stdout.matches("__grove_cd:").count();
    assert_eq!(
        cd_count, 1,
        "Expected exactly 1 __grove_cd: but found {}: {}",
        cd_count, stdout
    );

    // Worktree should be gone
    assert!(!wt_path.exists());
}

// =============================================================================
// -C FLAG TESTS
// =============================================================================

#[test]
fn test_dash_c_runs_in_specified_directory() {
    let repo = setup_git_repo();

    // Run from a different directory (temp dir root), but use -C to point at repo
    grove()
        .arg("-C")
        .arg(repo.path())
        .arg("list")
        .current_dir(std::env::temp_dir())
        .assert()
        .success()
        .stderr(predicate::str::contains("🌳 Git Worktrees"));
}

#[test]
fn test_dash_c_with_add_command() {
    let repo = setup_git_repo();

    // Create worktree using -C from outside the repo
    grove()
        .arg("-C")
        .arg(repo.path())
        .args(["add", "feature"])
        .current_dir(std::env::temp_dir())
        .assert()
        .success()
        .stderr(predicate::str::contains("Worktree created"));

    // Verify worktree exists
    assert!(repo.path().join(".git/wt/1").exists());
}

#[test]
fn test_dash_c_with_invalid_path() {
    grove()
        .arg("-C")
        .arg("/nonexistent/path/that/does/not/exist")
        .arg("list")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to change to"));
}

// =============================================================================
// PORCELAIN OUTPUT TESTS
// =============================================================================

/// Parse porcelain stdout into records: `Vec<Vec<String>>`, one inner vec per
/// line, fields already unescaped.
fn parse_porcelain(stdout: &str) -> Vec<Vec<String>> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            line.split('\t')
                .map(|f| {
                    // Reverse the escaping applied by porcelain::escape
                    let mut out = String::new();
                    let mut chars = f.chars();
                    while let Some(c) = chars.next() {
                        if c == '\\' {
                            match chars.next() {
                                Some('t') => out.push('\t'),
                                Some('n') => out.push('\n'),
                                Some('\\') => out.push('\\'),
                                Some(other) => {
                                    out.push('\\');
                                    out.push(other);
                                }
                                None => out.push('\\'),
                            }
                        } else {
                            out.push(c);
                        }
                    }
                    out
                })
                .collect()
        })
        .collect()
}

/// Run grove with --porcelain and return parsed stdout records
fn porcelain_records(repo_dir: &std::path::Path, args: &[&str]) -> Vec<Vec<String>> {
    let output = grove()
        .arg("--porcelain")
        .args(args)
        .current_dir(repo_dir)
        .output()
        .unwrap();
    parse_porcelain(&String::from_utf8_lossy(&output.stdout))
}

/// Find the first record of a given type
fn find_record<'a>(records: &'a [Vec<String>], kind: &str) -> Option<&'a Vec<String>> {
    records
        .iter()
        .find(|r| r.first().map(String::as_str) == Some(kind))
}

/// Find all records of a given type
fn find_records<'a>(records: &'a [Vec<String>], kind: &str) -> Vec<&'a Vec<String>> {
    records
        .iter()
        .filter(|r| r.first().map(String::as_str) == Some(kind))
        .collect()
}

/// Locate the `wt` record for a given branch
fn find_wt<'a>(records: &'a [Vec<String>], branch: &str) -> Option<&'a Vec<String>> {
    records.iter().find(|r| {
        r.first().map(String::as_str) == Some("wt") && r.get(2).map(String::as_str) == Some(branch)
    })
}

#[test]
fn test_porcelain_list_emits_repo_and_wt_records() {
    let repo = setup_git_repo();

    let records = porcelain_records(repo.path(), &["list"]);

    let repo_rec = find_record(&records, "repo").expect("expected a repo record");
    assert_eq!(
        repo_rec.len(),
        3,
        "repo record: repo\\t<root>\\t<default-branch>"
    );
    assert_eq!(repo_rec[2], "main");

    let main_wt = find_wt(&records, "main").expect("expected a wt record for main");
    assert_eq!(main_wt.len(), 9, "wt record has 9 fields");
    assert_eq!(main_wt[1], "-", "primary worktree has no id");
    assert_eq!(main_wt[4], "-", "primary worktree has no parent");
    assert!(main_wt[5].contains('p'), "primary flag: {}", main_wt[5]);
    assert!(main_wt[5].contains('c'), "current flag: {}", main_wt[5]);
}

#[test]
fn test_porcelain_list_no_ansi_no_emoji() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let output = grove()
        .args(["--porcelain", "list"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "porcelain list must write to stdout");
    assert!(
        stdout.is_ascii(),
        "porcelain output must be pure ASCII, got: {:?}",
        stdout
    );
    assert!(
        !stdout.contains('\x1b'),
        "porcelain output must contain no ANSI escapes"
    );

    // Human decoration must be gone entirely, not just moved
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Git Worktrees"),
        "porcelain must suppress the pretty header, got: {:?}",
        stderr
    );
}

#[test]
fn test_porcelain_list_child_reports_parent_id_and_cmp_base() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "parent-wt"])
        .current_dir(repo.path())
        .assert()
        .success();

    let parent_path = repo.path().join(".git/wt/1");
    grove()
        .args(["add", "child-wt"])
        .current_dir(&parent_path)
        .assert()
        .success();

    let records = porcelain_records(repo.path(), &["list"]);

    let parent = find_wt(&records, "parent-wt").expect("parent record");
    let child = find_wt(&records, "child-wt").expect("child record");

    assert_eq!(child[4], parent[1], "child parent-id points at the parent");
    assert_eq!(
        child[8], "parent-wt",
        "child ahead/behind is measured against its parent's branch"
    );
    assert_eq!(
        parent[8], "main",
        "top-level ahead/behind is measured against the default branch"
    );
}

#[test]
fn test_porcelain_list_marks_dirty_and_missing_flags() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "dirty-wt"])
        .current_dir(repo.path())
        .assert()
        .success();

    let wt_path = repo.path().join(".git/wt/1");

    // Modify a tracked file and add an untracked one
    fs::write(wt_path.join("README.md"), "# Changed").unwrap();
    fs::write(wt_path.join("brand-new.txt"), "hello").unwrap();

    let records = porcelain_records(repo.path(), &["list"]);
    let wt = find_wt(&records, "dirty-wt").expect("dirty-wt record");
    assert!(wt[5].contains('m'), "modified flag expected in {}", wt[5]);
    assert!(wt[5].contains('u'), "untracked flag expected in {}", wt[5]);

    // Now delete the directory out from under grove
    fs::remove_dir_all(&wt_path).unwrap();

    let records = porcelain_records(repo.path(), &["list"]);
    let wt = find_wt(&records, "dirty-wt").expect("dirty-wt record after deletion");
    assert!(
        wt[5].contains('x'),
        "missing-dir flag expected in {}",
        wt[5]
    );
}

#[test]
fn test_porcelain_list_marks_orphans() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "the-parent"])
        .current_dir(repo.path())
        .assert()
        .success();

    let parent_path = repo.path().join(".git/wt/1");
    grove()
        .args(["add", "the-child"])
        .current_dir(&parent_path)
        .assert()
        .success();

    // Remove the parent; the child is silently orphaned
    grove()
        .args(["rm", "the-parent"])
        .current_dir(repo.path())
        .assert()
        .success();

    let records = porcelain_records(repo.path(), &["list"]);
    let child = find_wt(&records, "the-child").expect("orphaned child record");
    assert!(
        child[5].contains('o'),
        "orphan flag expected in {}",
        child[5]
    );
}

#[test]
fn test_porcelain_add_emits_created_and_wt() {
    let repo = setup_git_repo();

    let records = porcelain_records(repo.path(), &["add", "feature"]);

    let created = find_record(&records, "created").expect("created record");
    assert_eq!(created.len(), 4, "created\\t<id>\\t<branch>\\t<path>");
    assert_eq!(created[2], "feature");
    assert!(created[3].ends_with(".git/wt/1"), "path: {}", created[3]);

    let wt = find_wt(&records, "feature").expect("wt record for the new worktree");
    assert_eq!(wt[1], created[1], "wt id matches created id");
}

#[test]
fn test_porcelain_go_emits_cd_record_not_grove_cd_prefix() {
    let repo = setup_git_repo();

    let output = grove()
        .args(["--porcelain", "go", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("__grove_cd:"),
        "porcelain mode replaces the shell prefix with a cd record"
    );

    let records = parse_porcelain(&stdout);
    let cd = find_record(&records, "cd").expect("cd record");
    assert_eq!(cd.len(), 2);
    assert!(cd[1].ends_with(".git/wt/1"), "cd path: {}", cd[1]);

    find_record(&records, "created").expect("go on a new name also emits created");
}

#[test]
fn test_porcelain_go_existing_worktree_single_invocation() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let records = porcelain_records(repo.path(), &["go", "feature"]);

    assert!(
        find_record(&records, "created").is_none(),
        "switching to an existing worktree must not report creation"
    );
    let cd = find_record(&records, "cd").expect("cd record");
    assert!(cd[1].ends_with(".git/wt/1"));
    find_wt(&records, "feature").expect("wt record for the target");
}

#[test]
fn test_porcelain_go_without_name_lists_instead_of_fzf() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let records = porcelain_records(repo.path(), &["go"]);

    find_record(&records, "repo").expect("bare `go` in porcelain mode degrades to a listing");
    find_wt(&records, "feature").expect("wt record present");
    assert!(
        find_record(&records, "cd").is_none(),
        "no selection was made, so nothing to cd to"
    );
}

#[test]
fn test_porcelain_remove_emits_removed_and_orphaned() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "doomed"])
        .current_dir(repo.path())
        .assert()
        .success();

    let parent_path = repo.path().join(".git/wt/1");
    grove()
        .args(["add", "survivor"])
        .current_dir(&parent_path)
        .assert()
        .success();

    let records = porcelain_records(repo.path(), &["rm", "doomed"]);

    let removed = find_record(&records, "removed").expect("removed record");
    assert_eq!(removed.len(), 4, "removed\\t<id>\\t<branch>\\t<reason>");
    assert_eq!(removed[2], "doomed");
    assert_eq!(removed[3], "explicit");

    let orphaned = find_record(&records, "orphaned").expect("orphaned record");
    assert_eq!(orphaned[2], "survivor");
}

#[test]
fn test_porcelain_remove_current_emits_cd() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "current-wt"])
        .current_dir(repo.path())
        .assert()
        .success();

    let wt_path = repo.path().join(".git/wt/1");
    let records = porcelain_records(&wt_path, &["rm", "current-wt"]);

    let cd = find_record(&records, "cd").expect("cd back to the repo root");
    assert_eq!(cd[1], repo.path().canonicalize().unwrap().to_string_lossy());
}

#[test]
fn test_porcelain_clean_emits_removed_and_skipped() {
    let repo = setup_git_repo();

    // A merged worktree, ready to be cleaned
    grove()
        .args(["add", "merged-wt"])
        .current_dir(repo.path())
        .assert()
        .success();
    let merged_path = repo.path().join(".git/wt/1");
    fs::write(merged_path.join("f.txt"), "x").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&merged_path)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "work"])
        .current_dir(&merged_path)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["merge", "merged-wt"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // A second merged worktree that is dirty, so clean must skip it
    grove()
        .args(["add", "dirty-wt"])
        .current_dir(repo.path())
        .assert()
        .success();
    let dirty_path = repo.path().join(".git/wt/2");
    StdCommand::new("git")
        .args(["merge", "dirty-wt"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    fs::write(dirty_path.join("README.md"), "# dirty").unwrap();

    let records = porcelain_records(repo.path(), &["clean"]);

    let removed = find_records(&records, "removed");
    assert!(
        removed.iter().any(|r| r[2] == "merged-wt"),
        "merged worktree should be removed: {:?}",
        records
    );
    assert!(
        removed
            .iter()
            .find(|r| r[2] == "merged-wt")
            .is_some_and(|r| r[3].starts_with("merged:")),
        "removal reason records the ref it was merged into"
    );

    let skipped = find_record(&records, "skipped").expect("skipped record for the dirty worktree");
    assert_eq!(skipped[2], "dirty-wt");
    assert_eq!(skipped[3], "dirty");
}

#[test]
fn test_porcelain_sync_emits_imported() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "tracked"])
        .current_dir(repo.path())
        .assert()
        .success();

    // Drop the metadata database so sync has to re-import from git
    fs::remove_file(repo.path().join(".git/wt/grove.db")).unwrap();

    let records = porcelain_records(repo.path(), &["sync"]);
    let imported = find_record(&records, "imported").expect("imported record");
    assert_eq!(imported.len(), 3, "imported\\t<id>\\t<branch>");
    assert_eq!(imported[2], "tracked");
}

#[test]
fn test_porcelain_prune_emits_pruned() {
    let repo = setup_git_repo();

    let records = porcelain_records(repo.path(), &["prune"]);
    find_record(&records, "pruned").expect("pruned record");
}

#[test]
fn test_porcelain_path_emits_path_record() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let records = porcelain_records(repo.path(), &["path", "feature"]);
    let path = find_record(&records, "path").expect("path record");
    assert_eq!(path.len(), 2);
    assert!(path[1].ends_with(".git/wt/1"), "path: {}", path[1]);
}

#[test]
fn test_porcelain_pull_emits_copied() {
    let repo = setup_git_repo();
    commit_gitignore(&repo, ".env\n");
    fs::write(repo.path().join(".env"), "SECRET=1").unwrap();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    let wt_path = repo.path().join(".git/wt/1");
    let records = porcelain_records(&wt_path, &["pull"]);

    let copied = find_record(&records, "copied").expect("copied record");
    assert_eq!(copied.len(), 2, "copied\\t<count>");
    assert_eq!(copied[1], "1");
}

#[test]
fn test_porcelain_error_goes_to_stderr_as_tsv() {
    let repo = setup_git_repo();

    let output = grove()
        .args(["--porcelain", "path", "no-such-worktree"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(!output.status.success(), "missing worktree is an error");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "errors must not pollute the record stream: {:?}",
        stdout
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error\t"),
        "expected a TSV error record, got: {:?}",
        stderr
    );
    assert!(
        !stderr.contains('\u{2717}'),
        "no ✗ decoration in porcelain mode"
    );
    assert!(stderr.contains("no-such-worktree"));
}

#[test]
fn test_porcelain_escapes_tab_in_repo_path() {
    // A repo whose path contains a tab would otherwise split a field in two
    let parent = TempDir::new().unwrap();
    let repo_dir = parent.path().join("has\ttab");
    fs::create_dir(&repo_dir).unwrap();

    for args in [
        vec!["init"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test"],
    ] {
        StdCommand::new("git")
            .args(&args)
            .current_dir(&repo_dir)
            .output()
            .unwrap();
    }
    fs::write(repo_dir.join("README.md"), "# Test").unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&repo_dir)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&repo_dir)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(&repo_dir)
        .output()
        .unwrap();

    let output = grove()
        .args(["--porcelain", "list"])
        .current_dir(&repo_dir)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let repo_line = stdout
        .lines()
        .find(|l| l.starts_with("repo\t"))
        .expect("repo record");
    assert_eq!(
        repo_line.split('\t').count(),
        3,
        "escaped tab must not create an extra field: {:?}",
        repo_line
    );

    let records = parse_porcelain(&stdout);
    let repo_rec = find_record(&records, "repo").unwrap();
    assert!(
        repo_rec[1].contains("has\ttab"),
        "unescaping round-trips the literal tab: {:?}",
        repo_rec[1]
    );
}

#[test]
fn test_porcelain_hook_stdout_does_not_pollute_records() {
    let repo = setup_git_repo();

    fs::write(
        repo.path().join(".grove.toml"),
        "[[hooks.post-create]]\nnoisy = \"echo NOT-A-RECORD && echo also\\tnot\"\n",
    )
    .unwrap();

    let output = grove()
        .args(["--porcelain", "add", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("NOT-A-RECORD"),
        "hook stdout must not land in the record stream: {:?}",
        stdout
    );

    let known = [
        "repo", "wt", "cd", "created", "removed", "orphaned", "skipped", "imported", "pruned",
        "fetched", "pulled", "copied", "copyfail", "path",
    ];
    for record in parse_porcelain(&stdout) {
        assert!(
            known.contains(&record[0].as_str()),
            "unexpected record type {:?} in {:?}",
            record[0],
            stdout
        );
    }
}

#[test]
fn test_pretty_output_unchanged_without_flag() {
    let repo = setup_git_repo();

    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success();

    // The human-facing rendering is untouched by the porcelain work
    let output = grove()
        .arg("list")
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("🌳 Git Worktrees"))
        .stderr(predicate::str::contains("📁"))
        .stderr(predicate::str::contains("● main"))
        .stderr(predicate::str::contains("← here"))
        .stderr(predicate::str::contains("💡 Use 'grove go <name>'"))
        .get_output()
        .stdout
        .clone();

    assert!(
        String::from_utf8_lossy(&output).trim().is_empty(),
        "without --porcelain, list writes nothing to stdout"
    );
}
