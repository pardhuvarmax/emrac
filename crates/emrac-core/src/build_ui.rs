//! A package build in emrac's style. makepkg's own lines (`==> Starting
//! build()...`) become phases with a clock; inside `prepare()`, `build()`,
//! `check()` and `package()` the compiler chatter is folded into one live line
//! (a spinner whose colour moves, the elapsed time, a bar when the build says how
//! far it is, and what it is compiling now), warnings are counted, and errors are
//! shown the moment they happen. Outside those phases everything else, above
//! all the questions of the dependency step, passes through untouched. The whole
//! output is kept by the caller in a log file; this keeps its tail for a failure.
//!
//! Pure: bytes in, bytes out, and the clock is given, so it is tested without
//! a terminal or a build.

use std::collections::VecDeque;
use std::time::Instant;

use crate::pacman_ui::{pulse_for_build, Screen};
use crate::style::{
    display_width, graded_bar, pad_right, render_line, strip_ansi, truncate, truncate_line, Grade,
    Line, Role, Seg, Style,
};

/// What a line of a build says it is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub pct: Option<u8>,
    pub counts: Option<(usize, usize)>,
    /// `CXX`, `CC`, `LINK`, `RUST`, `GO`, ...
    pub verb: String,
    pub target: String,
}

const KBUILD_VERBS: &[&str] = &[
    "CC", "CXX", "LD", "AR", "AS", "GEN", "CCLD", "CXXLD", "LINK", "INSTALL", "HOSTCC", "STRIP",
    "RANLIB", "OBJCOPY", "MKDIR", "CP", "GZIP", "MSGFMT",
];

fn object_verb(words: &[&str]) -> (String, String) {
    // `Building CXX object foo.o`, `Linking C shared library libx.so`,
    // `Generating foo.h`, `Compiling C object foo.o`, `Built target x`
    match words {
        ["Building" | "Compiling", lang, "object", rest @ ..] => {
            ((*lang).to_string(), rest.join(" "))
        }
        ["Linking", _, kind, rest @ ..] if matches!(*kind, "executable" | "library") => {
            ("LINK".to_string(), rest.join(" "))
        }
        ["Linking", _, _, _, rest @ ..] => ("LINK".to_string(), rest.join(" ")),
        ["Linking", rest @ ..] => ("LINK".to_string(), rest.join(" ")),
        ["Generating", rest @ ..] => ("GEN".to_string(), rest.join(" ")),
        ["Built", "target", rest @ ..] => ("TARGET".to_string(), rest.join(" ")),
        [verb, rest @ ..] if KBUILD_VERBS.contains(verb) => ((*verb).to_string(), rest.join(" ")),
        [first, rest @ ..] => ((*first).to_string(), rest.join(" ")),
        [] => (String::new(), String::new()),
    }
}

