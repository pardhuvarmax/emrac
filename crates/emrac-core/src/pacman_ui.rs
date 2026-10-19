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
        sync: bool,
    },
    /// A line that only names a file about to be downloaded.
    Swallow,
    /// ` core is up to date`: a database that needed no download.
    UpToDate(String),
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
            retrieving: t.contains("Retrieving") || t.contains("Synchronizing"),
            sync: t.contains("Synchronizing"),
        };
    }
    if retrieving && plain.starts_with(' ') {
        if let Some(name) = t.trim().strip_suffix(" is up to date") {
            if !name.is_empty() && !name.contains(' ') {
                return Class::UpToDate(name.to_string());
            }
        }
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
/// Where a cursor move to another line (`ESC [ n A/B/E/F`) that ends `seg` begins.
fn trailing_vertical_move(seg: &[u8]) -> Option<usize> {
    let last = *seg.last()?;
    if !matches!(last, b'A' | b'B' | b'E' | b'F') {
        return None;
    }
    let body = &seg[..seg.len() - 1];
    let digits = body.iter().rev().take_while(|b| b.is_ascii_digit()).count();
    let at = body.len().checked_sub(digits + 2)?;
    (body[at] == 0x1b && body[at + 1] == b'[').then_some(at)
}

fn looks_like_prompt(plain: &str) -> bool {
    plain.ends_with(' ')
        && ((plain.contains('[') && plain.contains(']'))
            || plain.trim_end().ends_with('?')
            || plain.trim_end().ends_with(':'))
}

/// The step of the red-to-green ramp for a frame of a build's spinner: the
/// colour of the spinner moves from red to green and back, as in a wait.
pub fn pulse_for_build(frame: usize) -> u8 {
    crate::style::pulse_step(frame)
}

/// What draws a program's output on the terminal: bytes from the program in,
/// bytes for the terminal out. The two screens (pacman's transaction and a
/// package build) are pure, so they are tested without a terminal.
pub trait Screen {
    /// What to write for `data` from the program.
    fn feed(&mut self, data: &[u8]) -> Vec<u8>;
    /// Nothing has arrived for a moment: an unfinished line is shown as it is.
    fn flush_stale(&mut self) -> Vec<u8>;
    /// Time passes (about ten times a second): spinners and clocks move.
    fn tick(&mut self) -> Vec<u8> {
        Vec::new()
    }
    /// The terminal changed width.
    fn resize(&mut self, width: usize) -> Vec<u8>;
    /// The program ended: whatever is held is written.
    fn finish(&mut self) -> Vec<u8>;
    /// The last thing the program did was ask a question, and it said nothing
    /// after it: with a failure status, that is an answer of no.
    fn left_at_a_question(&self) -> bool {
        false
    }
}

impl Screen for Renderer {
    fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        Renderer::feed(self, data)
    }
    fn flush_stale(&mut self) -> Vec<u8> {
        Renderer::flush_stale(self)
    }
    fn resize(&mut self, width: usize) -> Vec<u8> {
        Renderer::resize(self, width)
    }
    fn finish(&mut self) -> Vec<u8> {
        Renderer::finish(self)
    }
    fn left_at_a_question(&self) -> bool {
        self.at_question
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    name: String,
    rest: String,
    pct: u8,
}

pub struct Renderer {
    st: Style,
    seg: Vec<u8>,
    /// A record has just been taken and the end-of-line bytes after it are ours.
    swallowing: bool,
    live: Option<Rec>,
    /// A live line is on the screen now.
    drawn: bool,
    dirty: bool,
    retrieving: bool,
    /// The files of a download that are in progress, in the order they started,
    /// and the total of it: drawn together as one block that is redrawn in place.
    rows: Vec<Row>,
    total: Option<Rec>,
    /// Files of this download that finished: pacman writes a last line for
    /// some of them again, and it is not shown twice.
    finished: Vec<String>,
    /// The section is the sync of the databases: a few repositories, shown as a
    /// table whose rows keep their places, with a count under it.
    sync_mode: bool,
    /// The rows of the screen the block takes now, and the width of each of its lines.
    block_widths: Vec<usize>,
    /// Columns of the live line on the screen, for erasing it after a resize.
    drawn_len: usize,
    /// pacman asked the terminal to hide the cursor and has not shown it again.
    cursor_hidden: bool,
    /// The last output was a question, and nothing has been printed since.
    at_question: bool,
    out: Vec<u8>,
}

impl Renderer {
    pub fn new(st: Style) -> Renderer {
        Renderer {
            st,
            seg: Vec::new(),
            swallowing: false,
            live: None,
            drawn: false,
            dirty: false,
            retrieving: false,
            rows: Vec::new(),
            total: None,
            finished: Vec::new(),
            sync_mode: false,
            block_widths: Vec::new(),
            drawn_len: 0,
            cursor_hidden: false,
            at_question: false,
            out: Vec::new(),
        }
    }

    /// What to write to the terminal for `data` from pacman.
    pub fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        for &b in data {
            if b == b'\r' || b == b'\n' {
                self.terminator(b, true);
            } else {
                self.seg.push(b);
                // pacman rewrites its download block by moving the cursor between
                // lines, with no end of line: those moves end a line, and are ours
                if self.retrieving && matches!(b, b'A' | b'B' | b'E' | b'F') {
                    if let Some(start) = trailing_vertical_move(&self.seg) {
                        self.seg.truncate(start);
                        self.terminator(b'\r', false);
                    }
                }
            }
        }
        if !self.seg.is_empty() {
            let plain = strip_ansi(&String::from_utf8_lossy(&self.seg));
            if looks_like_prompt(&plain) {
                self.raw_partial();
                self.at_question = true;
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
        self.commit_block();
        self.commit_live();
        if self.cursor_hidden {
            // pacman hid the cursor for its bars and was stopped before it showed it
            self.out.extend_from_slice(b"\x1b[?25h");
            self.cursor_hidden = false;
        }
        std::mem::take(&mut self.out)
    }

    fn erase_live(&mut self) {
        if self.drawn {
            self.out.extend_from_slice(ERASE);
            self.drawn = false;
            self.drawn_len = 0;
        }
    }

    /// Takes the download block off the screen (its rows are kept to be drawn again).
    fn erase_block(&mut self) {
        if self.block_widths.is_empty() {
            return;
        }
        let width = self.st.width.max(1);
        let rows: usize = self
            .block_widths
            .iter()
            .map(|w| w.div_ceil(width).max(1))
            .sum();
        self.out.push(b'\r');
        if rows > 1 {
            self.out
                .extend_from_slice(format!("\x1b[{}A", rows - 1).as_bytes());
        }
        self.out.extend_from_slice(b"\x1b[J");
        self.block_widths.clear();
    }

    /// The block's lines now: every file that is in progress, then the total; for
    /// the database sync every repository in its place, and how many are done.
    fn block_lines(&self) -> Vec<Line> {
        let mut lines: Vec<Line> = self
            .rows
            .iter()
            .map(|r| {
                let done = self.sync_mode && r.pct >= 100;
                self.rec_line(
                    &Rec::DlFile {
                        name: r.name.clone(),
                        rest: r.rest.clone(),
                        pct: r.pct,
                    },
                    done.then_some(true),
                )
            })
            .collect();
        if let Some(t) = &self.total {
            lines.push(self.rec_line(t, None));
        }
        if self.sync_mode && !self.rows.is_empty() {
            lines.push(self.sync_summary());
        }
        lines
    }

    /// `Synchronizing repositories... 3/4 complete`.
    fn sync_summary(&self) -> Line {
        let done = self.rows.iter().filter(|r| r.pct >= 100).count();
        vec![
            Seg::plain(" "),
            Seg::new(
                format!(
                    "  Synchronizing repositories{} {done}/{} complete",
                    if self.st.unicode { "\u{2026}" } else { "..." },
                    self.rows.len()
                ),
                Role::Dim,
            ),
        ]
    }

    /// The width the file names are padded to: wide for package files, the
    /// longest repository for the sync.
    fn name_width(&self) -> usize {
        if self.sync_mode {
            self.rows
                .iter()
                .map(|r| display_width_of(&r.name))
                .max()
                .unwrap_or(8)
                .clamp(8, 24)
        } else {
            34
        }
    }

    fn draw_block(&mut self) {
        self.erase_block();
        if self.rows.is_empty() && self.total.is_none() {
            return;
        }
        let lines = self.block_lines();
        let mut widths = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let text = self.render(line);
            widths.push(crate::style::display_width(&text));
            if i > 0 {
                self.out.extend_from_slice(b"\r\n");
            }
            self.out.extend(text.into_bytes());
        }
        self.block_widths = widths;
    }

    /// A file that finished leaves the block for good as a line of its own, so
    /// a long download is a list of what is done above the few that are going.
    fn flush_done_rows(&mut self) {
        if self.sync_mode {
            return;
        }
        let finished: Vec<Row> = self.rows.iter().filter(|r| r.pct >= 100).cloned().collect();
        let total_done = matches!(&self.total, Some(Rec::DlTotal { pct, .. }) if *pct >= 100)
            && self.rows.iter().all(|r| r.pct >= 100);
        if finished.is_empty() && !total_done {
            return;
        }
        self.erase_block();
        self.rows.retain(|r| r.pct < 100);
        self.finished
            .extend(finished.iter().map(|r| r.name.clone()));
        for r in finished {
            let line = self.rec_line(
                &Rec::DlFile {
                    name: r.name,
                    rest: r.rest,
                    pct: r.pct,
                },
                Some(true),
            );
            self.print(&line);
        }
        if total_done {
            if let Some(t) = self.total.take() {
                let line = self.rec_line(&t, Some(true));
                self.print(&line);
            }
        }
    }

    /// The download ends: whatever is still in the block stays as lines.
    fn commit_block(&mut self) {
        self.finished.clear();
        if self.rows.is_empty() && self.total.is_none() {
            return;
        }
        self.erase_block();
        let summary = (self.sync_mode && !self.rows.is_empty()).then(|| self.sync_summary());
        let rows = std::mem::take(&mut self.rows);
        for r in rows {
            let done = r.pct >= 100;
            let line = self.rec_line(
                &Rec::DlFile {
                    name: r.name,
                    rest: r.rest,
                    pct: r.pct,
                },
                Some(done),
            );
            self.print(&line);
        }
        if let Some(t) = self.total.take() {
            let done = matches!(&t, Rec::DlTotal { pct, .. } if *pct >= 100);
            let line = self.rec_line(&t, Some(done));
            self.print(&line);
        }
        if let Some(summary) = summary {
            self.print(&summary);
        }
    }

    fn track_cursor(&mut self, raw: &[u8]) {
        let text = String::from_utf8_lossy(raw);
        if let Some(hide) = text.rfind("\x1b[?25l") {
            self.cursor_hidden = text.rfind("\x1b[?25h").is_none_or(|show| show < hide);
        } else if text.contains("\x1b[?25h") {
            self.cursor_hidden = false;
        }
    }

    fn raw_partial(&mut self) {
        self.erase_live();
        self.erase_block();
        self.dirty = true;
        let seg = std::mem::take(&mut self.seg);
        self.track_cursor(&seg);
        self.out.extend(seg);
        self.swallowing = false;
    }

    /// `real`: the byte came from pacman. A cursor move that ends a line is
    /// treated as an end of line that is not written.
    fn terminator(&mut self, c: u8, real: bool) {
        let seg = std::mem::take(&mut self.seg);
        if seg.is_empty() {
            if self.swallowing {
                if c == b'\n' && self.live_is_complete() {
                    self.commit_live();
                }
                return;
            }
            if real {
                self.erase_live();
                self.erase_block();
                self.dirty = true;
                self.out.push(c);
            }
            return;
        }
        let plain = strip_ansi(&String::from_utf8_lossy(&seg));
        if !plain.trim().is_empty() {
            self.at_question = false;
        }
        match classify(&plain, self.retrieving) {
            Class::Record(rec) => {
                self.apply(rec);
                self.swallowing = true;
                // a step ends with its line; a download is redrawn in place by
                // pacman (it moves the cursor and rewrites), so only a finished
                // one ends
                let ends = real && c == b'\n' && self.live_is_complete();
                if ends {
                    self.commit_live();
                } else {
                    self.dirty = true;
                }
            }
            Class::Swallow => {
                if self.sync_mode {
                    let name = plain.trim().to_string();
                    if !self.rows.iter().any(|r| r.name == name) {
                        self.upsert_row(&name, "waiting".to_string(), 0);
                        self.dirty = true;
                    }
                }
                self.swallowing = true;
            }
            Class::UpToDate(name) => {
                self.upsert_row(&name, "up to date".to_string(), 100);
                self.swallowing = true;
                self.dirty = true;
            }
            Class::Hook(k, n, text) => {
                self.commit_block();
                self.commit_live();
                let line = self.hook_line(k, n, &text);
                self.print(&line);
                self.swallowing = true;
            }
            Class::Section { retrieving, sync } => {
                self.commit_block();
                self.finished.clear();
                self.commit_live();
                self.retrieving = retrieving;
                self.sync_mode = sync;
                let line = vec![Seg::new(plain.trim_end().to_string(), Role::Brand)];
                self.print(&line);
                self.swallowing = true;
            }
            Class::Raw => {
                self.erase_live();
                self.erase_block();
                self.dirty = true;
                self.track_cursor(&seg);
                self.out.extend(seg);
                if real {
                    self.out.push(c);
                }
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
            Some(Rec::Step { pct, .. }) if *pct >= 100
        )
    }

    fn upsert_row(&mut self, name: &str, rest: String, pct: u8) {
        if !self.sync_mode && self.finished.iter().any(|f| f == name) {
            return;
        }
        match self.rows.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.rest = rest;
                r.pct = pct;
            }
            None => self.rows.push(Row {
                name: name.to_string(),
                rest,
                pct,
            }),
        }
    }

    fn apply(&mut self, rec: Rec) {
        match rec {
            Rec::DlTotal { .. } => {
                self.commit_live();
                self.total = Some(rec);
            }
            Rec::DlFile { name, rest, pct } => {
                self.commit_live();
                self.upsert_row(&name, rest, pct);
            }
            Rec::Step { .. } => {
                self.commit_block();
                if let (Some(Rec::Step { label: a, .. }), Rec::Step { label: b, .. }) =
                    (&self.live, &rec)
                {
                    if a != b {
                        self.commit_live();
                    }
                }
                self.live = Some(rec);
            }
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
        self.dirty = false;
    }

    fn redraw(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        if self.live.is_none() {
            self.flush_done_rows();
            self.draw_block();
            return;
        }
        let Some(rec) = self.live.clone() else {
            return;
        };
        let line = self.rec_line(&rec, None);
        let text = self.render(&line);
        self.out.extend_from_slice(ERASE);
        self.drawn_len = crate::style::display_width(&text);
        self.out.extend(text.into_bytes());
        self.drawn = true;
    }

    /// The terminal changed width: what is drawn is erased, wherever the
    /// terminal has wrapped it to, and the live line is drawn again to fit.
    pub fn resize(&mut self, width: usize) -> Vec<u8> {
        if width == self.st.width || width == 0 {
            return Vec::new();
        }
        if self.drawn {
            let rows = self.drawn_len.div_ceil(width).max(1);
            self.out.push(b'\r');
            if rows > 1 {
                self.out
                    .extend_from_slice(format!("\x1b[{}A", rows - 1).as_bytes());
            }
            self.out.extend_from_slice(b"\x1b[J");
            self.drawn = false;
            self.dirty = true;
        }
        if !self.block_widths.is_empty() {
            self.erase_block();
            self.dirty = true;
        }
        self.st.width = width;
        self.redraw();
        std::mem::take(&mut self.out)
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
                line.push(Seg::new(label(name, self.name_width()), Role::Plain));
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

fn display_width_of(text: &str) -> usize {
    crate::style::display_width(text)
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
    fn a_programs_own_cursor_movement_is_kept_and_only_pacmans_bars_lose_theirs() {
        // a hook that redraws its own line keeps its movements and colours
        let hook = "\x1b[?25lspin\r\x1b[2Kdone \x1b[32mgreen\x1b[0m\x1b[1A\r\n";
        let out = run(&[hook], false);
        assert!(out.starts_with("\x1b[?25lspin"), "{out:?}");
        assert!(
            out.contains("\x1b[2K") && out.contains("\x1b[1A"),
            "{out:?}"
        );
        // a sequence cut in two by the read is kept whole
        let out = run(&["a\x1b[", "1Fb\r\n"], false);
        assert_eq!(out, "a\x1b[1Fb\r\n");
        // the sequences that belong to pacman's own download block go with it
        let block = format!(
            ":: Retrieving packages...\r\n foo-1-1-x86_64\r\n Total (0/1)  0 B 0 B/s 00:00 {BAR0}\r\r\n\x1b[1F foo-1-1-x86_64  1 B 1 B/s 00:00 {BAR33}\r\n"
        );
        assert!(!run(&[&block], false).contains("\x1b[1F"));
    }

    #[test]
    fn a_cursor_pacman_hid_is_shown_again_if_it_was_stopped() {
        let mut r = Renderer::new(st(false));
        r.feed(b"\x1b[?25lresolving dependencies...\r\n");
        let end = r.finish();
        assert!(end.ends_with(b"\x1b[?25h"), "{end:?}");
        // when pacman showed it itself, nothing is added
        let mut r = Renderer::new(st(false));
        r.feed(b"\x1b[?25lx\r\n");
        r.feed(b"\x1b[?25h");
        assert_eq!(r.finish(), b"\x1b[?25h", "its own, once");
    }

    #[test]
    fn a_resize_erases_the_wrapped_live_line_and_draws_it_again_to_fit() {
        let mut r = Renderer::new(st(false));
        let drawn = r.feed(format!("(1/1) installing a   {BAR33}\r").as_bytes());
        let drawn = String::from_utf8_lossy(&drawn).into_owned();
        assert!(drawn.contains("installing a"));
        let cols = crate::style::display_width(drawn.trim_start_matches("\r\x1b[2K"));
        assert!(cols > 60 && cols < 100, "{cols}");
        // narrower than the line: it had wrapped onto two rows
        let out = String::from_utf8_lossy(&r.resize(50)).into_owned();
        assert!(out.starts_with("\r\x1b[1A\x1b[J"), "{out:?}");
        assert!(out.contains("installing a"));
        assert!(crate::style::display_width(out.rsplit("\x1b[J").next().unwrap()) < 50);
        // the same width is nothing, and a line that was never drawn is not redrawn
        assert!(r.resize(50).is_empty());
        let mut idle = Renderer::new(st(false));
        assert!(idle.resize(60).is_empty());
    }
    /// What a terminal would show after `out`: lines, a cursor that goes up and
    /// returns, and the erasing sequences emrac uses. Colours are not kept.
    fn screen(out: &str) -> Vec<String> {
        let plain = strip_ansi_keep_moves(out);
        let mut lines: Vec<Vec<char>> = vec![Vec::new()];
        let (mut row, mut col) = (0usize, 0usize);
        let mut chars = plain.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\r' => col = 0,
                '\n' => {
                    row += 1;
                    col = 0;
                    if lines.len() <= row {
                        lines.push(Vec::new());
                    }
                }
                '\x1b' => {
                    if chars.peek() == Some(&'[') {
                        chars.next();
                        let mut num = String::new();
                        let mut fin = ' ';
                        for n in chars.by_ref() {
                            if n.is_ascii_alphabetic() {
                                fin = n;
                                break;
                            }
                            num.push(n);
                        }
                        let n: usize = num.parse().unwrap_or(1);
                        match fin {
                            'A' => row = row.saturating_sub(n),
                            'J' => {
                                lines.truncate(row + 1);
                                lines[row].truncate(col);
                            }
                            'K' => lines[row].truncate(col),
                            _ => {}
                        }
                    }
                }
                c => {
                    while lines[row].len() < col {
                        lines[row].push(' ');
                    }
                    if col < lines[row].len() {
                        lines[row][col] = c;
                    } else {
                        lines[row].push(c);
                    }
                    col += 1;
                }
            }
        }
        lines
            .into_iter()
            .map(|l| l.into_iter().collect::<String>().trim_end().to_string())
            .collect()
    }

    /// Colours out, cursor moves and erases kept (the `screen` reads them).
    fn strip_ansi_keep_moves(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' && chars.peek() == Some(&'[') {
                let mut seq = String::from("\x1b[");
                chars.next();
                for n in chars.by_ref() {
                    seq.push(n);
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
                if !seq.ends_with('m') {
                    out.push_str(&seq);
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn bar_of(pct: u8) -> String {
        let full = usize::from(pct) * 32 / 100;
        format!("[{}{}] {pct:>3}%", "#".repeat(full), "-".repeat(32 - full))
    }

    fn file_rec(name: &str, rate: &str, pct: u8) -> String {
        format!(" {name}   16.0 KiB  {rate} 00:01 {}", bar_of(pct))
    }

    #[test]
    fn parallel_downloads_are_a_block_that_moves_and_each_file_finishes_once() {
        let mut r = Renderer::new(st(false));
        let mut all = String::new();
        let mut step = |r: &mut Renderer, text: String| {
            let out = String::from_utf8_lossy(&r.feed(text.as_bytes())).into_owned();
            all.push_str(&out);
            out
        };
        step(
            &mut r,
            ":: Retrieving packages...\r\n vim-runtime-9.2-1-x86_64\r\n vim-9.2-1-x86_64\r\n"
                .into(),
        );
        let first = step(
            &mut r,
            format!(
                "{}\r\r\n{}\r\r\n Total (0/2)   0.0 B  0.00 B/s 00:00 {}\r\r\n",
                file_rec("vim-runtime-9.2-1-x86_64", "1 MiB/s", 5),
                file_rec("vim-9.2-1-x86_64", "1 MiB/s", 2),
                bar_of(0)
            ),
        );
        // the block is drawn: two files and the total, three lines
        assert!(
            first.contains("vim-runtime-9.2-1-x86_64") && first.contains("downloading 0/2"),
            "{first:?}"
        );
        assert_eq!(first.matches("\r\n").count(), 2, "{first:?}");
        // an update redraws it in place: up two rows, clear, draw again
        let second = step(
            &mut r,
            format!(
                "\x1b[2F{}\r\r\n{}\r\r\n Total (0/2)   4 MiB  2 MiB/s 00:01 {}\r\r\n",
                file_rec("vim-runtime-9.2-1-x86_64", "2 MiB/s", 50),
                file_rec("vim-9.2-1-x86_64", "2 MiB/s", 60),
                bar_of(50)
            ),
        );
        assert!(
            second.contains("\x1b[2A\x1b[J") && second.contains("50%") && second.contains("60%"),
            "{second:?}"
        );
        // one file finishes: it leaves the block as a line, the other keeps going
        let third = step(
            &mut r,
            format!(
                "\x1b[2F{}\r\r\n{}\r\r\n Total (1/2)  8 MiB  3 MiB/s 00:01 {}\r\r\n",
                file_rec("vim-runtime-9.2-1-x86_64", "3 MiB/s", 100),
                file_rec("vim-9.2-1-x86_64", "3 MiB/s", 80),
                bar_of(90)
            ),
        );
        assert!(
            third.contains("\u{2714} vim-runtime-9.2-1-x86_64"),
            "{third:?}"
        );
        assert!(
            third.contains("vim-9.2-1-x86_64") && third.contains("80%"),
            "{third:?}"
        );
        step(
            &mut r,
            format!(
                "\x1b[1F{}\r\r\n Total (2/2)  10.3 MiB  5 MiB/s 00:02 {}\r\n",
                file_rec("vim-9.2-1-x86_64", "3 MiB/s", 100),
                bar_of(100)
            ),
        );
        step(&mut r, "checking keyring...\r\n".into());
        let rest = String::from_utf8_lossy(&r.finish()).into_owned();
        all.push_str(&rest);
        let plain: String = strip_ansi(&all).replace("\r\x1b[2K", "");
        assert_eq!(
            plain.matches("\u{2714} vim-runtime-9.2-1-x86_64").count(),
            1,
            "{plain}"
        );
        assert_eq!(
            plain.matches("\u{2714} vim-9.2-1-x86_64").count(),
            1,
            "{plain}"
        );
        assert_eq!(
            plain.matches("\u{2714} downloading 2/2").count(),
            1,
            "{plain}"
        );
        assert!(
            !plain.contains("\r\n vim-runtime-9.2-1-x86_64\r\n"),
            "names are not repeated: {plain}"
        );
        assert!(
            !all.contains("\x1b[1F") && !all.contains("\x1b[2F"),
            "pacman's own moves are gone"
        );
        assert!(plain.contains("checking keyring..."), "{plain}");
    }

    #[test]
    fn the_database_sync_is_a_table_whose_rows_keep_their_places() {
        let mut r = Renderer::new(st(false));
        let mut all = String::new();
        let mut step = |r: &mut Renderer, text: String| {
            all.push_str(&String::from_utf8_lossy(&r.feed(text.as_bytes())));
            screen(&all)
        };
        let s0 = step(
            &mut r,
            ":: Synchronizing package databases...\r\n core\r\n extra\r\n multilib\r\n g14\r\n"
                .into(),
        );
        // every repository is there at once, in the order pacman lists them
        let names: Vec<&str> = s0
            .iter()
            .skip(1)
            .take(4)
            .map(|l| l.split_whitespace().nth(1).unwrap())
            .collect();
        assert_eq!(names, ["core", "extra", "multilib", "g14"], "{s0:?}");
        assert!(
            s0[5].contains("Synchronizing repositories") && s0[5].contains("0/4 complete"),
            "{s0:?}"
        );
        let s1 = step(
            &mut r,
            format!(
                "{}\r\r\n{}\r\r\n{}\r\r\n",
                file_rec("multilib", "0.00 B/s", 19),
                file_rec("extra", "0.00 B/s", 0),
                file_rec("core", "0.00 B/s", 8)
            ),
        );
        assert!(s1[1].contains("core") && s1[1].contains("8%"), "{s1:?}");
        assert!(
            s1[3].contains("multilib") && s1[3].contains("19%"),
            "{s1:?}"
        );
        assert!(
            s1[4].contains("g14") && s1[4].contains("waiting"),
            "a repository not started yet waits in its row: {s1:?}"
        );
        // the way it really arrives: lines joined only by cursor moves, and a
        // database that is up to date written over the end of another line
        let s2 = step(
            &mut r,
            format!(
                "\x1b[3F{}\r\r\n\x1b[1E{}\r\r\n",
                file_rec("multilib", "311 KiB/s", 100),
                file_rec("core", "459 KiB/s", 100)
            ),
        );
        assert!(
            s2.iter().any(|l| l.contains("core") && l.contains("100%")),
            "{s2:?}"
        );
        let s3 = step(
            &mut r,
            format!(
                "\x1b[2F g14 is up to date\x1b[1E {}\r\r\n",
                file_rec("extra", "6.31 MiB/s", 53).trim_start()
            ),
        );
        let extra = s3.iter().find(|l| l.contains("extra")).unwrap();
        assert!(
            extra.contains("53%") && extra.contains('\u{2193}'),
            "{s3:?}"
        );
        let g14 = s3.iter().find(|l| l.contains("g14")).unwrap();
        assert!(
            g14.contains('\u{2714}') && g14.contains("up to date"),
            "{s3:?}"
        );
        assert!(s3.iter().any(|l| l.contains("3/4 complete")), "{s3:?}");
        step(
            &mut r,
            format!("\x1b[1F{}\r\n", file_rec("extra", "8.90 MiB/s", 100)),
        );
        all.push_str(&String::from_utf8_lossy(
            &r.feed(b":: Starting full system upgrade...\r\n"),
        ));
        let end = screen(&all);
        let done: Vec<&String> = end.iter().filter(|l| l.contains('\u{2714}')).collect();
        assert_eq!(done.len(), 4, "{end:?}");
        assert!(
            end[1..5]
                .iter()
                .all(|l| l.contains('\u{2714}') && l.contains("100%")),
            "{end:?}"
        );
        assert!(end[5].contains("4/4 complete"), "{end:?}");
        assert_eq!(end[6], ":: Starting full system upgrade...", "{end:?}");
        assert!(
            !all.contains("[#") && !all.contains("\x1b[1F"),
            "pacman's own bars and moves are gone"
        );
        assert!(
            !end.iter()
                .any(|l| l.trim() == "core" || l.trim() == "extra"),
            "bare names are not left: {end:?}"
        );
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
        // a warning in the middle of a bar clears the live line, is shown whole,
        // and the bar is drawn again under it
        let mut r = Renderer::new(st(false));
        r.feed(format!("(1/1) installing a   {BAR33}\r").as_bytes());
        let w = String::from_utf8_lossy(&r.feed(b"warning: x\r\n")).into_owned();
        assert!(
            w.starts_with("\r\x1b[2K") && w.contains("warning: x\r\n"),
            "{w:?}"
        );
        assert!(
            w.trim_end().ends_with("1/1") && w.contains("installing a"),
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

    #[test]
    fn the_screen_knows_when_the_program_was_left_at_a_question() {
        let mut r = Renderer::new(st(false));
        r.feed(b"Total Installed Size:  1 MiB\r\n\r\n:: Proceed with installation? [Y/n] ");
        assert!(r.left_at_a_question());
        // a blank line (the terminal's own echo of the answer) does not change that
        r.feed(b"\r\n");
        assert!(r.left_at_a_question());
        // anything printed after it does
        let mut r = Renderer::new(st(false));
        r.feed(b":: Proceed with installation? [Y/n] ");
        r.feed(b"error: failed to commit transaction\r\n");
        assert!(!r.left_at_a_question());
        let mut r = Renderer::new(st(false));
        r.feed(b":: Proceed with installation? [Y/n] ");
        r.feed(
            format!(":: Retrieving packages...\r\n (1/1) installing a  {BAR100}\r\n").as_bytes(),
        );
        assert!(!r.left_at_a_question());
        assert!(!Renderer::new(st(false)).left_at_a_question());
    }

    #[test]
    fn a_file_pacman_writes_a_last_time_is_not_shown_twice() {
        let mut r = Renderer::new(st(false));
        let mut all = String::new();
        for chunk in [
            ":: Retrieving packages...\r\n".to_string(),
            format!("{}\r\r\n", file_rec("extra", "1 MiB/s", 40)),
            format!("{}\r\r\n", file_rec("extra", "8 MiB/s", 100)),
            format!("{}\r\n", file_rec("extra", "9 MiB/s", 100)),
        ] {
            all.push_str(&String::from_utf8_lossy(&r.feed(chunk.as_bytes())));
        }
        all.push_str(&String::from_utf8_lossy(&r.finish()));
        assert_eq!(
            strip_ansi(&all).matches("\u{2714} extra").count(),
            1,
            "{all:?}"
        );
    }
}
