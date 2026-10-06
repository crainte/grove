# Grove — Git Worktree Manager

A fast, simple git worktree manager written in Rust.

## Design Goals

1. **Simple distribution** — single binary, no runtime dependencies
2. **Clean branch names** — no encoding restrictions, any valid git branch name works
3. **Flat storage** — worktrees live in `.wt/<id>/`, hierarchy in metadata
4. **Shell integration** — `eval "$(grove init bash)"` for seamless `cd`
5. **Config inheritance** — `.mise.toml`, `.env` inherited from repo root

## Storage Layout

```
repo/
├── .git/
│   ├── info/exclude         # grove adds /.wt/ here
│   └── wt/
│       └── grove.db         # worktree metadata (SQLite)
├── .wt/                     # configurable with `dir`
│   ├── 1/                   # worktree directory (short id)
│   ├── 2/
│   └── 3/
├── .mise.toml               # inherited by worktrees
└── src/
```

The worktree directory is inside the repo so config files are inherited, and
outside `.git/` because many tools (Vite/Vitest, watchers, linters) skip any
path containing `.git`. It is excluded through `.git/info/exclude`, which is
local and never produces a diff in the tracked `.gitignore`.

### grove.db

`worktrees(id, branch, parent, created, path, base)`, schema version in
`PRAGMA user_version`.

- `path` — absolute worktree path. `NULL` for rows created before v1, which
  resolve to the legacy `.git/wt/<id>`. Changing `dir` only affects new
  worktrees.
- `base` — the branch the worktree was created from. For rows without one,
  grove falls back to the grove parent's branch, then the branch reflog
  (`branch: Created from X`), then the default branch.

## Commands

### Navigation & Creation

```bash
grove <name> [base]     # Go to worktree (create if needed)
grove go <name> [base]  # Explicit go
grove add <name> [base] # Create without switching
```

### Management

```bash
grove rm <name>         # Remove worktree and branch
grove merge [src] [--into tgt] [-m msg] [--rm]  # Merge src into tgt (--no-ff)
grove list              # Show worktree tree
grove prune             # git worktree prune, then drop rows whose dir is gone
grove clean [branch]    # Remove merged worktrees
grove done              # cd to main, pull, clean
```

### Sync (gitignored files)

```bash
grove pull [paths...]   # Copy ignored files from main
grove push [paths...]   # Copy ignored files to main
```

### Shell Integration

```bash
grove init bash         # Output bash wrapper function
grove init zsh          # Output zsh wrapper function
grove init fish         # Output fish wrapper function
```

### Utility

```bash
grove path <name>       # Print path to worktree
grove config            # Show/set configuration
```

## Configuration

TOML files, local overriding global:

- **Global**: `~/.config/grove/config.toml`
- **Local**: `.grove.toml` in repo root

```toml
# Copy matching .gitignored files into new worktrees
copy = [".env*", ".terraform/", ".mise.local.toml"]

# Or copy every .gitignored file (default: false, supersedes `copy`)
copyignored = true

# Directory for new worktrees (default ".wt"; relative to repo root)
dir = ".wt"

# grove merge commit message; {{branch}} {{target}} {{id}} {{repo}}
[merge]
message = "chore: merge {{branch}} into {{target}}"

# Hooks: blocks run sequentially, tasks within a block run in parallel
[[hooks.post-create]]
trust = "mise trust {{path}}/mise.toml"
deps = "npm ci"

[[hooks.pre-remove]]
backup = "cp -r {{path}}/data {{repo}}/backup/"
```

`copy` patterns from the local config extend the global list; `copyignored`,
`dir`, and `merge.message` are overridden outright by the more local config.
Ignored nested repositories and worktrees (e.g. siblings under `.wt/`) are
never copied. Hooks support the template
variables `{{path}}`, `{{branch}}`, `{{id}}`, and `{{repo}}`.

## Shell Integration Protocol

The binary outputs to stdout. Special prefix `__grove_cd:` signals navigation:

```
__grove_cd:/path/to/worktree
```

The shell wrapper intercepts this and runs `cd`. All other output passes through.

## Porcelain Output

`--porcelain` is a global flag that swaps the decorated rendering for
tab-separated records on **stdout**. It is a stable protocol: changing a
record's arity or field order is a breaking change.