/// What a compiler or build tool line reports, if it reports progress.
pub fn parse_work(line: &str) -> Option<Work> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    // cmake and make: `[ 45%] Building CXX object ...`
    if let Some(rest) = t.strip_prefix('[') {
        if let Some((head, tail)) = rest.split_once(']') {
            let head = head.trim();
            if let Some(p) = head.strip_suffix('%') {
                if let Ok(pct) = p.trim().parse::<u32>() {
                    let words: Vec<&str> = tail.split_whitespace().collect();
                    let (verb, target) = object_verb(&words);
                    return Some(Work {
                        pct: Some(pct.min(100) as u8),
                        counts: None,
                        verb,
                        target,
                    });
                }
            }
            // ninja and meson: `[23/456] CC obj/foo.o`
            if let Some((k, n)) = head.split_once('/') {
                if let (Ok(k), Ok(n)) = (k.trim().parse::<usize>(), n.trim().parse::<usize>()) {
                    if let Some(pct) = (k * 100).checked_div(n) {
                        let words: Vec<&str> = tail.split_whitespace().collect();
                        let (verb, target) = object_verb(&words);
                        return Some(Work {
                            pct: Some(pct.min(100) as u8),
                            counts: Some((k, n)),
                            verb,
                            target,
                        });
                    }
                }
            }
        }
    }
    // cargo
    if let Some(rest) = t.strip_prefix("Compiling ") {
        let mut it = rest.split_whitespace();
        let name = it.next()?;
        let ver = it.next().unwrap_or("");
        return Some(Work {
            pct: None,
            counts: None,
            verb: "RUST".to_string(),
            target: format!("{name} {}", ver.trim_start_matches('v'))
                .trim()
                .to_string(),
        });
    }
    // go
    if let Some(pkg) = t.strip_prefix("# ") {
        if !pkg.contains(' ') && pkg.contains('/') {
            return Some(Work {
                pct: None,
                counts: None,
                verb: "GO".to_string(),
                target: pkg.to_string(),
            });
        }
    }
    // kbuild style: `  CC      foo.o`
    let mut it = t.split_whitespace();
    if let (Some(verb), Some(target)) = (it.next(), it.next()) {
        if KBUILD_VERBS.contains(&verb) && !target.starts_with('-') {
            return Some(Work {
                pct: None,
                counts: None,
                verb: verb.to_string(),
                target: target.to_string(),
            });
        }
    }
    // a compiler command line
    let words: Vec<&str> = t.split_whitespace().collect();
    let tool = words.first().map(|w| w.rsplit('/').next().unwrap_or(w))?;
    let lang = if tool.starts_with("g++") || tool.starts_with("c++") || tool.starts_with("clang++")
    {
        Some("CXX")
    } else if tool.starts_with("gcc") || tool == "cc" || tool.starts_with("clang") {
        Some("CC")
    } else {
        None
    };
    if let Some(lang) = lang {
        if words.contains(&"-c") {
            let source = words
                .iter()
                .rev()
                .find(|w| {
                    [".c", ".cc", ".cpp", ".cxx", ".C", ".S", ".m", ".mm"]
                        .iter()
                        .any(|e| w.ends_with(e))
                })
                .copied()
                .unwrap_or("");
            let out = words
                .iter()
                .position(|w| *w == "-o")
                .and_then(|i| words.get(i + 1))
                .copied()
                .unwrap_or("");
            let target = if source.is_empty() { out } else { source };
            if !target.is_empty() {
                return Some(Work {
                    pct: None,
                    counts: None,
                    verb: lang.to_string(),
                    target: target.to_string(),
                });
            }
        }
    }
    None
}

/// An error a build prints: shown when it happens, not folded away.
pub fn is_error(line: &str) -> bool {
    let t = line.trim_start();
    t.contains("error:")
        || t.starts_with("error[")
        || t.starts_with("FAILED:")
        || t.starts_with("make: ***")
        || t.contains("] Error ")
        || t.contains("undefined reference to")
        || t.contains("fatal error:")
        || t.starts_with("==> ERROR:")
}

pub fn is_warning(line: &str) -> bool {
    let t = line.trim_start();
    !is_error(line) && (t.contains("warning:") || t.starts_with("warning["))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// makepkg talking, or the dependency step: passed through.
    Setup,
    /// `prepare()`, `build()`, `check()`, `package()`: folded.
    Compile(String),
}

fn compile_phase(line: &str) -> Option<String> {
    let rest = line.strip_prefix("==> Starting ")?;
    let name = rest.trim_end_matches("...").trim();
    matches!(name, "prepare()" | "build()" | "check()" | "package()").then(|| name.to_string())
}

/// `4 s`, `2 min 13 s`, `1 h 04 min`.
pub fn elapsed_words(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{} h {:02} min", s / 3600, (s % 3600) / 60)
    }
}

/// `02:13`: the clock on the live line.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

const TAIL_LINES: usize = 60;
const ERASE: &[u8] = b"\r\x1b[2K";

pub struct BuildScreen {
    st: Style,
    start: Instant,
    seg: Vec<u8>,
    /// The line just handled ended with a carriage return: the line feed after
    /// it belongs to it.
    swallow_lf: bool,
    out: Vec<u8>,
    phase: Phase,
    phase_started: u64,
    now: u64,
    work: Option<Work>,
    files: usize,
    warnings: usize,
    errors: usize,
    drawn: bool,
    drawn_len: usize,
    frame: usize,
    /// (phase, milliseconds, files, warnings) of the phases that ended.
    pub done: Vec<(String, u64, usize, usize)>,
    tail: VecDeque<String>,
    cursor_hidden: bool,
    /// How long the whole build usually takes here, and how long it had run when
    /// this screen began (the build has several steps).
    expected_ms: Option<u64>,
    before_ms: u64,
}

