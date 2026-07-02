use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands;

#[derive(Parser)]
#[command(name = "grove")]
#[command(about = "A fast, simple git worktree manager")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Worktree name (shorthand for `grove go <name>`)
    #[arg(value_name = "NAME")]
    name: Option<String>,

    /// Base branch for new worktree
    #[arg(value_name = "BASE")]
    base: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Go to worktree (create if needed, or fzf select if no name)
    Go {
        #[arg(value_name = "NAME")]
        name: Option<String>,
        #[arg(value_name = "BASE")]
        base: Option<String>,
    },

    /// Create worktree without switching
    Add {
        name: String,
        #[arg(value_name = "BASE")]
        base: Option<String>,
    },

    /// Remove worktree
    Rm {
        name: String,
        #[arg(short, long)]
        force: bool,
    },

    /// List worktrees
    List,

    /// List worktrees (alias)
    Ls,

    /// Clean stale worktree references
    Prune,

    /// Sync database with git worktrees (import existing, remove stale)
    Sync,

    /// Remove merged worktrees
    Clean {
        #[arg(value_name = "BRANCH")]
        branch: Option<String>,
    },

    /// Finish up: cd to main, pull, clean
    Done,

    /// Copy ignored files from main to current worktree
    Pull {
        #[arg(value_name = "PATH")]
        paths: Vec<String>,
    },

    /// Copy ignored files from current worktree to main
    Push {
        #[arg(value_name = "PATH")]
        paths: Vec<String>,
    },

    /// Print path to worktree
    Path { name: String },

    /// Output shell integration script
    Init {
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Output completions (for shell integration)
    #[command(hide = true)]
    Complete {
        /// Subcommand context (go, rm, path, etc.)
        #[arg(value_name = "CMD")]
        cmd: Option<String>,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        // Check for orphaned worktree first (user stuck in deleted directory)
        // Only check for commands that need repo context and have no explicit target
        let needs_orphan_check = matches!(
            (&self.command, &self.name),
            (None, None) |  // bare `grove` -> list
            (Some(Commands::List), _) |
            (Some(Commands::Ls), _) |
            (Some(Commands::Go { name: None, .. }), _)  // interactive go
        );
        
        if needs_orphan_check {
            if commands::check_orphaned_worktree()? {
                return Ok(()); // Already handled - cd'd to main repo
            }
        }
        
        match &self.command {
            Some(Commands::Go { name: Some(name), base }) => commands::go(name, base.as_deref()),
            Some(Commands::Go { name: None, .. }) => commands::go_interactive(),
            Some(Commands::Add { name, base }) => commands::add(name, base.as_deref()),
            Some(Commands::Rm { name, force }) => commands::rm(name, *force),
            Some(Commands::List) | Some(Commands::Ls) => commands::list(),
            Some(Commands::Prune) => commands::prune(),
            Some(Commands::Sync) => commands::sync(),
            Some(Commands::Clean { branch }) => commands::clean(branch.as_deref()),
            Some(Commands::Done) => commands::done(),
            Some(Commands::Pull { paths }) => commands::pull(paths),
            Some(Commands::Push { paths }) => commands::push(paths),
            Some(Commands::Path { name }) => commands::path(name),
            Some(Commands::Init { shell }) => match shell {
                Shell::Bash => crate::shell::init_bash(),
                Shell::Zsh => crate::shell::init_zsh(),
                Shell::Fish => crate::shell::init_fish(),
            },
            Some(Commands::Complete { cmd }) => commands::complete(cmd.as_deref()),
            None => {
                // No subcommand - check for positional name arg
                if let Some(name) = &self.name {
                    commands::go(name, self.base.as_deref())
                } else {
                    // No name - show list
                    commands::list()
                }
            }
        }
    }
}
