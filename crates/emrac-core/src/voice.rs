//! emrac's voice: one verb for each kind of statement, so the verb tells the
//! reader how sure emrac is and what to do with the line. See
//! `docs/plan/r2/u6-voice.md`. Nothing outside this module builds an
//! `emrac <word>:` prefix by hand (a test scans the sources for that).

use std::fmt;
use std::io::{IsTerminal, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Voice {
    /// Something broke underneath emrac; not the user's doing.
    Says,
    /// A fact emrac discovered that the user can act on.
    Found,
    /// Plain information, nothing to decide.
    Notes,
    /// What it is about to do.
    Plans,
    /// A question; the answer decides what happens.
    Asks,
    /// A real risk, or something that could hurt.
    Warns,
    /// A conclusion from weak or indirect evidence; may be wrong.
    Suspects,
    /// A judgment with its reasons, from good evidence that is not proof.
    Thinks,
    /// The next step, usually a command to run.
    Suggests,
    /// A deliberate refusal, with no override.
    Refuses,
    /// Something was checked, or done, and holds.
    Confirms,
    /// Something recalled from what the user did before.
    Remembers,
}

impl Voice {
    pub const ALL: [Voice; 12] = [
        Voice::Says,
        Voice::Found,
        Voice::Notes,
        Voice::Plans,
        Voice::Asks,
        Voice::Warns,
        Voice::Suspects,
        Voice::Thinks,
        Voice::Suggests,
        Voice::Refuses,
        Voice::Confirms,
        Voice::Remembers,
    ];

    /// The verb: `thinks`.
    pub fn word(self) -> &'static str {
        match self {
            Voice::Says => "says",
            Voice::Found => "found",
            Voice::Notes => "notes",
            Voice::Plans => "plans",
            Voice::Asks => "asks",
            Voice::Warns => "warns",
            Voice::Suspects => "suspects",
            Voice::Thinks => "thinks",
            Voice::Suggests => "suggests",
            Voice::Refuses => "refuses",
            Voice::Confirms => "confirms",
            Voice::Remembers => "remembers",
        }
    }

    /// Problems and doubts go to standard error, results and information to
    /// standard output.
    pub fn to_stderr(self) -> bool {
        matches!(
            self,
            Voice::Says | Voice::Warns | Voice::Suspects | Voice::Refuses
        )
    }

    /// The ANSI colour (SGR code) of the verb, if it has one.
    pub fn color(self) -> Option<&'static str> {
        match self {
            Voice::Says | Voice::Refuses => Some("31"),
            Voice::Warns => Some("33"),
            Voice::Suspects => Some("35"),
            Voice::Thinks => Some("36"),
            Voice::Suggests | Voice::Confirms => Some("32"),
            Voice::Asks => Some("34"),
            Voice::Notes | Voice::Remembers => Some("2"),
            Voice::Plans | Voice::Found => None,
        }
    }

    /// `emrac thinks:`, with the verb coloured when `color` is set.
    pub fn prefix(self, color: bool) -> String {
        format!("emrac {}:", self.styled(color))
    }

    /// `thinks:` — for indented lines under a heading, where repeating the
    /// program name would be noise.
    pub fn short(self, color: bool) -> String {
        format!("{}:", self.styled(color))
    }

    fn styled(self, color: bool) -> String {
        match (color, self.color()) {
            (true, Some(code)) => format!("\x1b[{code}m{}\x1b[0m", self.word()),
            _ => self.word().to_string(),
        }
    }

    /// `thinks:` as it should look on `stderr` or `stdout` right now.
    pub fn short_for(self, stderr: bool) -> String {
        self.short(use_color(stderr))
    }

    /// The prefix as it should look on `stderr` or `stdout` right now.
    pub fn prefix_for(self, stderr: bool) -> String {
        self.prefix(use_color(stderr))
    }
}

impl fmt::Display for Voice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.prefix(false))
    }
}

/// Colour only on a terminal, never when `NO_COLOR` is set, never on a dumb one.
pub fn color_allowed(is_terminal: bool, no_color: Option<&str>, term: Option<&str>) -> bool {
    is_terminal && no_color.is_none_or(str::is_empty) && term != Some("dumb")
}

fn use_color(stderr: bool) -> bool {
    let is_terminal = if stderr {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    };
    color_allowed(
        is_terminal,
        std::env::var("NO_COLOR").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
    )
}

/// Prints `emrac <verb>: <text>` on the verb's stream.
pub fn emit(voice: Voice, text: fmt::Arguments<'_>) {
    emit_on(voice, voice.to_stderr(), text);
}

/// Like `emit`, on a stream the caller chooses (a `found` error goes to
/// standard error although a `found` heading goes to standard output).
pub fn emit_on(voice: Voice, stderr: bool, text: fmt::Arguments<'_>) {
    let line = format!("{} {text}", voice.prefix_for(stderr));
    // Ignored: a closed pipe (`emrac … | head`) must not become a panic.
    let _ = if stderr {
        writeln!(std::io::stderr(), "{line}")
    } else {
        writeln!(std::io::stdout(), "{line}")
    };
}

