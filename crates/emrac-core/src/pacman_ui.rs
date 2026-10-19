//! pacman's transaction in emrac's style. pacman does the work and talks to a
//! pseudo-terminal (so it draws its real progress bars); this reads what it says
//! and redraws the progress lines, the downloads, the checks, the installs and the
//! hooks, in emrac's colours. Everything it does not recognise, above all pacman's
//! questions, warnings and errors, is passed through byte for byte.
//!
//! The renderer is pure: bytes in, bytes out, so it is tested without a terminal.

use crate::style::{
    graded_bar, pad_right, render_line, strip_ansi, truncate, truncate_line, Grade, Line, Role,
    Seg, Style,
};

/// One progress line of pacman, understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rec {
    /// `(2/5) installing foo    [####----]  45%`
    Step {
        k: usize,
        n: usize,
        label: String,
        pct: u8,
    },
    /// ` Total (1/3)  5.0 MiB  2.0 MiB/s 00:03 [####----]  50%`
    DlTotal {
        k: usize,
        n: usize,
        rest: String,
        pct: u8,
    },
    /// ` foo-1.0-1-x86_64  5.0 MiB  2.0 MiB/s 00:03 [####----]  50%`
    DlFile { name: String, rest: String, pct: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Class {
    Record(Rec),
    Hook(usize, usize, String),
    Section {
        retrieving: bool,
    },
    /// A line that only names a file about to be downloaded.
    Swallow,
    Raw,
}

const ERASE: &[u8] = b"\r\x1b[2K";

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `(2/5)` at the start of `s`: the two numbers and what follows.
fn counter(s: &str) -> Option<(usize, usize, &str)> {
    let inner = s.strip_prefix('(')?;
    let close = inner.find(')')?;
    let (k, n) = inner[..close].split_once('/')?;
    Some((
        k.trim().parse().ok()?,
        n.trim().parse().ok()?,
        inner[close + 1..].trim(),
    ))
}

/// The progress record in a line of pacman's, if it is one.
pub fn parse_record(line: &str) -> Option<Rec> {
    let t = line.trim_end();
    let body = t.strip_suffix('%')?;
    let digits = body.len() - body.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let pct: u32 = body[body.len() - digits..].parse().ok()?;
    let before = body[..body.len() - digits].trim_end();
    let before = before.strip_suffix(']')?;
    let open = before.rfind('[')?;
    let bar = &before[open + 1..];
    if bar.is_empty() || !bar.chars().all(|c| c == '#' || c == '-') {
        return None;
    }
    let head = before[..open].trim();
    let pct = pct.min(100) as u8;
    if head.starts_with('(') {
        let (k, n, label) = counter(head)?;
        if label.is_empty() {
            return None;
        }
        return Some(Rec::Step {
            k,
            n,
            label: collapse(label),
            pct,
        });
    }
    if let Some(rest) = head.strip_prefix("Total ") {
        let (k, n, rest) = counter(rest)?;
        return Some(Rec::DlTotal {
            k,
            n,
            rest: collapse(rest),
            pct,
        });
    }
    let (name, rest) = head.split_once(char::is_whitespace).unwrap_or((head, ""));
    if name.is_empty() {
        return None;
    }
    Some(Rec::DlFile {
        name: name.to_string(),
        rest: collapse(rest),
        pct,
    })
}

fn classify(plain: &str, retrieving: bool) -> Class {
    let t = plain.trim_end();
    if let Some(rec) = parse_record(t) {
        return Class::Record(rec);
    }
    if !t.contains('[') && t.ends_with("...") {
        if let Some((k, n, text)) = counter(t) {
            if !text.is_empty() {
                return Class::Hook(k, n, text.trim_end_matches("...").trim().to_string());
            }
        }
    }
    if t.starts_with(":: ") && t.ends_with("...") && !t.contains('?') {
        return Class::Section {
            retrieving: t.contains("Retrieving"),
        };
    }
    if retrieving
        && plain.starts_with(' ')
        && !t.trim().is_empty()
        && !t.trim().contains(' ')
        && !t.contains('[')
    {
        return Class::Swallow;
    }
    Class::Raw
}

/// A question waiting for an answer: no end of line yet, and it ends the way a
/// prompt does.
fn looks_like_prompt(plain: &str) -> bool {
    plain.ends_with(' ')
        && ((plain.contains('[') && plain.contains(']'))
            || plain.trim_end().ends_with('?')
            || plain.trim_end().ends_with(':'))
}

/// Takes cursor movement and erasing out of the stream (pacman uses them to
/// redraw its bars; emrac draws its own) and keeps colours and everything else.
#[derive(Default)]
struct Unescaper {
    held: Vec<u8>,
}

impl Unescaper {
    fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        for &b in data {
            if self.held.is_empty() {
                if b == 0x1b {
                    self.held.push(b);
                } else {
                    out.push(b);
                }
                continue;
            }
            self.held.push(b);
            match self.held.len() {
                2 => {
                    if b != b'[' {
                        // not a CSI: not ours to touch
                        out.append(&mut self.held);
                    }
                }
                _ => {
                    if (0x40..=0x7e).contains(&b) {
                        if b == b'm' {
                            out.append(&mut self.held);
                        } else {
                            self.held.clear();
                        }
                    } else if !((0x20..=0x3f).contains(&b)) || self.held.len() > 32 {
                        out.append(&mut self.held);
                    }
                }
            }
        }
        out
    }

    fn rest(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }
}

