use emrac_core::{say, Voice};
use std::io::{self, Write};

/// Reads one line from standard input **one byte at a time**.
///
/// `std::io::stdin()` reads in blocks, so with piped input (`printf 'y\ny\n' |
/// emrac …`) the first prompt would swallow every line and leave nothing for
/// the pacman that emrac starts afterwards. Reading byte by byte takes exactly
/// one line. `None` means end of input with nothing typed (no terminal, a
/// closed pipe) or a read error: there is no answer to act on.
fn read_line() -> Option<String> {
    let mut line = Vec::new();
    let mut byte = 0u8;
    loop {
        // SAFETY: reading one byte into a valid, writable one-byte buffer.
        let n = unsafe { libc::read(0, (&mut byte as *mut u8).cast(), 1) };
        match n {
            1 if byte == b'\n' => break,
            1 => line.push(byte),
            0 => {
                if line.is_empty() {
                    return None;
                }
                break;
            }
            _ => {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return None;
            }
        }
    }
    Some(String::from_utf8_lossy(&line).into_owned())
}

/// Turns what was typed into a decision. **No answer at all (end of input) is
/// never consent**, whatever the default: an empty *line* takes the default,
/// but a closed terminal or pipe must not install anything.
pub(crate) fn decide(line: Option<&str>, default_yes: bool) -> bool {
    let Some(line) = line else {
        return false;
    };
    match line.trim().to_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        _ => false,
    }
}

/// The start of a question: `emrac asks:`.
fn asks() -> String {
    Voice::Asks.prefix_for(false)
}

/// When no answer came at all (the input is closed), say so once instead of
/// silently changing nothing. The unanswered question left the cursor mid-line.
fn no_answer() {
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        println!();
        say!(
            Voice::Refuses,
            "no answer came (the input is closed), so nothing was changed."
        );
    }
}

fn ask(message: &str, hint: &str, default_yes: bool) -> bool {
    print!("{} {message} {hint} ", asks());
    if io::stdout().flush().is_err() {
        return false;
    }
    let line = read_line();
    if line.is_none() {
        no_answer();
    }
    decide(line.as_deref(), default_yes)
}

/// Prompts on stdout/stdin; an empty answer means yes (matches pacman's own
/// `[Y/n]` convention). No answer at all — end of input, a read failure — is
/// a no.
pub fn confirm(message: &str) -> bool {
    ask(message, "[Y/n]", true)
}

/// Like `confirm`, but an empty answer means no: for decisions where
/// proceeding must be a deliberate act.
pub fn confirm_default_no(message: &str) -> bool {
    ask(message, "[y/N]", false)
}

/// Asks which of `options` (single letters) to take. An empty answer, an
/// unknown one and end of input all mean "none": nothing is changed.
pub fn choose(message: &str, options: &str) -> Option<char> {
    print!("{} {message} ", asks());
    io::stdout().flush().ok()?;
    let line = read_line();
    if line.is_none() {
        no_answer();
    }
    pick(line.as_deref(), options)
}

pub(crate) fn pick(line: Option<&str>, options: &str) -> Option<char> {
    let c = line?.trim().chars().next()?.to_ascii_lowercase();
    options.contains(c).then_some(c)
}

/// The user must type `expected` exactly: for changes that can lock them out.
pub fn confirm_typed(message: &str, expected: &str) -> bool {
    print!("{} {message} ", asks());
    if io::stdout().flush().is_err() {
        return false;
    }
    match read_line() {
        Some(line) => line.trim() == expected,
        None => {
            no_answer();
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_line_takes_the_default() {
        assert!(decide(Some(""), true));
        assert!(decide(Some("  \t"), true));
        assert!(!decide(Some(""), false));
    }

    #[test]
    fn yes_in_any_case_and_with_spaces_is_yes_either_way() {
        for yes in ["y", "Y", "yes", "YES", " Yes "] {
            assert!(decide(Some(yes), true), "{yes}");
            assert!(decide(Some(yes), false), "{yes}");
        }
    }

    #[test]
    fn anything_else_is_no_even_when_the_default_is_yes() {
        for no in ["n", "N", "no", "nope", "yy", "yes please", "0", "q"] {
            assert!(!decide(Some(no), true), "{no}");
            assert!(!decide(Some(no), false), "{no}");
        }
    }

    #[test]
    fn a_choice_needs_a_listed_letter_and_nothing_else_selects() {
        assert_eq!(pick(Some("t"), "tks"), Some('t'));
        assert_eq!(pick(Some(" K "), "tks"), Some('k'));
        assert_eq!(pick(Some("take"), "tks"), Some('t'));
        for none in [Some(""), Some("x"), Some("m"), None] {
            assert_eq!(pick(none, "tks"), None, "{none:?}");
        }
    }

    #[test]
    fn end_of_input_is_never_consent_whatever_the_default() {
        // This used to be treated as an empty answer, i.e. "yes" for [Y/n]:
        // `emrac install foo < /dev/null` installed without anyone agreeing.
        assert!(!decide(None, true));
        assert!(!decide(None, false));
    }
}
