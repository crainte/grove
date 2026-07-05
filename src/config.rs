use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Configuration for grove, merged from global and local sources
#[derive(Debug, Default)]
pub struct Config {
    pub copy: Vec<String>,
    pub hooks: Hooks,
}

/// All hook configurations
#[derive(Debug, Default)]
pub struct Hooks {
    pub post_create: Vec<HookBlock>,
    pub pre_remove: Vec<HookBlock>,
}

/// A block of hook tasks that run in parallel
#[derive(Debug, Clone)]
pub struct HookBlock {
    /// Named tasks: name -> command template
    pub tasks: HashMap<String, String>,
}

/// Template variables for hook expansion
pub struct HookContext<'a> {
    pub path: &'a Path,
    pub branch: &'a str,
    pub id: &'a str,
    pub repo: &'a Path,
}

impl Config {
    /// Load configuration, merging global and local sources
    /// Local takes precedence over global
    pub fn load(repo_root: &Path) -> Result<Self> {
        let global = Self::load_global().unwrap_or_default();
        let local = Self::load_local(repo_root).unwrap_or_default();

        Ok(Self::merge(global, local))
    }

    /// Load global config from ~/.config/grove/config.toml
    fn load_global() -> Result<RawConfig> {
        let path = dirs_path()?.join("config.toml");
        if !path.exists() {
            return Ok(RawConfig::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Load local config from {repo}/.grove.toml
    fn load_local(repo_root: &Path) -> Result<RawConfig> {
        let path = repo_root.join(".grove.toml");
        if !path.exists() {
            return Ok(RawConfig::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Merge global and local configs (local wins)
    fn merge(global: RawConfig, local: RawConfig) -> Self {
        // Merge copy patterns: local extends global
        let mut copy = global.copy;
        copy.extend(local.copy);

        // Merge hooks: local blocks come after global blocks
        let hooks = Hooks {
            post_create: merge_hook_blocks(
                global.hooks.as_ref().map(|h| &h.post_create),
                local.hooks.as_ref().map(|h| &h.post_create),
            ),
            pre_remove: merge_hook_blocks(
                global.hooks.as_ref().map(|h| &h.pre_remove),
                local.hooks.as_ref().map(|h| &h.pre_remove),
            ),
        };

        Self { copy, hooks }
    }
}

fn merge_hook_blocks(
    global: Option<&Vec<RawHookBlock>>,
    local: Option<&Vec<RawHookBlock>>,
) -> Vec<HookBlock> {
    let mut result = Vec::new();

    if let Some(blocks) = global {
        for block in blocks {
            result.push(HookBlock {
                tasks: block.0.clone(),
            });
        }
    }
    if let Some(blocks) = local {
        for block in blocks {
            result.push(HookBlock {
                tasks: block.0.clone(),
            });
        }
    }

    result
}

fn dirs_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".config/grove"))
}

/// Raw TOML structure for deserialization
#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    copy: Vec<String>,
    hooks: Option<RawHooks>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawHooks {
    #[serde(default)]
    post_create: Vec<RawHookBlock>,
    #[serde(default)]
    pre_remove: Vec<RawHookBlock>,
}

/// A hook block is a map of task names to command templates
#[derive(Debug, Clone, Deserialize)]
struct RawHookBlock(HashMap<String, String>);

// =============================================================================
// Hook Execution
// =============================================================================

impl HookBlock {
    /// Execute all tasks in this block in parallel
    /// Returns error if any task fails
    pub fn execute(&self, ctx: &HookContext) -> Result<()> {
        use rayon::prelude::*;

        let results: Vec<Result<()>> = self
            .tasks
            .par_iter()
            .map(|(name, template)| {
                let cmd = expand_template(template, ctx);
                run_hook_command(name, &cmd)
            })
            .collect();

        // Return first error if any
        for result in results {
            result?;
        }
        Ok(())
    }
}

/// Expand template variables in a hook command
fn expand_template(template: &str, ctx: &HookContext) -> String {
    template
        .replace("{{path}}", &ctx.path.display().to_string())
        .replace("{{branch}}", ctx.branch)
        .replace("{{id}}", ctx.id)
        .replace("{{repo}}", &ctx.repo.display().to_string())
}

/// Run a single hook command
fn run_hook_command(name: &str, cmd: &str) -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .with_context(|| format!("Failed to execute hook '{}'", name))?;

    if !status.success() {
        anyhow::bail!("Hook '{}' failed with exit code {:?}", name, status.code());
    }
    Ok(())
}

/// Run a sequence of hook blocks (blocks run sequentially, tasks within parallel)
pub fn run_hooks(blocks: &[HookBlock], ctx: &HookContext) -> Result<()> {
    for block in blocks {
        block.execute(ctx)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_copy_patterns() {
        let toml = r#"
copy = [".env*", ".terraform/"]
"#;
        let raw: RawConfig = toml::from_str(toml).unwrap();
        assert_eq!(raw.copy.len(), 2);
        assert_eq!(raw.copy[0], ".env*");
        assert_eq!(raw.copy[1], ".terraform/");
    }

    #[test]
    fn test_parse_hooks() {
        let toml = r#"
[[hooks.post-create]]
trust = "mise trust {{path}}"
deps = "npm ci"

[[hooks.post-create]]
server = "npm run dev"
"#;
        let raw: RawConfig = toml::from_str(toml).unwrap();
        let hooks = raw.hooks.unwrap();
        assert_eq!(hooks.post_create.len(), 2);
        assert_eq!(hooks.post_create[0].0.len(), 2);
        assert_eq!(hooks.post_create[1].0.len(), 1);
    }

    #[test]
    fn test_expand_template() {
        let ctx = HookContext {
            path: Path::new("/repo/.git/wt/a1"),
            branch: "feature/auth",
            id: "a1",
            repo: Path::new("/repo"),
        };

        let result = expand_template("mise trust {{path}} && echo {{branch}}", &ctx);
        assert_eq!(result, "mise trust /repo/.git/wt/a1 && echo feature/auth");
    }

    #[test]
    fn test_load_missing_config() {
        let dir = TempDir::new().unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.copy.is_empty());
        assert!(config.hooks.post_create.is_empty());
    }

    #[test]
    fn test_load_local_config() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".grove.toml"), "copy = [\".env*\"]").unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.copy.len(), 1);
        assert_eq!(config.copy[0], ".env*");
    }
}