impl BuildScreen {
    pub fn new(st: Style) -> BuildScreen {
        BuildScreen {
            st,
            start: Instant::now(),
            seg: Vec::new(),
            swallow_lf: false,
            out: Vec::new(),
            phase: Phase::Setup,
            phase_started: 0,
            now: 0,
            work: None,
            files: 0,
            warnings: 0,
            errors: 0,
            drawn: false,
            drawn_len: 0,
            frame: 0,
            done: Vec::new(),
            tail: VecDeque::new(),
            cursor_hidden: false,
            expected_ms: None,
            before_ms: 0,
        }
    }

    /// The usual time of the whole build, for the estimate beside the clock.
    pub fn set_expected(&mut self, expected_ms: u64, before_ms: u64) {
        self.expected_ms = Some(expected_ms);
        self.before_ms = before_ms;
    }

    pub fn warnings(&self) -> usize {
        self.warnings
    }

    pub fn errors(&self) -> usize {
        self.errors
    }

    /// The last lines the build printed, escapes removed: for a failure.
    pub fn tail(&self) -> Vec<String> {
        self.tail.iter().cloned().collect()
    }

    fn remember(&mut self, plain: &str) {
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(plain.trim_end().to_string());
    }

    fn compiling(&self) -> bool {
        matches!(self.phase, Phase::Compile(_))
    }

    fn erase(&mut self) {
        if self.drawn {
            self.out.extend_from_slice(ERASE);
            self.drawn = false;
            self.drawn_len = 0;
        }
    }

    fn print(&mut self, line: &Line) {
        self.erase();
        let text = render_line(
            &self.st,
            &truncate_line(&self.st, line, self.st.width.saturating_sub(1)),
        );
        self.out.extend(text.into_bytes());
        self.out.extend_from_slice(b"\r\n");
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.erase();
        let text = String::from_utf8_lossy(bytes);
        if let Some(hide) = text.rfind("\x1b[?25l") {
            self.cursor_hidden = text.rfind("\x1b[?25h").is_none_or(|show| show < hide);
        } else if text.contains("\x1b[?25h") {
            self.cursor_hidden = false;
        }
        self.out.extend_from_slice(bytes);
    }

    fn mark(&self, glyph: &str, role: Role) -> Seg {
        Seg::new(format!("{glyph} "), role)
    }

    fn end_phase(&mut self) {
        if let Phase::Compile(name) = self.phase.clone() {
            let took = self.now.saturating_sub(self.phase_started);
            let g = self.st.glyphs().ok;
            let mut line: Line = vec![
                Seg::plain(" "),
                self.mark(g, Role::Ok),
                Seg::new(pad_right(&name, 12), Role::Plain),
                Seg::new(format!("  {}", elapsed_words(took)), Role::Size),
            ];
            let mut notes = Vec::new();
            if self.files > 0 {
                notes.push(format!("{} files", self.files));
            }
            if self.warnings > 0 {
                notes.push(format!("{} warnings", self.warnings));
            }
            if !notes.is_empty() {
                line.push(Seg::new(format!("  {}", notes.join(" \u{b7} ")), Role::Dim));
            }
            self.print(&line);
            self.done.push((name, took, self.files, self.warnings));
        }
        self.work = None;
        self.files = 0;
        self.warnings = 0;
    }

    fn begin_phase(&mut self, name: String) {
        self.end_phase();
        self.phase = Phase::Compile(name);
        self.phase_started = self.now;
        self.draw();
    }

