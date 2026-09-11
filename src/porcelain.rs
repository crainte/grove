//! Machine-readable TSV output.
//!
//! When `--porcelain` is set, grove writes tab-separated records to stdout
//! instead of the decorated human rendering. The format is a stable protocol
//! (see SPEC.md); treat any change to a record's arity or field order as
//! breaking.
//!
//! Two invariants keep the stream parseable:
//!
//! 1. Records go to **stdout** and nothing else does. Human notes must use the
//!    [`note!`] macro, never a bare `eprintln!`, or they will vanish in pretty
//!    mode and corrupt nothing — but a bare `println!` *will* corrupt the
//!    stream.
//! 2. Child processes must not inherit stdout in porcelain mode. See
//!    `config::run_hook_command`.

use std::sync::atomic::{AtomicBool, Ordering};

/// Set once from `main` before any command runs; read-only thereafter.
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set(value: bool) {
    ENABLED.store(value, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Placeholder for an absent optional field.
pub const NONE: &str = "-";

/// Escape a field so it cannot introduce a spurious field or record boundary.
fn escape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for c in field.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// Emit one record. No-op unless porcelain mode is active, so call sites do not
/// need to branch.
pub fn record(fields: &[&str]) {
    if !enabled() {
        return;
    }
    let escaped: Vec<String> = fields.iter().map(|f| escape(f)).collect();
    println!("{}", escaped.join("\t"));
}

/// Emit an error record on stderr. Errors stay off stdout so a failed command
/// never yields a half-parsed record stream.
pub fn error(msg: &str) {
    eprintln!("error\t{}", escape(msg));
}

/// Human-facing note. Suppressed entirely in porcelain mode.
///
/// Every `eprintln!` in a command path should be a `note!` instead; the
/// exceptions are `main`'s top-level error handler and [`error`] above.
#[macro_export]
macro_rules! note {
    ($($arg:tt)*) => {
        if !$crate::porcelain::enabled() {
            eprintln!($($arg)*);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_field_separators() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape("has\ttab"), "has\\ttab");
        assert_eq!(escape("has\nnewline"), "has\\nnewline");
        assert_eq!(escape("has\\backslash"), "has\\\\backslash");
    }

    /// A literal backslash-t must not be confused with an escaped tab after a
    /// round trip, which is why the backslash itself is escaped first.
    #[test]
    fn escaping_is_unambiguous() {
        assert_eq!(escape("literal\\t"), "literal\\\\t");
        assert_ne!(escape("literal\\t"), escape("literal\t"));
    }
}
