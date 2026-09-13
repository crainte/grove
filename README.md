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

### Machine-readable output

The global `--porcelain` flag replaces the decorated rendering with
tab-separated records on stdout, for scripts and tools:

```bash
$ grove --porcelain list
repo	/home/you/project	main
wt	-	main	/home/you/project	-	pc	0	2	origin/main
wt	1	feature/auth	/home/you/project/.git/wt/1	-	mu	2	0	main
```

Each `wt` record is `id`, `branch`, `path`, `parent-id`, `flags`, `ahead`,
`behind`, and the ref those counts were measured against. Flags are `p` primary,
`c` current, `m` modified, `u` untracked, `x` missing, `o` orphan; `-` means
none. Absent optional fields are `-`.

Errors become `error\t<message>` on stderr, leaving stdout clean. Navigation is
a `cd\t<path>` record rather than the `__grove_cd:` shell prefix. See
[SPEC.md](SPEC.md#porcelain-output) for the full record list.

## Configuration

Grove uses TOML config files:

- **Global**: `~/.config/grove/config.toml`
- **Local**: `.grove.toml` in repo root (takes precedence)

```toml
# .grove.toml

# Copy matching .gitignored files into new worktrees
copy = [".env*", ".terraform/", ".mise.local.toml"]

# Or copy every .gitignored file (default: false).
# When true, this supersedes `copy`.
copyignored = true

# Hooks - blocks run sequentially, tasks within a block run in parallel
[[hooks.post-create]]
trust = "mise trust {{path}}"
deps = "npm ci"

[[hooks.pre-remove]]
backup = "cp -r {{path}}/data {{repo}}/backup/"
```

Only `.gitignored` files are ever copied - tracked files come from git itself.
`copy` patterns accept exact names (`.env`), globs (`.env*`), and directories
(`.terraform/`). Local `.grove.toml` `copy` patterns extend the global list,
while `copyignored` is overridden outright by the more local config.

### Hook Types

| Hook | When | Runs in |
|------|------|---------|
| `post-create` | After worktree created, before cd | Current worktree |
| `pre-remove` | Before worktree removed | Worktree being removed |

### Template Variables

- `{{path}}` - worktree path
- `{{branch}}` - branch name  
- `{{id}}` - worktree ID
- `{{repo}}` - repo root path

## Storage Layout

Worktrees live in `.git/wt/` with short IDs, keeping them inside your repo tree so config files are inherited:

```
repo/
├── .git/
│   └── wt/
│       ├── grove.db     # worktree metadata
│       ├── a1/          # feature/auth
│       └── b2/          # sub-task (child of a1)
├── .grove.toml          # local config
├── .mise.toml           # inherited by all worktrees
└── src/
```

## License

MIT
