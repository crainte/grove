# Grove Development Context

## Origin Story

Grove is a rewrite of the bash `wt` function from `~/.config/dotfiles/worktree.sh`. The bash version grew organically and hit limitations that warranted a proper rewrite in Rust.

### Why Rust?

- **Simple distribution** — single binary, no runtime dependencies
- **Planned Claude Code integration** — needs to be robust and predictable
- **User preference** — distributable tooling

## Key Problems Solved

### 1. Config Inheritance (mise.toml, .env)

**Original problem**: Worktrees stored in `~/.worktrees/{repo}/{name}` couldn't inherit `.mise.toml` from the repo root.

**Solution**: Store worktrees in `{repo}/.git/wt/{id}/`. Since they're inside the repo directory tree, config files are inherited naturally.

### 2. Nested Worktrees Without Visible Folders

**Original problem**: With bash script, creating a child worktree `sub` from inside worktree `test` would create `.git/wt/test/sub/` — but this made `sub/` visible as a folder inside `test`'s working tree.

**Solution (bash attempt)**: Use `--` delimiter for flat storage: `.git/wt/test--sub/`. But this polluted branch names.

**Solution (grove)**: Decouple storage from naming entirely:
- Storage uses short IDs: `.git/wt/a1/`, `.git/wt/b2/`
- Metadata file tracks: branch name, parent relationship, timestamps
- Any branch name works, no encoding needed

### 3. Branch Names with Special Characters

**Original problem**: Branch names like `feature/auth` would create nested directories, breaking flat storage assumptions.

**Bash attempts**:
1. Replace `/` with `_` — but then need to escape all special chars
2. User rejected partial escaping as a rabbit hole

**Solution (grove)**: Metadata-based approach means branch names are just strings in JSON. No filesystem encoding needed.

### 4. Shell Integration (cd)

**Problem**: A binary can't change the shell's working directory.

**Solution**: The `eval "$(grove init bash)"` pattern (used by zoxide, z, autojump):
1. Binary outputs `__grove_cd:/path/to/worktree` for navigation
2. Shell wrapper function intercepts this prefix and runs `cd`
3. All other output passes through normally

## Architecture

```
grove/
├── src/
│   ├── main.rs      # Entry point
│   ├── cli.rs       # Clap command structure
│   ├── commands.rs  # Command implementations (stubs)
│   ├── meta.rs      # Metadata handling (implemented, tested)
│   └── shell.rs     # Shell integration scripts
├── SPEC.md          # Full specification
├── CLAUDE.md        # This file
└── Cargo.toml
```

### Metadata Storage

`.git/wt/meta.json`:
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
    }
  },
  "next_id": 3
}
```

### ID Generation

Base36 incrementing: `1`, `2`, ... `9`, `a`, `b`, ... `z`, `10`, ...
- Short (1-2 chars for typical usage)
- Human-readable
- Predictable order

## Current State

### Implemented & Tested
- `meta.rs` — Metadata struct, serialization, ID generation, tree operations
- `shell.rs` — Shell wrapper output for bash/zsh/fish
- `cli.rs` — Full command structure with clap

### Stubbed (TODO)
- All commands in `commands.rs` — need implementation
- Integration tests with real git repos

## Commands Reference

| Command | Description |
|---------|-------------|
| `grove <name> [base]` | Go to worktree (create if needed) |
| `grove go <name> [base]` | Explicit go |
| `grove add <name> [base]` | Create without switching |
| `grove rm <name>` | Remove worktree and branch |
| `grove list` / `grove ls` | Show worktree tree |
| `grove prune` | Clean stale references |
| `grove clean [branch]` | Remove merged worktrees |
| `grove done` | cd to main, pull, clean |
| `grove pull [paths...]` | Copy ignored files from main |
| `grove push [paths...]` | Copy ignored files to main |
| `grove path <name>` | Print path to worktree |
| `grove init <shell>` | Output shell wrapper |

## Configuration

Via git config:
```bash
git config grove.copyignored true           # Auto-copy gitignored files
git config --add grove.hook "mise trust"    # Post-create hooks
```

## Testing Strategy

- **Unit tests**: In each module (`meta.rs` has them)
- **Integration tests**: Use `tempfile` + `assert_cmd` for full workflows
- **TDD approach**: Write tests first, then implement

## Related Files

- Original bash script: `~/.config/dotfiles/worktree.sh` (still functional, grove is the successor)
- Spec document: `./SPEC.md`

## Next Steps

1. Implement `commands::go()` — the core command
2. Add git operations (worktree add/remove, branch operations)
3. Implement `list` with tree rendering
4. Integration tests
5. Remaining commands (clean, done, pull/push, etc.)

## Design Decisions Log

| Decision | Rationale |
|----------|-----------|
| Store in `.git/wt/` | Inherit repo config files (mise.toml) |
| Metadata in JSON | Proper escaping, extensible, readable |
| Short base36 IDs | Compact paths, avoid encoding issues |
| `__grove_cd:` prefix | Shell-agnostic navigation protocol |
| Separate branch name from storage | Any valid git branch name works |
| Context-aware lookup | `grove sub` from `test` finds child first |