pub struct Renderer {
    st: Style,
    esc: Unescaper,
    seg: Vec<u8>,
    /// A record has just been taken and the end-of-line bytes after it are ours.
    swallowing: bool,
    live: Option<Rec>,
    /// A live line is on the screen now.
    drawn: bool,
    dirty: bool,
    retrieving: bool,
    dl_total_seen: bool,
    out: Vec<u8>,
}

impl Renderer {
    pub fn new(st: Style) -> Renderer {
        Renderer {
            st,
            esc: Unescaper::default(),
            seg: Vec::new(),
            swallowing: false,
            live: None,
            drawn: false,
            dirty: false,
            retrieving: false,
            dl_total_seen: false,
            out: Vec::new(),
        }
    }

    /// What to write to the terminal for `data` from pacman.
    pub fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        let clean = self.esc.feed(data);
        for &b in &clean {
            if b == b'\r' || b == b'\n' {
                self.terminator(b);
            } else {
                self.seg.push(b);
            }
        }
        if !self.seg.is_empty() {
            let plain = strip_ansi(&String::from_utf8_lossy(&self.seg));
            if looks_like_prompt(&plain) {
                self.raw_partial();
            }
        }
        self.redraw();
        std::mem::take(&mut self.out)
    }

    /// Called when nothing has arrived for a moment: an unfinished line is not a
    /// progress line (those end at once), so it is shown as it is.
    pub fn flush_stale(&mut self) -> Vec<u8> {
        if !self.seg.is_empty() {
            self.raw_partial();
        }
        std::mem::take(&mut self.out)
    }

    /// The end: whatever is held is written, a live line is finished.
    pub fn finish(&mut self) -> Vec<u8> {
        if !self.seg.is_empty() {
            self.raw_partial();
        }
        let rest = self.esc.rest();
        self.out.extend(rest);
        self.commit_live();
        std::mem::take(&mut self.out)
    }

    fn erase_live(&mut self) {
        if self.drawn {
            self.out.extend_from_slice(ERASE);
            self.drawn = false;
        }
    }

    fn raw_partial(&mut self) {
        self.erase_live();
        let seg = std::mem::take(&mut self.seg);
        self.out.extend(seg);
        self.swallowing = false;
    }

    fn terminator(&mut self, c: u8) {
        let seg = std::mem::take(&mut self.seg);
        if seg.is_empty() {
            if self.swallowing {
                if c == b'\n' && self.live_is_complete() {
                    self.commit_live();
                }
                return;
            }
            self.erase_live();
            self.out.push(c);
            return;
        }
        let plain = strip_ansi(&String::from_utf8_lossy(&seg));
        match classify(&plain, self.retrieving) {
            Class::Record(rec) => {
                self.apply(rec);
                self.swallowing = true;
                // a step ends with its line; a download is redrawn in place by
                // pacman (it moves the cursor and rewrites), so only a finished
                // one ends
                let ends = c == b'\n' && self.live_is_complete();
                if ends {
                    self.commit_live();
                } else {
                    self.dirty = true;
                }
            }
            Class::Swallow => self.swallowing = true,
            Class::Hook(k, n, text) => {
                self.commit_live();
                let line = self.hook_line(k, n, &text);
                self.print(&line);
                self.swallowing = true;
            }
            Class::Section { retrieving } => {
                self.commit_live();
                self.retrieving = retrieving;
                self.dl_total_seen = false;
                let line = vec![Seg::new(plain.trim_end().to_string(), Role::Brand)];
                self.print(&line);
                self.swallowing = true;
            }
            Class::Raw => {
                self.erase_live();
                self.out.extend(seg);
                self.out.push(c);
                self.swallowing = false;
            }
        }
    }

    /// The record on show has reached 100%: its line is finished. Anything less
    /// is redrawn in place (pacman rewrites the lines of a download, and it
    /// starts a check at 0% on a line of its own).
    fn live_is_complete(&self) -> bool {
        matches!(
            &self.live,
            Some(Rec::Step { pct, .. } | Rec::DlTotal { pct, .. } | Rec::DlFile { pct, .. })
                if *pct >= 100
        )
    }

    fn apply(&mut self, rec: Rec) {
        match (&self.live, &rec) {
            (Some(Rec::Step { label: a, .. }), Rec::Step { label: b, .. }) if a != b => {
                self.commit_live();
            }
            (Some(Rec::DlFile { name: a, .. }), Rec::DlFile { name: b, .. }) if a != b => {
                self.commit_live();
            }
            _ => {}
        }
        match rec {
            Rec::DlTotal { .. } => {
                if matches!(self.live, Some(Rec::DlFile { .. })) {
                    self.live = None;
                    self.erase_live();
                }
                self.dl_total_seen = true;
                self.live = Some(rec);
            }
            Rec::DlFile { .. } => {
                // with a total on show, the files are only its parts
                if !self.dl_total_seen {
                    self.live = Some(rec);
                }
            }
            Rec::Step { .. } => self.live = Some(rec),
        }
    }

    fn commit_live(&mut self) {
        let Some(rec) = self.live.take() else {
            return;
        };
        let done = matches!(&rec, Rec::Step { pct, .. } | Rec::DlTotal { pct, .. } | Rec::DlFile { pct, .. } if *pct >= 100);
        let line = self.rec_line(&rec, if done { Some(true) } else { Some(false) });
        self.erase_live();
        self.print(&line);
        if matches!(rec, Rec::DlTotal { .. }) {
            self.dl_total_seen = false;
        }
        self.dirty = false;
    }

    fn redraw(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(rec) = self.live.clone() else {
            return;
        };
        let line = self.rec_line(&rec, None);
        self.out.extend_from_slice(ERASE);
        self.out.extend(self.render(&line).into_bytes());
        self.drawn = true;
    }

    fn print(&mut self, line: &Line) {
        let text = self.render(line);
        self.out.extend(text.into_bytes());
        self.out.extend_from_slice(b"\r\n");
    }

    fn render(&self, line: &Line) -> String {
        render_line(
            &self.st,
            &truncate_line(&self.st, line, self.st.width.saturating_sub(1)),
        )
    }

    fn bar_width(&self) -> usize {
        match self.st.width {
            100.. => 24,
            80..=99 => 18,
            _ => 10,
        }
    }

    fn bar(&self, pct: u8) -> Line {
        graded_bar(
            &self.st,
            f64::from(pct) / 100.0,
            self.bar_width(),
            Grade::HighGood,
        )
    }

    /// The line of a record. `finished`: `None` while it is live, else whether it
    /// ended complete (a mark) or cut short.
    fn rec_line(&self, rec: &Rec, finished: Option<bool>) -> Line {
        let g = self.st.glyphs();
        let lead = |role: Role, glyph: &str| -> Seg {
            match finished {
                Some(true) => Seg::new(format!("{} ", g.ok), Role::Ok),
                _ => Seg::new(format!("{glyph} "), role),
            }
        };
        let arrow = if self.st.unicode { "\u{2193}" } else { "v" };
        let label = |text: &str, width: usize| -> String {
            pad_right(&truncate(&self.st, text, width), width)
        };
        let mut line: Line = vec![Seg::plain(" ")];
        match rec {
            Rec::Step {
                k,
                n,
                label: l,
                pct,
            } => {
                line.push(lead(Role::Dim, " "));
                line.push(Seg::new(label(l, 34), Role::Plain));
                line.push(Seg::plain("  "));
                line.extend(self.bar(*pct));
                line.push(Seg::new(format!(" {pct:>3}%"), Role::Size));
                line.push(Seg::new(format!("  {k}/{n}"), Role::Dim));
            }
            Rec::DlTotal { k, n, rest, pct } => {
                line.push(lead(Role::Note, arrow));
                line.push(Seg::new(
                    label(&format!("downloading {k}/{n}"), 34),
                    Role::Plain,
                ));
                line.push(Seg::plain("  "));
                line.extend(self.bar(*pct));
                line.push(Seg::new(format!(" {pct:>3}%"), Role::Size));
                line.push(Seg::new(format!("  {rest}"), Role::Dim));
            }
            Rec::DlFile { name, rest, pct } => {
                line.push(lead(Role::Note, arrow));
                line.push(Seg::new(label(name, 34), Role::Plain));
                line.push(Seg::plain("  "));
                line.extend(self.bar(*pct));
                line.push(Seg::new(format!(" {pct:>3}%"), Role::Size));
                line.push(Seg::new(format!("  {rest}"), Role::Dim));
            }
        }
        line
    }

    fn hook_line(&self, k: usize, n: usize, text: &str) -> Line {
        let frac = if n == 0 { 0.0 } else { k as f64 / n as f64 };
        let mut line: Line = vec![
            Seg::plain(" "),
            Seg::new(format!("{} ", self.st.glyphs().ok), Role::Ok),
        ];
        line.push(Seg::new(label_of(&self.st, text), Role::Plain));
        line.push(Seg::plain("  "));
        line.extend(graded_bar(&self.st, frac, 8, Grade::HighGood));
        line.push(Seg::new(format!("  {k}/{n}"), Role::Dim));
        line
    }
}

