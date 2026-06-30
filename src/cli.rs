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
    /// Go to worktree (create if needed)
    Go {
        name: String,
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
}

#[derive(Clone, clap::ValueEnum)]
enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        match &self.command {
            Some(Commands::Go { name, base }) => commands::go(name, base.as_deref()),
            Some(Commands::Add { name, base }) => commands::add(name, base.as_deref()),
            Some(Commands::Rm { name, force }) => commands::rm(name, *force),
            Some(Commands::List) | Some(Commands::Ls) => commands::list(),
            Some(Commands::Prune) => commands::prune(),
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
            None => {
                // No subcommand - check for positional name arg
                if let Some(name) = &self.name {
                    commands::go(name, self.base.as_deref())
                } else {
                    commands::list()
                }
            }
        }
    }
}