    fn live_line(&self) -> Line {
        let Phase::Compile(name) = &self.phase else {
            return Vec::new();
        };
        let frames = self.st.glyphs().spinner;
        let spin = frames[self.frame % frames.len()];
        let mut line: Line = vec![
            Seg::plain(" "),
            Seg::new(format!("{spin} "), Role::Heat(pulse_for_build(self.frame))),
            Seg::new(pad_right(name, 12), Role::Plain),
            Seg::new(
                format!("  {}", clock(self.now.saturating_sub(self.phase_started))),
                Role::Size,
            ),
        ];
        if let Some(expected) = self.expected_ms {
            let ran = self.before_ms + self.now;
            if ran > 3_000 {
                line.push(Seg::new(
                    format!("  {}", crate::eta::live_note(expected, ran)),
                    Role::Dim,
                ));
            }
        }
        if let Some(w) = &self.work {
            if let Some(p) = w.pct {
                line.push(Seg::plain("  "));
                line.extend(graded_bar(
                    &self.st,
                    f64::from(p) / 100.0,
                    14,
                    Grade::HighGood,
                ));
                line.push(Seg::new(format!(" {p:>3}%"), Role::Size));
            }
            if let Some((k, n)) = w.counts {
                line.push(Seg::new(format!("  {k}/{n}"), Role::Dim));
            }
            if !w.verb.is_empty() {
                line.push(Seg::new(format!("  {}", w.verb), Role::Note));
            }
            if !w.target.is_empty() {
                let room = self.st.width.saturating_sub(display_width_of(&line) + 16);
                line.push(Seg::new(
                    format!(" {}", truncate(&self.st, &w.target, room.max(12))),
                    Role::Plain,
                ));
            }
        } else if self.files > 0 {
            line.push(Seg::new(format!("  {} files", self.files), Role::Dim));
        }
        if self.warnings > 0 {
            line.push(Seg::new(
                format!("  {} {}", self.st.glyphs().warn, self.warnings),
                Role::Warn,
            ));
        }
        line
    }

    fn draw(&mut self) {
        if !self.compiling() {
            return;
        }
        let line = self.live_line();
        let text = render_line(
            &self.st,
            &truncate_line(&self.st, &line, self.st.width.saturating_sub(1)),
        );
        self.out.extend_from_slice(ERASE);
        self.drawn_len = display_width(&text);
        self.out.extend(text.into_bytes());
        self.drawn = true;
    }

    /// Time has moved to `now` milliseconds: the clock and the spinner.
    pub fn tick_at(&mut self, now: u64) -> Vec<u8> {
        self.now = now;
        self.frame += 1;
        self.draw();
        std::mem::take(&mut self.out)
    }

    fn line(&mut self, seg: Vec<u8>, terminator: u8) {
        let plain = strip_ansi(&String::from_utf8_lossy(&seg));
        let t = plain.trim_end();
        self.remember(t);
        self.swallow_lf = terminator == b'\r';
        // makepkg's own lines
        if let Some(name) = compile_phase(t) {
            self.begin_phase(name);
            return;
        }
        if t.starts_with("==> ") && !is_error(t) && !t.starts_with("==> WARNING:") {
            let after_compile = self.compiling();
            if after_compile {
                self.end_phase();
                self.phase = Phase::Setup;
            }
            let text = t.trim_start_matches("==> ");
            let line: Line = vec![
                Seg::plain(" "),
                Seg::new("\u{25b8} ".to_string(), Role::Brand),
                Seg::new(text.to_string(), Role::Heading),
            ];
            self.print(&line);
            return;
        }
        if t.starts_with("  -> ") || t.starts_with("    -> ") {
            let line: Line = vec![Seg::plain("   "), Seg::new(t.trim().to_string(), Role::Dim)];
            self.print(&line);
            return;
        }
        if t.starts_with("==> WARNING:") {
            let line: Line = vec![
                Seg::plain(" "),
                self.mark(self.st.glyphs().warn, Role::Warn),
                Seg::new(
                    t.trim_start_matches("==> WARNING:").trim().to_string(),
                    Role::Warn,
                ),
            ];
            self.print(&line);
            return;
        }
        if is_error(t) {
            self.errors += 1;
            let shown = t.trim_start_matches("==> ERROR:").trim();
            let line: Line = vec![
                Seg::plain(" "),
                self.mark(self.st.glyphs().problem, Role::Problem),
                Seg::new(shown.to_string(), Role::Problem),
            ];
            self.print(&line);
            self.draw();
            return;
        }
        if !self.compiling() {
            // the dependency step, the sources, the sandbox: untouched
            self.raw(&seg);
            self.raw(&[terminator]);
            self.swallow_lf = false;
            return;
        }
        if is_warning(t) {
            self.warnings += 1;
        } else if let Some(work) = parse_work(t) {
            self.files += 1;
            self.work = Some(work);
        }
        self.draw();
    }

