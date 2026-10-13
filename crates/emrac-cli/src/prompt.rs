use std::io::{self, Write};

/// Prompts on stdout/stdin, defaulting to yes on an empty answer (matches
/// pacman's own `[Y/n]` convention). Returns `false` on any read failure
/// (e.g. no attached terminal) rather than assuming consent.
pub fn confirm(message: &str) -> bool {
    print!("emrac asks: {message} [Y/n] ");
    if io::stdout().flush().is_err() {
        return false;
    }

    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }

    let answer = answer.trim().to_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
}

/// Like `confirm`, but an empty answer means no: for decisions where
/// proceeding must be a deliberate act.
pub fn confirm_default_no(message: &str) -> bool {
    print!("emrac asks: {message} [y/N] ");
    if io::stdout().flush().is_err() {
        return false;
    }

    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }

    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}
