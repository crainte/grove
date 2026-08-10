mod cli;
mod commands;
mod config;
mod copyfiles;
mod git;
mod meta;
mod shell;

use clap::Parser;
use colored::Colorize;
use std::env;
use std::io::IsTerminal;

fn main() {
    // Force colors if stderr is a TTY (colored crate only checks stdout)
    if std::io::stderr().is_terminal() {
        colored::control::set_override(true);
    }

    let cli = cli::Cli::parse();

    if let Some(ref dir) = cli.directory
        && let Err(e) = env::set_current_dir(dir)
    {
        eprintln!("{} Failed to change to {}: {}", "✗".red(), dir.display(), e);
        std::process::exit(1);
    }

    if let Err(e) = cli.run() {
        let msg = format!("{}", e);
        // Warnings are recoverable (user can --force, etc.)
        if msg.contains("Use --force") || msg.contains("Use -f") {
            eprintln!("{} {}", "⚠".yellow(), msg.yellow());
        } else {
            eprintln!("{} {}", "✗".red(), msg.red());
        }
        std::process::exit(1);
    }
}