/// `say!(Voice::Notes, "removed {}", n)` prints `emrac notes: removed 3`.
#[macro_export]
macro_rules! say {
    ($voice:expr, $($arg:tt)*) => {
        $crate::voice::emit($voice, ::std::format_args!($($arg)*))
    };
}

/// `say_err!(voice, …)` always prints on standard error (an error report whose
/// verb is `found` would otherwise go to standard output).
#[macro_export]
macro_rules! say_err {
    ($voice:expr, $($arg:tt)*) => {
        $crate::voice::emit_on($voice, true, ::std::format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `.rs` file under `crates/*/src`, with its path.
    fn sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    out.push((path.display().to_string(), text));
                }
            }
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut out = Vec::new();
        for krate in ["emrac-core", "emrac-cli"] {
            walk(&crates.join(krate).join("src"), &mut out);
        }
        assert!(out.len() > 20, "found only {} source files", out.len());
        out
    }

    #[test]
    fn nothing_builds_an_emrac_word_prefix_by_hand() {
        let re_words: Vec<String> = Voice::ALL
            .iter()
            .map(|v| format!("\"emrac {}:", v.word()))
            .collect();
        let mut offenders = Vec::new();
        for (path, text) in sources() {
            if path.ends_with("emrac-core/src/voice.rs") {
                continue;
            }
            for (i, line) in text.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                if re_words.iter().any(|w| line.contains(w.as_str())) {
                    offenders.push(format!("{path}:{}: {}", i + 1, code));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "write messages with say!(Voice::…, …), not by hand:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn facts_and_confirmations_never_hedge() {
        // Only suspects and thinks may say "probably" and the like.
        const HEDGES: [&str; 8] = [
            "probably",
            "looks like",
            "maybe",
            "perhaps",
            "possibly",
            "might",
            "seems",
            "may be",
        ];
        let mut offenders = Vec::new();
        for (path, text) in sources() {
            if path.ends_with("emrac-core/src/voice.rs") {
                continue;
            }
            let mut rest = text.as_str();
            while let Some(at) = rest.find("Voice::") {
                rest = &rest[at + "Voice::".len()..];
                let verb: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
                if !["Says", "Found", "Confirms"].contains(&verb.as_str()) {
                    continue;
                }
                // The string literal that follows, if the next token is one.
                let after = rest[verb.len()..].trim_start_matches([',', ' ', '\n']);
                if let Some(lit) = after.strip_prefix('"') {
                    let message: String = lit.chars().take_while(|c| *c != '"').collect();
                    let lower = message.to_lowercase();
                    if let Some(h) = HEDGES.iter().find(|h| lower.contains(**h)) {
                        offenders.push(format!("{path}: {verb} says \"{message}\" ({h})"));
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "{}", offenders.join("\n"));
    }

    #[test]
    fn every_voice_has_its_own_word_and_a_plain_prefix() {
        let words: Vec<&str> = Voice::ALL.iter().map(|v| v.word()).collect();
        let mut unique = words.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 12, "{words:?}");
        for v in Voice::ALL {
            assert_eq!(v.prefix(false), format!("emrac {}:", v.word()));
            assert_eq!(v.short(false), format!("{}:", v.word()));
            assert_eq!(v.to_string(), v.prefix(false));
        }
    }

    #[test]
    fn problems_and_doubts_go_to_stderr_and_results_to_stdout() {
        for v in [Voice::Says, Voice::Warns, Voice::Suspects, Voice::Refuses] {
            assert!(v.to_stderr(), "{v:?}");
        }
        for v in [
            Voice::Found,
            Voice::Notes,
            Voice::Plans,
            Voice::Asks,
            Voice::Thinks,
            Voice::Suggests,
            Voice::Confirms,
            Voice::Remembers,
        ] {
            assert!(!v.to_stderr(), "{v:?}");
        }
    }

    #[test]
    fn only_the_verb_is_coloured_and_only_when_asked() {
        assert_eq!(Voice::Warns.prefix(true), "emrac \x1b[33mwarns\x1b[0m:");
        assert_eq!(Voice::Warns.prefix(false), "emrac warns:");
        assert_eq!(
            Voice::Found.prefix(true),
            "emrac found:",
            "found has no colour"
        );
        assert_eq!(Voice::Thinks.short(true), "\x1b[36mthinks\x1b[0m:");
    }

    #[test]
    fn colour_needs_a_terminal_and_respects_no_color_and_dumb_terminals() {
        assert!(color_allowed(true, None, Some("xterm-256color")));
        assert!(
            color_allowed(true, Some(""), None),
            "an empty NO_COLOR is unset"
        );
        assert!(!color_allowed(false, None, Some("xterm")), "piped");
        assert!(!color_allowed(true, Some("1"), Some("xterm")), "NO_COLOR");
        assert!(!color_allowed(true, None, Some("dumb")));
    }
}
