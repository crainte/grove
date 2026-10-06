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

# Remove a worktree (refuses unless its work is merged; --force overrides)
g rm feature/auth

# Merge a worktree (--no-ff), git-style: the name is the source
g merge feature/auth                # into the branch checked out here
g merge --into main                 # this worktree into main
g merge feature/auth --into main    # from anywhere
g merge feature/auth --rm           # and remove the worktree afterwards

# Drop entries whose directories are gone
g prune

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

### Merging

`grove merge` follows `git merge`: a name you pass is always the **source**,
and `--into` is always the **target**. With neither, it fails and suggests
`grove merge --into <base>` using the branch the worktree was created from.

The merge runs in whichever worktree has the target checked out, which need
not be the one you are in. It refuses if either side has uncommitted changes.
On a conflict (or a rejected commit message) the merge is left in progress
for you to finish or `git merge --abort`, and `--rm` is skipped.

### Removing

`grove rm` checks before touching anything that the branch's work exists
elsewhere: reachable from HEAD, its upstream, or its base, or with its content
already on the base (cherry-picked or squashed). Otherwise it refuses and
suggests `--force`.

### Machine-readable output

The global `--porcelain` flag replaces the decorated rendering with
tab-separated records on stdout, for scripts and tools:

```bash
$ grove --porcelain list
repo	/home/you/project	main
wt	-	main	/home/you/project	-	pc	0	2	origin/main
wt	1	feature/auth	/home/you/project/.wt/1	-	mu	2	0	main
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

# Where new worktrees go (default: ".wt"), relative to the repo root.
# Absolute paths are used as-is; ids are per repo, so don't share one
# absolute dir between repos.
dir = ".wt"

# Merge commit message for `grove merge` (-m overrides)
[merge]
message = "chore: merge {{branch}} into {{target}}"

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

The merge message supports `{{branch}}` (source), `{{target}}`, `{{id}}`, and
`{{repo}}`. The default is a conventional commit so commit-msg hooks that
enforce one accept it.

## Storage Layout

Worktrees live in `.wt/` with short IDs, keeping them inside your repo tree so
config files are inherited. grove adds `/.wt/` to `.git/info/exclude` (local,
untracked) so they never show up in `git status` or get committed:

```
repo/
├── .git/
│   └── wt/
│       └── grove.db     # worktree metadata
├── .wt/
│   ├── 1/               # feature/auth
│   └── 2/               # sub-task (child of 1)
├── .grove.toml          # local config
├── .mise.toml           # inherited by all worktrees
└── src/
```

Earlier versions stored worktrees in `.git/wt/<id>`, where tools that skip
paths containing `.git` (Vite/Vitest, file watchers) misbehave. Existing
worktrees there keep working; new ones go to `.wt/`.

## License

MIT