fn label_of(st: &Style, text: &str) -> String {
    pad_right(&truncate(st, text, 44), 44)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Depth, Theme};

    fn st(color: bool) -> Style {
        Style {
            color,
            unicode: true,
            fancy: true,
            width: 100,
            theme: Theme::Default,
            depth: Depth::TrueColor,
        }
    }

    fn run(chunks: &[&str], color: bool) -> String {
        let mut r = Renderer::new(st(color));
        let mut out = Vec::new();
        for c in chunks {
            out.extend(r.feed(c.as_bytes()));
        }
        out.extend(r.finish());
        String::from_utf8_lossy(&out).into_owned()
    }

    const BAR0: &str = "[--------------------------------]   0%";
    const BAR33: &str = "[##########----------------------]  33%";
    const BAR100: &str = "[################################] 100%";

    #[test]
    fn a_progress_line_is_understood_and_nothing_else_is() {
        let r = parse_record(&format!("(2/5) installing foo        {BAR33}")).unwrap();
        assert_eq!(
            r,
            Rec::Step {
                k: 2,
                n: 5,
                label: "installing foo".into(),
                pct: 33
            }
        );
        assert_eq!(
            parse_record(&format!(" Total (1/3)   5.0 MiB  2.0 MiB/s 00:03 {BAR33}")),
            Some(Rec::DlTotal {
                k: 1,
                n: 3,
                rest: "5.0 MiB 2.0 MiB/s 00:03".into(),
                pct: 33
            })
        );
        assert_eq!(
            parse_record(&format!(
                " foo-1.0-1-x86_64  16.0 KiB  1.0 MiB/s 00:01 {BAR100}"
            )),
            Some(Rec::DlFile {
                name: "foo-1.0-1-x86_64".into(),
                rest: "16.0 KiB 1.0 MiB/s 00:01".into(),
                pct: 100
            })
        );
        for not in [
            "installing foo...",
            ":: Proceed with installation? [Y/n]",
            "Packages (3) a-1 b-2",
            "[#####] 50%",
            "warning: 100%",
            "(1/2) Arming ConditionNeedsUpdate...",
            "see [x] 100%",
            "",
        ] {
            assert_eq!(parse_record(not), None, "{not:?}");
        }
    }

    #[test]
    fn the_steps_of_a_transaction_become_emrac_lines_and_pacmans_bars_are_gone() {
        let input = format!(
            "(0/2) checking keys in keyring    {BAR0}\r(1/2) checking keys in keyring    {BAR33}\r(2/2) checking keys in keyring    {BAR100}\r\n\
             :: Processing package changes...\r\n\
             (1/2) installing a    {BAR0}\r(1/2) installing a    {BAR100}\r\r\n\
             Optional dependencies for a\r\n    x: y\r\n\
             (2/2) installing b    {BAR0}\r(2/2) installing b    {BAR100}\r\r\n"
        );
        let out = run(&[&input], false);
        assert!(!out.contains("[###") && !out.contains("[---"), "{out}");
        assert!(out.contains("\u{2714} checking keys in keyring"), "{out}");
        assert!(out.contains("\u{2714} installing a"), "{out}");
        assert!(out.contains("\u{2714} installing b"), "{out}");
        assert!(out.contains(":: Processing package changes..."), "{out}");
        // what is not a progress line is untouched, in order
        let a = out.find("Optional dependencies for a").unwrap();
        let b = out.find("    x: y").unwrap();
        assert!(a < b);
        assert!(out.contains("100%") && out.contains("2/2"), "{out}");
    }

    #[test]
    fn a_question_is_passed_at_once_and_the_answer_is_not_hidden() {
        let mut r = Renderer::new(st(false));
        let q =
            r.feed(b"Total Installed Size:  44 MiB\r\n\r\n:: Proceed with installation? [Y/n] ");
        let q = String::from_utf8_lossy(&q).into_owned();
        assert!(q.ends_with(":: Proceed with installation? [Y/n] "), "{q:?}");
        let a = r.feed(b"y\r\n");
        assert_eq!(a, b"y\r\n");
    }

    #[test]
    fn warnings_errors_and_hook_output_pass_through_byte_for_byte() {
        let input = "warning: foo is up to date -- reinstalling\r\nerror: failed to commit transaction (conflicting files)\r\nfoo: /usr/bin/x exists in filesystem\r\n";
        assert_eq!(run(&[input], false), input);
        let coloured = "\x1b[1;33mwarning:\x1b[0m something\r\n";
        assert_eq!(run(&[coloured], true), coloured);
    }

    #[test]
    fn cursor_movement_is_dropped_and_colours_are_kept() {
        let input = "\x1b[?25lhello\x1b[1F\x1b[2K \x1b[32mgreen\x1b[0m\r\n";
        let out = run(&[input], false);
        assert_eq!(out, "hello \x1b[32mgreen\x1b[0m\r\n");
        // a sequence cut in two by the read is still understood
        let out = run(&["a\x1b[", "1Fb\r\n"], false);
        assert_eq!(out, "ab\r\n");
    }

    #[test]
    fn parallel_downloads_show_one_total_and_hide_the_files() {
        let input = format!(
            ":: Retrieving packages...\r\n vim-runtime-9.2-1-x86_64\r\n vim-9.2-1-x86_64\r\n Total (0/2)   0.0 B  0.00 B/s 00:00 {BAR0}\r\r\n\x1b[1F vim-9.2-1-x86_64   16.0 KiB  1.0 MiB/s 00:01 {BAR33}\r\n\x1b[2E Total (1/2)   16.0 KiB  1.0 MiB/s 00:01 {BAR33}\r\r\n Total (2/2)   10.3 MiB  5.0 MiB/s 00:02 {BAR100}\r\n"
        );
        let out = run(&[&input], false);
        assert!(
            !out.contains("vim-runtime-9.2-1-x86_64"),
            "names are swallowed: {out}"
        );
        assert!(out.contains(":: Retrieving packages..."), "{out}");
        assert!(out.contains("\u{2714} downloading 2/2"), "{out}");
        assert!(out.contains("10.3 MiB 5.0 MiB/s 00:02"), "{out}");
        assert!(
            !out.contains("vim-9.2-1-x86_64"),
            "no file rows beside a total: {out}"
        );
        assert!(!out.contains('\u{1b}'), "{out:?}");
    }

    #[test]
    fn one_file_with_no_total_is_shown_as_itself() {
        let input = format!(
            ":: Retrieving packages...\r\n foo-1-1-x86_64   8.0 KiB  25.0 KiB/s 00:00 {BAR100}\r\n"
        );
        let out = run(&[&input], false);
        assert!(out.contains("\u{2714} foo-1-1-x86_64"), "{out}");
    }

    #[test]
    fn hooks_with_counters_are_restyled() {
        let out = run(&[":: Running post-transaction hooks...\r\n(1/2) Reloading system manager configuration...\r\nFailed to check for chroot()\r\n(2/2) Arming ConditionNeedsUpdate...\r\n"], false);
        assert!(
            out.contains("\u{2714} Reloading system manager configuration"),
            "{out}"
        );
        assert!(out.contains("1/2") && out.contains("2/2"));
        assert!(out.contains("Failed to check for chroot()\r\n"), "{out}");
    }

    #[test]
    fn a_live_update_is_one_line_redrawn_and_a_finished_one_stays() {
        let mut r = Renderer::new(st(false));
        let a =
            String::from_utf8_lossy(&r.feed(format!("(1/1) installing a   {BAR33}\r").as_bytes()))
                .into_owned();
        assert!(a.starts_with("\r\x1b[2K") && !a.contains('\n'), "{a:?}");
        let b = String::from_utf8_lossy(
            &r.feed(format!("(1/1) installing a   {BAR100}\r\r\n").as_bytes()),
        )
        .into_owned();
        assert!(
            b.contains("\u{2714} installing a") && b.ends_with("\r\n"),
            "{b:?}"
        );
        // a warning in the middle of a bar clears the live line first
        let mut r = Renderer::new(st(false));
        r.feed(format!("(1/1) installing a   {BAR33}\r").as_bytes());
        let w = String::from_utf8_lossy(&r.feed(b"warning: x\r\n")).into_owned();
        assert!(
            w.starts_with("\r\x1b[2K") && w.ends_with("warning: x\r\n"),
            "{w:?}"
        );
    }

    #[test]
    fn an_unfinished_line_is_shown_after_a_moment_not_held_for_ever() {
        let mut r = Renderer::new(st(false));
        assert!(r.feed(b"downloading something").is_empty());
        assert_eq!(r.flush_stale(), b"downloading something");
        assert!(r.flush_stale().is_empty());
    }

    #[test]
    fn colour_only_decorates() {
        let input =
            format!("(1/1) installing a   {BAR100}\r\r\n:: Processing package changes...\r\n");
        let plain = run(&[&input], false);
        let lit = run(&[&input], true);
        assert!(lit.contains('\u{1b}'));
        assert_eq!(strip_ansi(&lit), plain);
    }

    #[test]
    fn the_end_finishes_a_line_that_never_got_its_newline() {
        let out = run(&[&format!("(1/1) installing a   {BAR33}\r")], false);
        assert!(
            out.contains("installing a") && out.ends_with("\r\n"),
            "{out:?}"
        );
    }

    #[test]
    fn a_check_that_starts_on_a_line_of_its_own_is_not_finished_at_0_percent() {
        let out = run(
            &[&format!(
                "(0/1) checking keys in keyring   {BAR0}\r\n(1/1) checking keys in keyring   {BAR100}\r\n"
            )],
            false,
        );
        assert_eq!(out.matches("checking keys in keyring").count(), 1, "{out}");
        assert!(out.contains("\u{2714} checking keys in keyring"), "{out}");
    }
}
