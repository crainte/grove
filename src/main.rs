mod cli;
mod commands;
mod git;
mod meta;
mod shell;
mod copyfiles;

use std::io::IsTerminal;
use clap::Parser;
use colored::Colorize;

fn main() {
    // Force colors if stderr is a TTY (colored crate only checks stdout)
    if std::io::stderr().is_terminal() {
        colored::control::set_override(true);
    }

    let cli = cli::Cli::parse();
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