    fn terminator(&mut self, c: u8) {
        let seg = std::mem::take(&mut self.seg);
        if seg.is_empty() {
            let own = self.swallow_lf && c == b'\n';
            self.swallow_lf = false;
            if own || self.compiling() {
                return;
            }
            self.raw(&[c]);
            return;
        }
        self.line(seg, c);
    }
}

fn display_width_of(line: &Line) -> usize {
    line.iter().map(|s| display_width(&s.text)).sum()
}

impl Screen for BuildScreen {
    fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        for &b in data {
            if b == b'\n' || b == b'\r' {
                self.terminator(b);
            } else {
                self.seg.push(b);
            }
        }
        // a question waiting for its answer is never held
        if !self.seg.is_empty() {
            let plain = strip_ansi(&String::from_utf8_lossy(&self.seg));
            let q = plain.trim_end();
            if plain.ends_with(' ')
                && ((plain.contains('[') && plain.contains(']'))
                    || q.ends_with('?')
                    || q.ends_with(':'))
            {
                let seg = std::mem::take(&mut self.seg);
                self.raw(&seg);
            }
        }
        std::mem::take(&mut self.out)
    }

    fn flush_stale(&mut self) -> Vec<u8> {
        if !self.seg.is_empty() && !self.compiling() {
            let seg = std::mem::take(&mut self.seg);
            self.raw(&seg);
        }
        std::mem::take(&mut self.out)
    }

    fn tick(&mut self) -> Vec<u8> {
        let now = self.start.elapsed().as_millis() as u64;
        self.tick_at(now)
    }

    fn resize(&mut self, width: usize) -> Vec<u8> {
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
        }
        self.st.width = width;
        self.draw();
        std::mem::take(&mut self.out)
    }

    fn finish(&mut self) -> Vec<u8> {
        if !self.seg.is_empty() {
            let seg = std::mem::take(&mut self.seg);
            let plain = strip_ansi(&String::from_utf8_lossy(&seg));
            self.remember(&plain);
            if !self.compiling() {
                self.raw(&seg);
            }
        }
        self.erase();
        self.end_phase();
        self.phase = Phase::Setup;
        if self.cursor_hidden {
            self.out.extend_from_slice(b"\x1b[?25h");
            self.cursor_hidden = false;
        }
        std::mem::take(&mut self.out)
    }
}

/// A screen that also writes everything the program says to a log file.
pub struct Logged<'a> {
    pub inner: &'a mut dyn Screen,
    pub log: Option<std::fs::File>,
}

impl Screen for Logged<'_> {
    fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        if let Some(f) = &mut self.log {
            use std::io::Write;
            let _ = f.write_all(data);
        }
        self.inner.feed(data)
    }
    fn flush_stale(&mut self) -> Vec<u8> {
        self.inner.flush_stale()
    }
    fn tick(&mut self) -> Vec<u8> {
        self.inner.tick()
    }
    fn resize(&mut self, width: usize) -> Vec<u8> {
        self.inner.resize(width)
    }
    fn finish(&mut self) -> Vec<u8> {
        self.inner.finish()
    }
}

/// How many build logs are kept.
pub const LOGS_KEPT: usize = 20;

/// A new log file for a build of `pkg`, in `dir`; the oldest logs are removed so
/// that `LOGS_KEPT` remain. `None` when it cannot be made (a log is a courtesy).
pub fn new_log(
    dir: &std::path::Path,
    pkg: &str,
    now: u64,
) -> Option<(std::path::PathBuf, std::fs::File)> {
    std::fs::create_dir_all(dir).ok()?;
    let safe: String = pkg
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let path = dir.join(format!("{safe}-{now}.log"));
    let file = std::fs::File::create(&path).ok()?;
    let mut logs: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    logs.sort();
    while logs.len() > LOGS_KEPT {
        let (_, old) = logs.remove(0);
        if old != path {
            let _ = std::fs::remove_file(old);
        }
    }
    Some((path, file))
}

/// Where the build logs are: `~/.local/share/emrac/build-logs`.
pub fn log_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(std::path::PathBuf::from(home).join(".local/share/emrac/build-logs"))
}

