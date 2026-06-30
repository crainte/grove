use anyhow::Result;

/// Output bash shell integration
pub fn init_bash() -> Result<()> {
    print!(
        r#"# Grove shell integration for Bash
# Add to ~/.bashrc: eval "$(grove init bash)"

grove() {{
    local out
    out=$(command grove "$@")
    local exit_code=$?
    
    if [[ "$out" == __grove_cd:* ]]; then
        cd "${{out#__grove_cd:}}" || return 1
    elif [[ -n "$out" ]]; then
        printf '%s\n' "$out"
    fi
    
    return $exit_code
}}
"#
    );
    Ok(())
}

/// Output zsh shell integration  
pub fn init_zsh() -> Result<()> {
    print!(
        r#"# Grove shell integration for Zsh
# Add to ~/.zshrc: eval "$(grove init zsh)"

grove() {{
    local out
    out=$(command grove "$@")
    local exit_code=$?
    
    if [[ "$out" == __grove_cd:* ]]; then
        cd "${{out#__grove_cd:}}" || return 1
    elif [[ -n "$out" ]]; then
        printf '%s\n' "$out"
    fi
    
    return $exit_code
}}
"#
    );
    Ok(())
}

/// Output fish shell integration
pub fn init_fish() -> Result<()> {
    print!(
        r#"# Grove shell integration for Fish
# Add to ~/.config/fish/config.fish: grove init fish | source

function grove
    set -l out (command grove $argv)
    set -l exit_code $status
    
    if string match -q '__grove_cd:*' $out
        cd (string replace '__grove_cd:' '' $out)
    else if test -n "$out"
        printf '%s\n' $out
    end
    
    return $exit_code
end
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
