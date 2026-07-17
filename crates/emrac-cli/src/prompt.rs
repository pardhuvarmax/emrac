use std::io::{self, Write};

/// Prompts on stdout/stdin, defaulting to yes on an empty answer (matches
/// pacman's own `[Y/n]` convention). Returns `false` on any read failure
/// (e.g. no attached terminal) rather than assuming consent.
pub fn confirm(message: &str) -> bool {
    print!("{message} [Y/n] ");
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