impl BuildScreen {
    /// What to show when a build failed: its last lines in a frame, and where
    /// the whole log is.
    pub fn failure_report(&self, log: Option<&std::path::Path>) -> String {
        let lines = self.tail();
        let take = 25.min(lines.len());
        let shown = &lines[lines.len() - take..];
        let mut out = String::new();
        let title = format!("last {} lines of the build", shown.len());
        out.push_str(&render_line(
            &self.st,
            &vec![
                Seg::new(format!("{} ", self.st.glyphs().problem), Role::Problem),
                Seg::new(title, Role::Heading),
            ],
        ));
        out.push('\n');
        for l in shown {
            let l = truncate(&self.st, l, self.st.width.saturating_sub(4));
            let role = if is_error(&l) {
                Role::Problem
            } else if is_warning(&l) {
                Role::Warn
            } else {
                Role::Dim
            };
            out.push_str(&format!("  {}\n", self.st.paint(role, &l)));
        }
        if let Some(p) = log {
            out.push_str(&format!(
                "{} {}\n",
                self.st.paint(Role::Dim, "the whole build is in"),
                self.st.paint(Role::Path, &p.display().to_string())
            ));
        }
        out
    }
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

    fn feed_all(s: &mut BuildScreen, text: &str) -> String {
        String::from_utf8_lossy(&s.feed(text.as_bytes())).into_owned()
    }

    #[test]
    fn progress_is_read_from_cmake_ninja_cargo_gcc_and_kbuild_lines() {
        let w = parse_work("[ 45%] Building CXX object CMakeFiles/foo.dir/src/a.cpp.o").unwrap();
        assert_eq!(
            (w.pct, w.verb.as_str(), w.target.as_str()),
            (Some(45), "CXX", "CMakeFiles/foo.dir/src/a.cpp.o")
        );
        let w = parse_work("[100%] Linking CXX executable foo").unwrap();
        assert_eq!(
            (w.pct, w.verb.as_str(), w.target.as_str()),
            (Some(100), "LINK", "foo")
        );
        let w = parse_work("[23/456] Building C object src/x.o").unwrap();
        assert_eq!(
            (w.pct, w.counts, w.verb.as_str()),
            (Some(5), Some((23, 456)), "C")
        );
        let w = parse_work("[7/10] Compiling C object lib/foo.p/a.c.o").unwrap();
        assert_eq!(
            (w.counts, w.verb.as_str(), w.target.as_str()),
            (Some((7, 10)), "C", "lib/foo.p/a.c.o")
        );
        let w = parse_work("   Compiling serde v1.0.200 (/home/x/.cargo/registry/serde)").unwrap();
        assert_eq!(
            (w.verb.as_str(), w.target.as_str()),
            ("RUST", "serde 1.0.200")
        );
        let w = parse_work("g++ -O2 -fPIC -c -o build/a.o src/a.cc").unwrap();
        assert_eq!((w.verb.as_str(), w.target.as_str()), ("CXX", "src/a.cc"));
        let w = parse_work("/usr/bin/gcc -c foo.c -o foo.o").unwrap();
        assert_eq!((w.verb.as_str(), w.target.as_str()), ("CC", "foo.c"));
        let w = parse_work("  CC      drivers/net/x.o").unwrap();
        assert_eq!(
            (w.verb.as_str(), w.target.as_str()),
            ("CC", "drivers/net/x.o")
        );
        let w = parse_work("# github.com/foo/bar/internal").unwrap();
        assert_eq!(w.verb, "GO");
        for none in [
            "",
            "checking for gcc... yes",
            "make[1]: Entering directory '/x'",
            "install -Dm755 foo /pkg/usr/bin/foo",
            "[ok] fine",
            "gcc --version",
            "# a comment",
        ] {
            assert_eq!(parse_work(none), None, "{none:?}");
        }
    }

