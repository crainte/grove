# Grove

A fast, simple git worktree manager.

## Why Grove?

Git worktrees are great for working on multiple branches simultaneously, but managing them is clunky. Grove makes it seamless:

- **Clean branch names** - no encoding restrictions, any valid git branch works
- **Hierarchical worktrees** - create child worktrees that inherit context
- **Config inheritance** - `.mise.toml`, `.env`, etc. inherited from repo root
- **Single binary** - no runtime dependencies

## Installation

```bash
mise use -g github:crainte/grove

# With cargo
cargo install --git https://github.com/crainte/grove

# Or download from releases
```

### Shell Integration

Add to your shell config:

```bash
# ~/.bashrc or ~/.zshrc
eval "$(grove init bash)"  # or zsh

# ~/.config/fish/config.fish
grove init fish | source
```

This sets up:
- **`g`** - short alias for grove with tab completion
- Directory changing when switching worktrees

## Usage

```bash
# Go to a worktree (creates if needed)
g feature/auth

# Create without switching
g add feature/auth

# Create from a specific base branch
g feature/auth main

# List all worktrees
g ls

# Remove a worktree
g rm feature/auth

# Clean up merged worktrees
g clean

# Copy ignored files (.env, etc.) from main worktree
g pull

# Print path to a worktree
g path feature/auth
```

### Hierarchical Worktrees

Create child worktrees from within a parent:

```bash
g feature/auth          # create parent
g sub-task              # creates child of feature/auth
g ../other-feature      # go up and create sibling
```

Context-aware lookup finds children first - `g sub-task` from within `feature/auth` finds the child before any top-level `sub-task`.

## Storage Layout

Worktrees live in `.git/wt/` with short IDs, keeping them inside your repo tree so config files are inherited:

```
repo/
├── .git/
│   └── wt/
│       ├── meta.json    # worktree metadata
│       ├── a1/          # feature/auth
│       └── b2/          # sub-task (child of a1)
├── .mise.toml           # inherited by all worktrees
└── src/
```

## License

MIT
