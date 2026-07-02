use anyhow::Result;

/// Output bash shell integration
pub fn init_bash() -> Result<()> {
    print!(
        r#"# Grove shell integration for Bash
# Add to ~/.bashrc: eval "$(grove init bash)"

# g - short alias for grove (handles cd)
g() {{
    local out
    out=$(command grove "$@")
    local exit_code=$?
    
    if [[ "$out" == __grove_cd:* ]]; then
        \builtin cd -- "${{out#__grove_cd:}}"
    elif [[ -n "$out" ]]; then
        \builtin printf '%s\n' "$out"
    fi
    
    return $exit_code
}}

# Get worktree branch names for completion
__grove_worktrees() {{
    command grove complete "$1" 2>/dev/null
}}

# Bash completion for g
_g_complete() {{
    local cur="${{COMP_WORDS[COMP_CWORD]}}"
    local prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    
    local commands="go add rm list ls prune clean done pull push path init sync help"
    
    case "${{prev}}" in
        go|path)
            # Complete with all worktree names (including main)
            COMPREPLY=( $(compgen -W "$(__grove_worktrees go)" -- "$cur") )
            ;;
        rm)
            # Complete with grove worktrees only (not main)
            COMPREPLY=( $(compgen -W "$(__grove_worktrees rm)" -- "$cur") )
            ;;
        add)
            # Complete with branch names (for base branch)
            local branches=$(git branch --format='%(refname:short)' 2>/dev/null)
            COMPREPLY=( $(compgen -W "${{branches}}" -- "$cur") )
            ;;
        g)
            # First argument - commands or worktree names (go creates if needed)
            COMPREPLY=( $(compgen -W "${{commands}} $(__grove_worktrees go)" -- "$cur") )
            ;;
        *)
            # Second argument - branch names (for base branch)
            local branches=$(git branch --format='%(refname:short)' 2>/dev/null)
            COMPREPLY=( $(compgen -W "${{branches}}" -- "$cur") )
            ;;
    esac
}}

complete -F _g_complete g
"#
    );
    
    Ok(())
}

/// Output zsh shell integration
pub fn init_zsh() -> Result<()> {
    print!(
        r#"# Grove shell integration for Zsh
# Add to ~/.zshrc: eval "$(grove init zsh)"

# g - short alias for grove (handles cd)
g() {{
    local out
    out=$(command grove "$@")
    local exit_code=$?
    
    if [[ "$out" == __grove_cd:* ]]; then
        \builtin cd -- "${{out#__grove_cd:}}"
    elif [[ -n "$out" ]]; then
        \builtin printf '%s\n' "$out"
    fi
    
    return $exit_code
}}

# Get worktree branch names for completion
__grove_worktrees() {{
    command grove complete "$1" 2>/dev/null
}}

# Zsh completion for g
_g_complete() {{
    local commands="go add rm list ls prune clean done pull push path init sync help"
    
    case "${{words[2]}}" in
        go|path)
            # All worktrees including main
            local worktree_names=(${{(f)"$(__grove_worktrees go)"}})
            compadd -a worktree_names
            ;;
        rm)
            # Only grove worktrees (not main)
            local worktree_names=(${{(f)"$(__grove_worktrees rm)"}})
            compadd -a worktree_names
            ;;
        add)
            local branches=(${{(f)"$(git branch --format='%(refname:short)' 2>/dev/null)"}})
            compadd -a branches
            ;;
        *)
            local worktree_names=(${{(f)"$(__grove_worktrees go)"}})
            compadd ${{=commands}}
            compadd -a worktree_names
            ;;
    esac
}}

compdef _g_complete g
"#
    );
    
    Ok(())
}

/// Output fish shell integration
pub fn init_fish() -> Result<()> {
    print!(
        r#"# Grove shell integration for Fish
# Add to ~/.config/fish/config.fish: grove init fish | source

# g - short alias for grove (handles cd)
function g
    set -l out (command grove $argv)
    set -l exit_code $status
    
    if string match -q '__grove_cd:*' $out
        builtin cd (string replace '__grove_cd:' '' $out)
    else if test -n "$out"
        printf '%s\n' $out
    end
    
    return $exit_code
end

# Get worktree branch names for completion
function __grove_worktrees
    command grove complete $argv[1] 2>/dev/null
end

# Completions for g
complete -c g -f -n '__fish_use_subcommand' -a 'go add rm list ls prune clean done pull push path init sync help'
complete -c g -f -n '__fish_use_subcommand' -a '(__grove_worktrees go)'
complete -c g -f -n '__fish_seen_subcommand_from go path' -a '(__grove_worktrees go)'
complete -c g -f -n '__fish_seen_subcommand_from rm' -a '(__grove_worktrees rm)'
complete -c g -f -n '__fish_seen_subcommand_from add' -a '(git branch --format="%(refname:short)" 2>/dev/null)'
"#
    );
    
    Ok(())
}

/// Prefix for cd output - shell wrapper intercepts this
pub const CD_PREFIX: &str = "__grove_cd:";

/// Output a cd command for the shell wrapper
pub fn output_cd(path: &std::path::Path) {
    println!("{}{}", CD_PREFIX, path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cd_prefix() {
        assert_eq!(CD_PREFIX, "__grove_cd:");
    }
}
