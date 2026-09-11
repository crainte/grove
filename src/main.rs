mod cli;
mod commands;
mod config;
mod copyfiles;
mod git;
mod meta;
mod porcelain;
mod shell;

use clap::Parser;
use colored::Colorize;
use std::env;
use std::io::IsTerminal;

fn main() {
    let cli = cli::Cli::parse();

    porcelain::set(cli.porcelain);

    // Force colors if stderr is a TTY (colored crate only checks stdout).
    // Porcelain output must stay plain regardless of where it is attached.
    if cli.porcelain {
        colored::control::set_override(false);
    } else if std::io::stderr().is_terminal() {
        colored::control::set_override(true);
    }

    if let Some(ref dir) = cli.directory
        && let Err(e) = env::set_current_dir(dir)
    {
        fail(&format!("Failed to change to {}: {}", dir.display(), e));
    }

    if let Err(e) = cli.run() {
        fail(&format!("{}", e));
    }
}

/// Report a fatal error and exit. This is the only place errors are styled;
/// everything below propagates via `anyhow`.
fn fail(msg: &str) -> ! {
    if porcelain::enabled() {
        porcelain::error(msg);
    } else if msg.contains("Use --force") || msg.contains("Use -f") {
        // Warnings are recoverable (user can --force, etc.)
        eprintln!("{} {}", "⚠".yellow(), msg.yellow());
    } else {
        eprintln!("{} {}", "✗".red(), msg.red());
    }
    std::process::exit(1);
}