    #[test]
    fn errors_and_warnings_are_told_apart() {
        for e in [
            "foo.c:12:5: error: expected ';'",
            "error[E0425]: cannot find value",
            "FAILED: src/a.o",
            "make: *** [Makefile:3: all] Error 2",
            "ld: x.o: undefined reference to `foo'",
            "fatal error: no.h: No such file",
            "==> ERROR: A failure occurred in build().",
        ] {
            assert!(is_error(e) && !is_warning(e), "{e}");
        }
        for w in [
            "foo.c:3:1: warning: unused variable 'x'",
            "warning[W0001]: x",
        ] {
            assert!(is_warning(w) && !is_error(w), "{w}");
        }
        assert!(!is_error("compiling errors.c") && !is_warning("-Wall -Werror"));
    }

    #[test]
    fn a_build_becomes_phases_with_a_live_line_and_a_summary_of_each() {
        let mut s = BuildScreen::new(st(false));
        let mut shown = String::new();
        shown += &feed_all(&mut s, "==> Making package: foo 1.0-1 (Sun Oct 18)\n==> Extracting sources...\n  -> Extracting foo.tar.gz with bsdtar\n");
        assert!(
            shown.contains("\u{25b8} Making package: foo 1.0-1"),
            "{shown}"
        );
        assert!(shown.contains("\u{25b8} Extracting sources..."), "{shown}");
        assert!(
            shown.contains("-> Extracting foo.tar.gz with bsdtar"),
            "{shown}"
        );
        shown.clear();
        shown += &feed_all(&mut s, "==> Starting build()...\n");
        assert!(
            shown.contains("build()") && shown.starts_with("\r\x1b[2K"),
            "{shown:?}"
        );
        // compile lines are folded into the live line; nothing is printed with a newline
        let chatter = feed_all(&mut s, "checking for gcc... yes\n[ 10%] Building CXX object a.o\n[ 20%] Building CXX object b.o\n");
        assert!(!chatter.contains("checking for gcc"), "{chatter}");
        assert!(!chatter.contains('\n'), "{chatter:?}");
        let live = String::from_utf8_lossy(&s.tick_at(65_000)).into_owned();
        assert!(
            live.contains("01:05")
                && live.contains(" 20%")
                && live.contains("CXX")
                && live.contains("b.o"),
            "{live}"
        );
        // warnings are counted, errors shown at once
        let w = feed_all(&mut s, "a.c:1:1: warning: unused\n");
        assert!(!w.contains("unused"), "{w}");
        let e = feed_all(&mut s, "b.c:2:2: error: expected ';'\n");
        assert!(
            e.contains("\u{2716} b.c:2:2: error: expected ';'\r\n"),
            "{e:?}"
        );
        let end = feed_all(&mut s, "==> Starting package()...\n");
        assert!(
            end.contains("\u{2714} build()")
                && end.contains("2 files")
                && end.contains("1 warnings"),
            "{end}"
        );
        let fin = String::from_utf8_lossy(&s.finish()).into_owned();
        assert!(fin.contains("\u{2714} package()"), "{fin}");
        assert_eq!(s.done.len(), 2);
        assert_eq!(s.warnings(), 0, "counted per phase");
        assert_eq!(s.errors(), 1);
    }

    #[test]
    fn outside_the_compile_phases_everything_passes_and_questions_are_never_held() {
        let mut s = BuildScreen::new(st(false));
        let out = feed_all(&mut s, "==> Checking runtime dependencies...\nresolving dependencies...\n:: Proceed with installation? [Y/n] ");
        assert!(
            out.contains("\u{25b8} Checking runtime dependencies..."),
            "{out}"
        );
        assert!(out.contains("resolving dependencies...\n"), "{out:?}");
        assert!(
            out.ends_with(":: Proceed with installation? [Y/n] "),
            "{out:?}"
        );
        // a half line is shown after a moment, there
        assert!(feed_all(&mut s, "downloading").is_empty());
        assert_eq!(String::from_utf8_lossy(&s.flush_stale()), "downloading");
    }

    #[test]
    fn the_tail_keeps_the_last_lines_for_a_failure() {
        let mut s = BuildScreen::new(st(false));
        feed_all(&mut s, "==> Starting build()...\n");
        for i in 0..100 {
            feed_all(&mut s, &format!("line {i}\n"));
        }
        let t = s.tail();
        assert_eq!(t.len(), 60);
        assert_eq!(t.last().unwrap(), "line 99");
        assert_eq!(t.first().unwrap(), "line 40");
    }

