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
    Command::cargo_bin("grove").unwrap()
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
fn test_add_does_not_copy_ignored_files_when_disabled() {
    let repo = setup_git_repo();

    // Explicitly disable copyignored via .grove.toml
    fs::write(repo.path().join(".grove.toml"), "copyignored = false").unwrap();

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

    // Create worktree - should NOT auto-copy ignored files
    grove()
        .args(["add", "feature"])
        .current_dir(repo.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Copying").not());

    // Verify file was NOT copied
    let output = grove()
        .args(["path", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let wt_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(!std::path::Path::new(&wt_path).join(".env").exists());
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
    let wt_dir = entries[0].path();
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
