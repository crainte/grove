# Grove — Git Worktree Manager

A fast, simple git worktree manager written in Rust.

## Design Goals

1. **Simple distribution** — single binary, no runtime dependencies
2. **Clean branch names** — no encoding restrictions, any valid git branch name works
3. **Flat storage** — worktrees live in `.git/wt/<id>/`, hierarchy in metadata
4. **Shell integration** — `eval "$(grove init bash)"` for seamless `cd`
5. **Config inheritance** — `.mise.toml`, `.env` inherited from repo root

## Storage Layout

```
repo/
├── .git/
│   └── wt/
│       ├── meta.json        # worktree metadata
│       ├── a1/              # worktree directory (short id)
│       ├── b2/
│       └── c3/
├── .mise.toml               # inherited by worktrees
└── src/
```

### meta.json

```json
{
  "version": 1,
  "worktrees": {
    "a1": {
      "branch": "feature/auth",
      "parent": null,
      "created": "2024-01-15T10:30:00Z"
    },
    "b2": {
      "branch": "sub-task",
      "parent": "a1",
      "created": "2024-01-15T11:00:00Z"
    },
    "c3": {
      "branch": "refactor/cleanup",
      "parent": "a1",
      "created": "2024-01-15T12:00:00Z"
    }
  },
  "next_id": 4
}
```

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
grove list              # Show worktree tree
grove prune             # Clean stale references
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

Stored in git config:

```bash
git config grove.copyignored true     # Auto-copy gitignored files
git config --add grove.hook "mise trust"  # Post-create hooks
```

## Shell Integration Protocol

The binary outputs to stdout. Special prefix `__grove_cd:` signals navigation:

```
__grove_cd:/path/to/worktree
```

The shell wrapper intercepts this and runs `cd`. All other output passes through.

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

## Error Handling

- Missing worktree → suggest `grove add`
- Branch exists → offer to checkout existing or pick new name
- Dirty worktree on rm → warn, require `--force`

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