    #[test]
    fn a_resize_redraws_the_live_line_to_fit() {
        let mut s = BuildScreen::new(st(false));
        feed_all(&mut s, "==> Starting build()...\n[ 50%] Building CXX object some/long/path/to/an/object/file.o\n");
        let drawn = s.tick_at(1000);
        assert!(!drawn.is_empty());
        let out = String::from_utf8_lossy(&s.resize(40)).into_owned();
        assert!(out.starts_with('\r') && out.contains("\x1b[J"), "{out:?}");
        let redrawn = out.rsplit("\x1b[J").next().unwrap();
        assert!(
            display_width(redrawn.trim_start_matches("\r\x1b[2K")) < 40,
            "{redrawn:?}"
        );
    }

    #[test]
    fn colour_only_decorates_and_the_spinner_colour_moves() {
        let run = |color| {
            let mut s = BuildScreen::new(st(color));
            let mut out = feed_all(
                &mut s,
                "==> Starting build()...\n[ 50%] Building CXX object a.o\n",
            );
            out += &String::from_utf8_lossy(&s.tick_at(2000));
            out += &String::from_utf8_lossy(&s.finish());
            out
        };
        assert_eq!(strip_ansi(&run(true)), strip_ansi(&run(false)));
        let mut s = BuildScreen::new(st(true));
        feed_all(&mut s, "==> Starting build()...\n");
        let a = String::from_utf8_lossy(&s.tick_at(100)).into_owned();
        for _ in 0..8 {
            s.tick_at(200);
        }
        let b = String::from_utf8_lossy(&s.tick_at(300)).into_owned();
        assert_ne!(a, b);
    }

    #[test]
    fn a_stream_with_carriage_returns_is_the_same_build() {
        let mut s = BuildScreen::new(st(false));
        let out = feed_all(&mut s, "==> Starting build()...\r\n[ 50%] Building CXX object a.o\r\n==> Starting package()...\r\n");
        assert!(
            out.contains("\u{2714} build()") && out.contains("1 files"),
            "{out}"
        );
    }

    #[test]
    fn times_are_said_plainly() {
        assert_eq!(elapsed_words(4_000), "4 s");
        assert_eq!(elapsed_words(133_000), "2 min 13 s");
        assert_eq!(elapsed_words(3_840_000), "1 h 04 min");
        assert_eq!(clock(133_000), "02:13");
    }

    #[test]
    fn a_restyled_line_does_not_leave_a_blank_line_behind() {
        let mut s = BuildScreen::new(st(false));
        let out = feed_all(
            &mut s,
            "==> Making package: foo 1.0-1\r\n==> Retrieving sources...\r\n  -> Found a.tar.gz\r\nplain text\r\n\r\nafter a blank\r\n",
        );
        assert!(!out.contains("\r\n\r\n\r\n"), "{out:?}");
        assert!(
            out.contains("plain text\r\n\r\nafter a blank\r\n"),
            "a real blank line stays: {out:?}"
        );
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert!(
            lines[0].contains("Making package") && lines[1].contains("Retrieving"),
            "{lines:?}"
        );
    }

    #[test]
    fn the_live_line_says_how_long_is_left_and_then_that_it_is_slow() {
        let mut s = BuildScreen::new(st(false));
        s.set_expected(120_000, 0);
        feed_all(&mut s, "==> Starting build()...\n");
        let early = strip_ansi(&String::from_utf8_lossy(&s.tick_at(1_000)));
        assert!(
            !early.contains("left"),
            "nothing is said in the first seconds: {early}"
        );
        let mid = strip_ansi(&String::from_utf8_lossy(&s.tick_at(20_000)));
        assert!(mid.contains("~1 min 40 s left"), "{mid}");
        let slow = strip_ansi(&String::from_utf8_lossy(&s.tick_at(130_000)));
        assert!(slow.contains("longer than the usual 2 min 00 s"), "{slow}");
        // without an expectation the line is as it was
        let mut plain = BuildScreen::new(st(false));
        feed_all(&mut plain, "==> Starting build()...\n");
        let l = strip_ansi(&String::from_utf8_lossy(&plain.tick_at(20_000)));
        assert!(!l.contains("left") && !l.contains("usual"), "{l}");
    }
}