Fields are escaped so a value can never introduce a field or record boundary:
`\` → `\\`, tab → `\t`, newline → `\n`, carriage return → `\r`. Absent
optional fields are `-`.

Errors go to stderr as `error\t<message>` with a nonzero exit; stdout stays
empty so a failed command never yields a half-parsed stream.

### Records

```
repo      <root-abspath>  <default-branch>
wt        <id>  <branch>  <abspath>  <parent-id>  <flags>  <ahead>  <behind>  <cmp-base>
cd        <abspath>
created   <id>  <branch>  <abspath>
removed   <id>  <branch>  <reason>          # explicit | merged:<ref> | stale
merged    <id>  <branch>  <target>          # id is - if source is not a grove worktree
orphaned  <id>  <branch>
skipped   <id>  <branch>  <reason>          # dirty | missing-branch
imported  <id>  <branch>
renamed   <id>  <old-branch>  <new-branch>
pruned
fetched
pulled
copied    <count>
copyfail  <relpath>  <reason>
path      <abspath>
```

`id` is `-` for the primary worktree; `parent-id` is `-` at top level.

`cmp-base` is the ref `ahead`/`behind` were measured against. It is not implied
by position: an upstream tracking branch wins if one exists, otherwise a child
compares against its parent's branch and a top-level worktree against the
default branch. Bare counts are ambiguous without it.

### Flags

| Char | Meaning |
|------|---------|
| `p` | Primary worktree (repo root) |
| `c` | Current (cwd is inside it) |
| `m` | Has modified tracked files |
| `u` | Has untracked files |
| `x` | Directory missing on disk |
| `o` | Orphan (parent was deleted) |

`-` when no flags apply.

### Per-command output

| Command | Records |
|---------|---------|
| `list` | `repo`, then `wt`* in tree order |
| `add` | `created`, `wt` |
| `go` | `wt`, `cd` — plus `created` if the worktree was new |
| `go` (no name) | Degrades to `list`; no fzf, no `cd` |
| `rm` | `removed`, `orphaned`*, `cd` if the cwd was inside it |
| `clean` | `removed`*, `skipped`*, `cd` if the current worktree went |
| `done` | `fetched`, `pulled`, then `clean` records, `cd` |
| `prune` | `removed`* (reason `stale`), `pruned` |
| `merge` | `merged`; with `--rm` also `removed`, `orphaned`*, `cd` if the cwd was inside the source |
| `sync` | `imported`*, `removed`*, `renamed`* |
| `pull` / `push` | `copied`, `copyfail`* |
| `path` | `path` |
| `init`, `complete` | Flag ignored; already machine output |

In porcelain mode `cd` replaces the `__grove_cd:` prefix, so consumers parse one
format rather than special-casing the shell protocol.

## ID Generation

Short incrementing base36 IDs: `1`, `2`, ... `a`, `b`, ... `z`, `10`, `11`, ...

- Compact (1-2 chars for typical usage)
- Predictable order
- Human-readable

## Nested Worktrees

Creating a worktree while inside another sets the parent:

```bash
cd repo
grove feature           # creates "feature", parent: null
grove sub               # from inside feature: creates "sub", parent: "feature"
```

The `list` command renders the tree:

```
📁 myrepo (/path/to/repo)
├─ ○ main
└─ ● feature ← here
   ├─ ○ sub
   └─ ○ fix
```

## Context-Aware Lookup

When inside worktree `feature`, `grove sub` finds the child `sub` before a top-level `sub`.

## Merge

Git semantics: a positional name is always the source, `--into` always the
target. Nothing is implied.

| Invocation | Source → target |
|---|---|
| `grove merge` | Error; in a grove worktree the hint suggests `--into <base>` |
| `grove merge <src>` | `<src>` → branch checked out in the cwd |
| `grove merge --into <tgt>` | current worktree → `<tgt>` (error in the main worktree) |
| `grove merge <src> --into <tgt>` | `<src>` → `<tgt>`, from anywhere |

The merge is `git merge --no-ff -m <msg>` run in the worktree that has the
target checked out, so commit-msg hooks apply. It refuses when the target is
not checked out anywhere, when either side has tracked changes, when a merge
is already in progress there, or when source equals target. On failure the
merge state is left for the user and `--rm` is skipped.

## Error Handling

- Missing worktree → suggest `grove add`
- Branch exists → offer to checkout existing or pick new name
- Dirty worktree on rm → warn, require `--force`
- Unmerged branch on rm → refuse before removing anything, require `--force`.
  Merged means reachable from HEAD, its upstream, or its base, or content
  already on the base (cherry-pick/squash).

## Testing Strategy

### Unit Tests

- ID generation
- Metadata serialization/deserialization  
- Path sanitization
- Tree rendering

### Integration Tests

- Full command workflows in temp git repos
- Shell wrapper behavior
- Config inheritance verification

## Future Considerations

- `grove clone` — clone and immediately create worktree
- `grove stash` — stash changes before switching
- Fuzzy finder integration (`grove go` with fzf)
- Remote worktree sync
