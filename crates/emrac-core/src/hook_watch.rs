//! A pacman hook that fails after the transaction was committed.
//!
//! pacman exits 0 then: the packages are installed and the hook's work (rebuilding
//! an initramfs, a cache, an index) was not done, and it says only `error: command
//! failed to execute correctly`. emrac listens to what pacman prints, remembers
//! which hook failed, and reports it after the transaction is recorded: the
//! transaction is committed, but the run is not called a success.

use std::sync::Mutex;

const FAILED: &str = "command failed to execute correctly";

static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Reads what pacman prints, line by line, however it arrives.
#[derive(Default)]
pub struct Watch {
    partial: Vec<u8>,
    last_hook: Option<String>,
}

impl Watch {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if b == b'\n' || b == b'\r' {
                let line = String::from_utf8_lossy(&std::mem::take(&mut self.partial)).into_owned();
                self.line(&crate::style::strip_ansi(&line));
            } else {
                self.partial.push(b);
            }
        }
    }

    fn line(&mut self, line: &str) {
        let line = line.trim();
        // `(2/2) zz-failhook.hook`, or the hook's description
        if let Some(rest) = line.strip_prefix('(') {
            if let Some((counts, name)) = rest.split_once(") ") {
                if counts.split_once('/').is_some_and(|(a, b)| {
                    a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit())
                }) {
                    self.last_hook = Some(name.trim().to_string());
                    return;
                }
            }
        }
        if line.ends_with(FAILED) && line.starts_with("error") {
            let name = self.last_hook.clone().unwrap_or_else(|| "a hook".into());
            if let Ok(mut seen) = SEEN.lock() {
                seen.push(name);
            }
        }
    }

    /// The last line, if the output ended without a newline.
    pub fn finish(&mut self) {
        self.feed(b"\n");
    }
}

/// The hooks that failed in this run (and forget them).
pub fn take() -> Vec<String> {
    SEEN.lock()
        .map(|mut s| std::mem::take(&mut *s))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // `SEEN` is shared by the whole process, so these tests take turns.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn alone() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_failed_hook_is_named_from_the_line_before_the_error() {
        let _alone = alone();
        take();
        let mut w = Watch::default();
        w.feed(b":: Running post-transaction hooks...\n(1/2) Arming ConditionNeedsUpdate...\n(2/2) zz-failhook.hook\nerror: command failed to execute correctly\n");
        assert_eq!(take(), ["zz-failhook.hook"]);
    }

    #[test]
    fn lines_split_across_reads_and_coloured_lines_count() {
        let _alone = alone();
        take();
        let mut w = Watch::default();
        w.feed(b"(1/1) Updating the font cache");
        w.feed(b"...\r\n\x1b[1;31merror:\x1b[0m command failed to exec");
        w.feed(b"ute correctly\n");
        assert_eq!(take(), ["Updating the font cache..."]);
    }

    #[test]
    fn nothing_failed_means_nothing_reported() {
        let _alone = alone();
        take();
        let mut w = Watch::default();
        w.feed(b"(1/1) Arming ConditionNeedsUpdate...\nerror: failed to commit transaction (conflicting files)\n");
        assert!(take().is_empty());
    }
}
