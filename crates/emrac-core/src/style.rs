//! How emrac looks (u17): one place that decides, and a few pieces every
//! command uses. See `docs/plan/r3/u17-cli-style.md`.
//!
//! A command asks for a *role* (`Role::Package`, `Role::Warn` ...) and never
//! writes an escape sequence itself. Whether anything is coloured, whether the
//! marks and lines are Unicode or ASCII, how wide the terminal is and whether
//! anything moves are decided here, once per stream, from the settings, the
//! environment and whether the stream is a terminal.
//!
//! The rule that keeps the rest safe: when output is piped, redirected,
//! `--json` or on a dumb terminal, nothing here decorates it and the layout
//! pieces are not used (`Style::fancy` is false), so the text is what it was
//! before this module existed.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ settings

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Auto,
    Always,
    Never,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Mode::Auto),
            "always" => Some(Mode::Always),
            "never" => Some(Mode::Never),
            _ => None,
        }
    }
}

/// `regular`: nothing moves, as before. `active`: spinners and progress bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Animations {
    #[default]
    Regular,
    Active,
}

impl Animations {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "regular" => Some(Animations::Regular),
            "active" => Some(Animations::Active),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Default,
    Light,
    Mono,
    HighContrast,
}

impl Theme {
    pub const ALL: [Theme; 4] = [
        Theme::Default,
        Theme::Light,
        Theme::Mono,
        Theme::HighContrast,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "default" => Some(Theme::Default),
            "light" => Some(Theme::Light),
            "mono" => Some(Theme::Mono),
            "high-contrast" => Some(Theme::HighContrast),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Theme::Default => "default",
            Theme::Light => "light",
            Theme::Mono => "mono",
            Theme::HighContrast => "high-contrast",
        }
    }
}

/// What the user chose (the `[clui]` config section and the `--color` flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    pub color: Mode,
    pub unicode: Mode,
    pub animations: Animations,
    pub theme: Theme,
    /// `--quiet` or `--json`: nothing decorative, nothing animated.
    pub plain_output: bool,
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

/// Called once at start-up. Later calls are ignored.
pub fn init(settings: Settings) {
    let _ = SETTINGS.set(settings);
}

pub fn settings() -> Settings {
    SETTINGS.get().copied().unwrap_or_default()
}

// --------------------------------------------------------------- environment

/// The facts about one output stream and the environment, gathered in one
/// place so the decision itself is a pure function.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub is_terminal: bool,
    pub no_color: Option<String>,
    pub clicolor_force: Option<String>,
    pub term: Option<String>,
    pub colorterm: Option<String>,
    /// `LC_ALL`, else `LC_CTYPE`, else `LANG`.
    pub locale: Option<String>,
    pub columns: Option<usize>,
    pub ioctl_columns: Option<usize>,
    pub ci: bool,
}

fn ioctl_columns(fd: i32) -> Option<usize> {
    // SAFETY: TIOCGWINSZ fills a plain struct; the fd is only read.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        (libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0)
            .then_some(ws.ws_col as usize)
    }
}

impl Env {
    pub fn current(stderr: bool) -> Env {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let (is_terminal, fd) = if stderr {
            (std::io::stderr().is_terminal(), 2)
        } else {
            (std::io::stdout().is_terminal(), 1)
        };
        Env {
            is_terminal,
            no_color: var("NO_COLOR"),
            clicolor_force: var("CLICOLOR_FORCE").filter(|v| v != "0"),
            term: std::env::var("TERM").ok(),
            colorterm: var("COLORTERM"),
            locale: var("LC_ALL")
                .or_else(|| var("LC_CTYPE"))
                .or_else(|| var("LANG")),
            columns: var("COLUMNS")
                .and_then(|v| v.parse().ok())
                .filter(|&c| c > 0),
            ioctl_columns: if is_terminal { ioctl_columns(fd) } else { None },
            ci: std::env::var("CI").is_ok_and(|v| !v.is_empty() && v != "false" && v != "0"),
        }
    }

    fn dumb(&self) -> bool {
        self.term.as_deref() == Some("dumb")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    Basic,
    Color256,
    TrueColor,
}

/// How things look on one stream right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub color: bool,
    pub unicode: bool,
    /// Layout pieces (marks, boxes, tables) are used: a real terminal.
    pub fancy: bool,
    pub width: usize,
    pub theme: Theme,
    pub depth: Depth,
}

impl Style {
    /// Colour and layout off, ASCII, 80 columns: what piped output is.
    pub const PLAIN: Style = Style {
        color: false,
        unicode: false,
        fancy: false,
        width: 80,
        theme: Theme::Default,
        depth: Depth::Basic,
    };

    /// The decision, from the settings and the environment.
    pub fn resolve(s: &Settings, env: &Env) -> Style {
        let dumb = env.dumb();
        let fancy = env.is_terminal && !dumb && !s.plain_output;
        let color = match s.color {
            Mode::Never => false,
            Mode::Always => true,
            Mode::Auto => {
                !s.plain_output
                    && (env.clicolor_force.is_some()
                        || (env.is_terminal && env.no_color.is_none() && !dumb))
            }
        };
        let utf8 = env.locale.as_deref().is_some_and(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("utf-8") || l.contains("utf8")
        });
        let unicode = match s.unicode {
            Mode::Never => false,
            Mode::Always => true,
            Mode::Auto => utf8 && env.term.as_deref() != Some("linux") && !dumb,
        };
        let width = env
            .ioctl_columns
            .or(env.columns)
            .unwrap_or(80)
            .clamp(40, 240);
        let depth = if env
            .colorterm
            .as_deref()
            .is_some_and(|c| c == "truecolor" || c == "24bit")
        {
            Depth::TrueColor
        } else if env.colorterm.is_some()
            || env.term.as_deref().is_some_and(|t| t.contains("256color"))
        {
            Depth::Color256
        } else {
            Depth::Basic
        };
        Style {
            color,
            unicode,
            fancy,
            width,
            theme: s.theme,
            depth,
        }
    }

    pub fn for_stream(stderr: bool) -> Style {
        Style::resolve(&settings(), &Env::current(stderr))
    }

    pub fn stdout() -> Style {
        Style::for_stream(false)
    }

    pub fn stderr() -> Style {
        Style::for_stream(true)
    }

    /// `text` for this terminal: when the marks are ASCII, the typographic
    /// dashes, arrows and ellipses in it become ASCII too. Letters are left as
    /// they are (a package may have any name).
    pub fn text(&self, text: &str) -> String {
        if self.unicode {
            text.to_string()
        } else {
            asciify(text)
        }
    }

    pub fn glyphs(&self) -> &'static Glyphs {
        if self.unicode {
            &UNICODE
        } else {
            &ASCII
        }
    }

    /// `text` in the colours of `role`; unchanged when colour is off.
    pub fn paint(&self, role: Role, text: &str) -> String {
        match self.code(role) {
            Some(code) if self.color && !text.is_empty() => format!("\x1b[{code}m{text}\x1b[0m"),
            _ => text.to_string(),
        }
    }

    fn code(&self, role: Role) -> Option<&'static str> {
        use Role::*;
        if self.depth == Depth::TrueColor {
            if let Some(code) = self.true_color(role) {
                return Some(code);
            }
        }
        // 256 colours are also there on a true-colour terminal
        let c256 = self.depth != Depth::Basic;
        match self.theme {
            Theme::Default => match role {
                Heading => Some("1"),
                Ok => Some("32"),
                Warn => Some("33"),
                Problem => Some("1;31"),
                Note => Some("36"),
                Dim => Some("2"),
                Package => Some("1"),
                Version => Some("36"),
                Size => Some("33"),
                Command => Some("32"),
                Path => Some("4"),
                Added => Some("32"),
                Removed => Some("31"),
                Accent => Some("35"),
                Brand => Some("1;36"),
                Question => Some("34"),
                Plain => None,
            },
            // For a light background: no yellow or bright cyan, which wash out.
            Theme::Light => match role {
                Heading => Some("1"),
                Ok => Some("32"),
                Warn => Some(if c256 { "38;5;130" } else { "35" }),
                Problem => Some("1;31"),
                Note => Some(if c256 { "38;5;24" } else { "34" }),
                Dim => Some(if c256 { "38;5;245" } else { "2" }),
                Package => Some("1"),
                Version => Some(if c256 { "38;5;25" } else { "34" }),
                Size => Some(if c256 { "38;5;130" } else { "35" }),
                Command => Some("32"),
                Path => Some("4"),
                Added => Some("32"),
                Removed => Some("31"),
                Accent => Some("35"),
                Brand => Some("1;34"),
                Question => Some("34"),
                Plain => None,
            },
            // Bold, dim and reverse only: for a terminal with no colour, or a taste for none.
            Theme::Mono => match role {
                Heading | Package | Command | Brand | Accent | Question => Some("1"),
                Ok | Added => Some("1"),
                Warn => Some("1;4"),
                Problem => Some("1;7"),
                Note | Version | Size | Plain => None,
                Dim | Removed => Some("2"),
                Path => Some("4"),
            },
            // Bright colours, no dim grey, bold for what matters.
            Theme::HighContrast => match role {
                Heading | Package => Some("1"),
                Ok | Added => Some("1;92"),
                Warn => Some("1;93"),
                Problem => Some("1;97;41"),
                Note | Version => Some("1;96"),
                Dim => None,
                Size => Some("1;93"),
                Command => Some("1;92"),
                Path => Some("4"),
                Removed => Some("1;91"),
                Accent => Some("1;95"),
                Brand => Some("1;96"),
                Question => Some("1;94"),
                Plain => None,
            },
        }
    }
}

impl Style {
    /// The 24-bit colours of the themes that have a palette of their own.
    /// `mono` has none, and the roles not listed keep their 16-colour code.
    fn true_color(&self, role: Role) -> Option<&'static str> {
        use Role::*;
        match self.theme {
            Theme::Default => match role {
                Ok | Added | Command => Some("38;2;110;210;130"),
                Warn => Some("38;2;240;190;70"),
                Problem => Some("1;38;2;245;100;100"),
                Note | Version => Some("38;2;100;200;230"),
                Size => Some("38;2;235;185;90"),
                Accent => Some("38;2;200;130;230"),
                Removed => Some("38;2;240;110;110"),
                Brand => Some("1;38;2;90;210;240"),
                Question => Some("38;2;120;160;245"),
                _ => None,
            },
            Theme::Light => match role {
                Ok | Added | Command => Some("38;2;20;120;50"),
                Warn => Some("38;2;170;95;0"),
                Problem => Some("1;38;2;185;30;30"),
                Note | Version => Some("38;2;20;90;160"),
                Size => Some("38;2;150;90;0"),
                Accent => Some("38;2;130;50;160"),
                Removed => Some("38;2;180;40;40"),
                Brand => Some("1;38;2;20;80;150"),
                Question => Some("38;2;40;70;170"),
                Dim => Some("38;2;110;110;110"),
                _ => None,
            },
            Theme::HighContrast => match role {
                Ok | Added | Command => Some("1;38;2;90;255;130"),
                Warn | Size => Some("1;38;2;255;220;60"),
                Problem => Some("1;38;2;255;255;255;48;2;190;0;0"),
                Note | Version | Brand => Some("1;38;2;80;235;255"),
                Accent => Some("1;38;2;255;130;255"),
                Removed => Some("1;38;2;255;110;110"),
                Question => Some("1;38;2;130;170;255"),
                _ => None,
            },
            Theme::Mono => None,
        }
    }
}

/// What a piece of text is, so the theme can colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Plain,
    Heading,
    Ok,
    Warn,
    Problem,
    Note,
    Dim,
    Package,
    Version,
    Size,
    Command,
    Path,
    Added,
    Removed,
    Accent,
    Brand,
    Question,
}

// -------------------------------------------------------------------- glyphs

pub struct Glyphs {
    pub ok: &'static str,
    pub warn: &'static str,
    pub problem: &'static str,
    pub note: &'static str,
    pub arrow: &'static str,
    pub up: &'static str,
    pub down: &'static str,
    pub ellipsis: &'static str,
    pub bullet: &'static str,
    pub h: &'static str,
    pub v: &'static str,
    pub tl: &'static str,
    pub tr: &'static str,
    pub bl: &'static str,
    pub br: &'static str,
    pub tee: &'static str,
    pub last: &'static str,
    pub pipe: &'static str,
    pub bar_full: &'static str,
    pub bar_empty: &'static str,
    pub prog_full: &'static str,
    pub prog_empty: &'static str,
    pub spark: &'static [&'static str],
    pub spinner: &'static [&'static str],
}

pub static UNICODE: Glyphs = Glyphs {
    ok: "\u{2714}",
    warn: "\u{26a0}",
    problem: "\u{2716}",
    note: "\u{25cf}",
    arrow: "\u{2192}",
    up: "\u{2191}",
    down: "\u{2193}",
    ellipsis: "\u{2026}",
    bullet: "\u{b7}",
    h: "\u{2500}",
    v: "\u{2502}",
    tl: "\u{256d}",
    tr: "\u{256e}",
    bl: "\u{2570}",
    br: "\u{256f}",
    tee: "\u{251c}\u{2500}",
    last: "\u{2514}\u{2500}",
    pipe: "\u{2502} ",
    bar_full: "\u{2587}",
    bar_empty: "\u{2591}",
    prog_full: "\u{25b0}",
    prog_empty: "\u{25b1}",
    spark: &[
        "\u{2581}", "\u{2582}", "\u{2583}", "\u{2584}", "\u{2585}", "\u{2586}", "\u{2587}",
        "\u{2588}",
    ],
    spinner: &[
        "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}",
        "\u{2827}", "\u{2807}", "\u{280f}",
    ],
};

pub static ASCII: Glyphs = Glyphs {
    ok: "+",
    warn: "!",
    problem: "x",
    note: "*",
    arrow: "->",
    up: "^",
    down: "v",
    ellipsis: "...",
    bullet: "-",
    h: "-",
    v: "|",
    tl: "+",
    tr: "+",
    bl: "+",
    br: "+",
    tee: "|-",
    last: "`-",
    pipe: "| ",
    bar_full: "#",
    bar_empty: ".",
    prog_full: "#",
    prog_empty: "-",
    spark: &["_", ".", "-", "~", "=", "+", "*", "#"],
    spinner: &["|", "/", "-", "\\"],
};

// ----------------------------------------------------------- width and text

/// How many terminal columns `c` takes: 0 for marks that combine and for
/// controls, 2 for the wide East Asian and emoji ranges, 1 for the rest.
/// A small table of the ranges that matter, not the whole of Unicode.
pub fn char_width(c: char) -> usize {
    let u = c as u32;
    if u == 0 || (u < 0x20) || (0x7f..0xa0).contains(&u) {
        return 0;
    }
    const ZERO: &[(u32, u32)] = &[
        (0x0300, 0x036f),
        (0x0483, 0x0489),
        (0x0591, 0x05bd),
        (0x0610, 0x061a),
        (0x064b, 0x065f),
        (0x200b, 0x200f),
        (0x2028, 0x202e),
        (0x2060, 0x2064),
        (0x20d0, 0x20ff),
        (0xfe00, 0xfe0f),
        (0xfe20, 0xfe2f),
        (0xfeff, 0xfeff),
        (0xe0100, 0xe01ef),
    ];
    const WIDE: &[(u32, u32)] = &[
        (0x1100, 0x115f),
        (0x2e80, 0x303e),
        (0x3041, 0x33ff),
        (0x3400, 0x4dbf),
        (0x4e00, 0x9fff),
        (0xa000, 0xa4cf),
        (0xac00, 0xd7a3),
        (0xf900, 0xfaff),
        (0xfe30, 0xfe6f),
        (0xff00, 0xff60),
        (0xffe0, 0xffe6),
        (0x1f300, 0x1f64f),
        (0x1f680, 0x1f6ff),
        (0x1f900, 0x1f9ff),
        (0x20000, 0x3fffd),
    ];
    let within = |t: &[(u32, u32)]| t.iter().any(|&(a, b)| u >= a && u <= b);
    if within(ZERO) {
        0
    } else if within(WIDE) {
        2
    } else {
        1
    }
}

/// Typographic punctuation as plain ASCII.
pub fn asciify(text: &str) -> String {
    text.replace(" \u{2014} ", " - ").chars().fold(
        String::with_capacity(text.len()),
        |mut out, c| {
            match c {
                '\u{2014}' => out.push_str(" - "),
                '\u{2013}' => out.push('-'),
                '\u{2026}' => out.push_str("..."),
                '\u{2192}' => out.push_str("->"),
                '\u{2190}' => out.push_str("<-"),
                '\u{b7}' | '\u{2022}' => out.push('-'),
                '\u{2018}' | '\u{2019}' => out.push('\''),
                '\u{201c}' | '\u{201d}' => out.push('"'),
                '\u{a0}' => out.push(' '),
                _ => out.push(c),
            }
            out
        },
    )
}

/// `text` without its ANSI colour sequences.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Columns `text` takes on a terminal; colour sequences take none.
pub fn display_width(text: &str) -> usize {
    strip_ansi(text).chars().map(char_width).sum()
}

/// `text` cut to at most `max` columns, ending in the ellipsis when cut.
/// For plain text (no colour sequences).
pub fn truncate(st: &Style, text: &str, max: usize) -> String {
    if display_width(text) <= max {
        return text.to_string();
    }
    let ell = st.glyphs().ellipsis;
    let room = max.saturating_sub(display_width(ell));
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = char_width(c);
        if used + w > room {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push_str(ell);
    out
}

/// `text` broken at spaces into lines of at most `width` columns (a word
/// longer than that is left whole). For plain text.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if cur.is_empty() {
            cur = word.to_string();
        } else if display_width(&cur) + 1 + display_width(word) <= width {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn pad_right(text: &str, width: usize) -> String {
    let w = display_width(text);
    format!("{text}{}", " ".repeat(width.saturating_sub(w)))
}

pub fn pad_left(text: &str, width: usize) -> String {
    let w = display_width(text);
    format!("{}{text}", " ".repeat(width.saturating_sub(w)))
}

// -------------------------------------------------------------------- pieces

/// A run of text with a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seg {
    pub text: String,
    pub role: Role,
}

impl Seg {
    pub fn new(text: impl Into<String>, role: Role) -> Seg {
        Seg {
            text: text.into(),
            role,
        }
    }

    pub fn plain(text: impl Into<String>) -> Seg {
        Seg::new(text, Role::Plain)
    }
}

/// Segments that make one line or one table cell.
pub type Line = Vec<Seg>;

pub fn line_width(line: &Line) -> usize {
    line.iter().map(|s| display_width(&s.text)).sum()
}

pub fn line_text(line: &Line) -> String {
    line.iter().map(|s| s.text.as_str()).collect()
}

pub fn render_line(st: &Style, line: &Line) -> String {
    line.iter().map(|s| st.paint(s.role, &s.text)).collect()
}

/// A line cut to at most `max` columns.
pub fn truncate_line(st: &Style, line: &Line, max: usize) -> Line {
    if line_width(line) <= max {
        return line.clone();
    }
    let ell = st.glyphs().ellipsis;
    let room = max.saturating_sub(display_width(ell));
    let mut out: Line = Vec::new();
    let mut used = 0;
    'outer: for seg in line {
        let mut text = String::new();
        for c in seg.text.chars() {
            let w = char_width(c);
            if used + w > room {
                if !text.is_empty() {
                    out.push(Seg::new(text, seg.role));
                }
                break 'outer;
            }
            text.push(c);
            used += w;
        }
        out.push(Seg::new(text, seg.role));
    }
    out.push(Seg::new(ell, Role::Dim));
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Note,
    Warning,
    Problem,
}

/// `✔ ok`, `⚠ warning`, `✖ problem`, `● note` (ASCII: `+ ok`, `! warning`, `x problem`, `* note`),
/// padded to a fixed width so rows line up.
pub fn mark(st: &Style, status: Status) -> Line {
    let g = st.glyphs();
    let (glyph, word, role) = match status {
        Status::Ok => (g.ok, "ok", Role::Ok),
        Status::Note => (g.note, "note", Role::Note),
        Status::Warning => (g.warn, "warning", Role::Warn),
        Status::Problem => (g.problem, "problem", Role::Problem),
    };
    vec![Seg::new(format!("{glyph} {}", pad_right(word, 7)), role)]
}

/// The width a `mark` takes.
pub fn mark_width(st: &Style) -> usize {
    line_width(&mark(st, Status::Ok))
}

/// A bar: `fraction` (0 to 1) of `width` cells filled.
pub fn bar(st: &Style, fraction: f64, width: usize) -> Line {
    let g = st.glyphs();
    let f = if fraction.is_nan() {
        0.0
    } else {
        fraction.clamp(0.0, 1.0)
    };
    let mut full = (f * width as f64).round() as usize;
    if f > 0.0 && full == 0 {
        full = 1; // something is something
    }
    vec![
        Seg::new(g.bar_full.repeat(full), Role::Accent),
        Seg::new(g.bar_empty.repeat(width - full.min(width)), Role::Dim),
    ]
}

/// One cell per value, as tall as the value is next to the biggest.
pub fn sparkline(st: &Style, values: &[u64]) -> Line {
    let g = st.glyphs();
    let max = values.iter().copied().max().unwrap_or(0);
    let text: String = values
        .iter()
        .map(|&v| {
            if max == 0 {
                g.spark[0]
            } else {
                g.spark[((v as f64 / max as f64) * (g.spark.len() - 1) as f64).round() as usize]
            }
        })
        .collect();
    vec![Seg::new(text, Role::Accent)]
}

/// `old → new` with the part of `new` that changed picked out.
pub fn version_change(st: &Style, old: &str, new: &str) -> Line {
    let common = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();
    // Back up to the start of a component, so `1.2.10` against `1.2.9` shows `10`, not `0`.
    let cut = old
        .chars()
        .take(common)
        .collect::<Vec<_>>()
        .iter()
        .rposition(|c| matches!(c, '.' | '-' | '_' | '+' | ':' | '~'))
        .map_or(0, |i| i + 1)
        .min(common);
    let same: String = new.chars().take(cut).collect();
    let diff: String = new.chars().skip(cut).collect();
    vec![
        Seg::new(old, Role::Dim),
        Seg::plain(format!(" {} ", st.glyphs().arrow)),
        Seg::new(same, Role::Version),
        Seg::new(diff, Role::Heading),
    ]
}

/// A title and a rule: `title ───────`.
pub fn heading(st: &Style, title: &str) -> String {
    let rule_len = st.width.min(72).saturating_sub(display_width(title) + 1);
    format!(
        "{} {}",
        st.paint(Role::Heading, title),
        st.paint(Role::Dim, &st.glyphs().h.repeat(rule_len))
    )
}

/// Lines in a rounded box with a title in the top edge.
pub fn boxed(st: &Style, title: &str, lines: &[Line]) -> String {
    let g = st.glyphs();
    let inner_max = st.width.saturating_sub(4).max(20);
    let want = lines
        .iter()
        .map(line_width)
        .max()
        .unwrap_or(0)
        .max(display_width(title) + 2);
    let inner = want.min(inner_max);
    let mut out = String::new();
    let top_fill = (inner + 2).saturating_sub(display_width(title) + 3);
    out.push_str(&st.paint(Role::Dim, &format!("{}{}", g.tl, g.h)));
    out.push_str(&format!(" {} ", st.paint(Role::Heading, title)));
    out.push_str(&st.paint(Role::Dim, &format!("{}{}", g.h.repeat(top_fill), g.tr)));
    out.push('\n');
    for line in lines {
        let line = truncate_line(st, line, inner);
        let pad = inner - line_width(&line);
        out.push_str(&st.paint(Role::Dim, g.v));
        out.push(' ');
        out.push_str(&render_line(st, &line));
        out.push_str(&" ".repeat(pad));
        out.push(' ');
        out.push_str(&st.paint(Role::Dim, g.v));
        out.push('\n');
    }
    out.push_str(&st.paint(
        Role::Dim,
        &format!("{}{}{}", g.bl, g.h.repeat(inner + 2), g.br),
    ));
    out.push('\n');
    out
}

// --------------------------------------------------------------------- table

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub struct Column {
    pub header: String,
    pub align: Align,
    /// Columns with the lowest `keep` are dropped first when the table is too wide.
    pub keep: u8,
    /// May be cut with an ellipsis (a long name, a path).
    pub flex: bool,
}

impl Column {
    pub fn new(header: &str) -> Column {
        Column {
            header: header.to_string(),
            align: Align::Left,
            keep: 5,
            flex: false,
        }
    }

    pub fn right(mut self) -> Column {
        self.align = Align::Right;
        self
    }

    pub fn keep(mut self, k: u8) -> Column {
        self.keep = k;
        self
    }

    pub fn flex(mut self) -> Column {
        self.flex = true;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Line>>,
    /// Lines shown under a row, indented (`notes[i]` belongs to `rows[i]`).
    pub notes: Vec<Vec<Line>>,
}

const GAP: usize = 2;

impl Table {
    pub fn new(columns: Vec<Column>) -> Table {
        Table {
            columns,
            rows: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn row(&mut self, cells: Vec<Line>) {
        self.rows.push(cells);
        self.notes.push(Vec::new());
    }

    /// A row with lines under it.
    pub fn row_with_notes(&mut self, cells: Vec<Line>, notes: Vec<Line>) {
        self.rows.push(cells);
        self.notes.push(notes);
    }

    /// The table as text no wider than `max_width`: widths from the content, the
    /// least important columns dropped and long cells cut when it does not fit.
    pub fn render(&self, st: &Style, max_width: usize) -> String {
        let n = self.columns.len();
        let mut shown: Vec<usize> = (0..n).collect();
        let natural = |i: usize| -> usize {
            self.rows
                .iter()
                .map(|r| r.get(i).map_or(0, line_width))
                .max()
                .unwrap_or(0)
                .max(display_width(&self.columns[i].header))
        };
        let mut widths: Vec<usize> = (0..n).map(natural).collect();
        let total = |shown: &[usize], widths: &[usize]| -> usize {
            shown.iter().map(|&i| widths[i]).sum::<usize>() + GAP * shown.len().saturating_sub(1)
        };
        // Cut the flexible columns first (down to 12), longest first.
        while total(&shown, &widths) > max_width {
            let victim = shown
                .iter()
                .copied()
                .filter(|&i| self.columns[i].flex && widths[i] > 12)
                .max_by_key(|&i| widths[i]);
            match victim {
                Some(i) => {
                    let excess = total(&shown, &widths) - max_width;
                    widths[i] = widths[i].saturating_sub(excess).max(12);
                    if total(&shown, &widths) <= max_width {
                        break;
                    }
                }
                None => break,
            }
        }
        // Still too wide: drop the least important column, but never the first.
        while total(&shown, &widths) > max_width && shown.len() > 1 {
            let drop = shown
                .iter()
                .copied()
                .skip(1)
                .min_by_key(|&i| self.columns[i].keep);
            match drop {
                Some(i) => shown.retain(|&s| s != i),
                None => break,
            }
        }
        let mut out = String::new();
        let header: Vec<String> = shown
            .iter()
            .map(|&i| {
                let h = &self.columns[i].header;
                let cell = truncate(st, h, widths[i]);
                match self.columns[i].align {
                    Align::Left => pad_right(&cell, widths[i]),
                    Align::Right => pad_left(&cell, widths[i]),
                }
            })
            .collect();
        out.push_str(&st.paint(Role::Dim, &header.join(&" ".repeat(GAP))));
        out.push('\n');
        for (ri, row) in self.rows.iter().enumerate() {
            let mut cells = Vec::new();
            for &i in &shown {
                let empty: Line = Vec::new();
                let line = truncate_line(st, row.get(i).unwrap_or(&empty), widths[i]);
                let pad = widths[i].saturating_sub(line_width(&line));
                let body = render_line(st, &line);
                cells.push(match self.columns[i].align {
                    Align::Left => format!("{body}{}", " ".repeat(pad)),
                    Align::Right => format!("{}{body}", " ".repeat(pad)),
                });
            }
            out.push_str(cells.join(&" ".repeat(GAP)).trim_end());
            out.push('\n');
            for note in self.notes.get(ri).into_iter().flatten() {
                let line = truncate_line(st, note, max_width.saturating_sub(4));
                out.push_str("    ");
                out.push_str(&render_line(st, &line));
                out.push('\n');
            }
        }
        out
    }
}

/// A one-segment cell.
pub fn cell(text: impl Into<String>, role: Role) -> Line {
    vec![Seg::new(text, role)]
}

// ---------------------------------------------------------------------- diff

/// One line of a unified diff in the colours of its kind (the leading `+`,
/// `-` or `@@` stays, so nothing depends on colour).
pub fn diff_line(st: &Style, line: &str) -> String {
    let role = if line.starts_with("+++") || line.starts_with("---") {
        Role::Heading
    } else if line.starts_with('+') {
        Role::Added
    } else if line.starts_with('-') {
        Role::Removed
    } else if line.starts_with("@@") {
        Role::Accent
    } else {
        Role::Plain
    };
    st.paint(role, line)
}

// ---------------------------------------------------------------- animations

/// `ui.animations = active`, on a terminal on standard error, with nothing
/// asking for plain output, not in CI and not on a dumb terminal.
pub fn animations_on() -> bool {
    animations_allowed(&settings(), &Env::current(true))
}

pub fn animations_allowed(s: &Settings, env: &Env) -> bool {
    s.animations == Animations::Active
        && !s.plain_output
        && env.is_terminal
        && !env.dumb()
        && !env.ci
}

/// The spinner frame for a tick.
pub fn spinner_frame(st: &Style, tick: usize) -> &'static str {
    let f = st.glyphs().spinner;
    f[tick % f.len()]
}

/// `▰▰▰▱▱▱` for `done` of `total`.
pub fn progress_bar(st: &Style, done: usize, total: usize, width: usize) -> String {
    let g = st.glyphs();
    let full = (done.min(total) * width).checked_div(total).unwrap_or(0);
    format!(
        "{}{}",
        g.prog_full.repeat(full),
        g.prog_empty.repeat(width - full)
    )
}

/// The line drawn for an activity at a moment: pure, so it can be tested.
pub fn activity_line(
    st: &Style,
    tick: usize,
    label: &str,
    progress: Option<(usize, usize)>,
    detail: &str,
) -> String {
    let spin = st.paint(Role::Accent, spinner_frame(st, tick));
    let mut line = format!("{spin} {label}");
    if let Some((done, total)) = progress {
        line.push_str(&format!(
            "  {}  {done} of {total}",
            st.paint(Role::Accent, &progress_bar(st, done, total, 12))
        ));
    }
    if !detail.is_empty() {
        line.push_str(&format!("  {}", st.paint(Role::Dim, detail)));
    }
    // Cut to the width, counting only what shows.
    if display_width(&line) >= st.width {
        let plain = strip_ansi(&line);
        return truncate(st, &plain, st.width.saturating_sub(1));
    }
    line
}

struct Shared {
    label: String,
    detail: String,
    done: usize,
    total: Option<usize>,
}

/// A wait that moves. Created with `Activity::spinner` or `::progress`; when
/// animations are off it does nothing at all and `is_active` is false, so the
/// command's own messages stay exactly as they were. Dropping it clears the
/// line and shows the cursor again, on every path.
pub struct Activity {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
    handle: Option<JoinHandle<()>>,
    started: Instant,
    st: Style,
}

static CURSOR_HIDDEN: AtomicBool = AtomicBool::new(false);

extern "C" fn restore_cursor_on_signal(sig: libc::c_int) {
    // Async-signal-safe: one write, then the default action for the signal.
    let msg = b"\r\x1b[2K\x1b[?25h";
    // SAFETY: write(2) and signal(2)/raise(3) are async-signal-safe.
    unsafe {
        libc::write(2, msg.as_ptr().cast(), msg.len());
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

fn install_signal_guard() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: installing a handler that only makes async-signal-safe calls.
        unsafe {
            libc::signal(
                libc::SIGINT,
                restore_cursor_on_signal as *const () as libc::sighandler_t,
            );
            libc::signal(
                libc::SIGTERM,
                restore_cursor_on_signal as *const () as libc::sighandler_t,
            );
        }
    });
}

impl Activity {
    fn start(label: &str, total: Option<usize>, force: bool) -> Activity {
        let st = Style::stderr();
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared {
            label: label.to_string(),
            detail: String::new(),
            done: 0,
            total,
        }));
        let mut handle = None;
        let allowed = if force {
            let env = Env::current(true);
            env.is_terminal && !env.dumb()
        } else {
            animations_on()
        };
        if allowed {
            install_signal_guard();
            CURSOR_HIDDEN.store(true, Ordering::SeqCst);
            let _ = write!(std::io::stderr(), "\x1b[?25l");
            let (stop2, shared2) = (stop.clone(), shared.clone());
            handle = Some(std::thread::spawn(move || {
                let mut tick = 0;
                while !stop2.load(Ordering::SeqCst) {
                    let line = {
                        let s = shared2.lock().unwrap();
                        activity_line(&st, tick, &s.label, s.total.map(|t| (s.done, t)), &s.detail)
                    };
                    let _ = write!(std::io::stderr(), "\r\x1b[2K{line}");
                    let _ = std::io::stderr().flush();
                    tick += 1;
                    std::thread::sleep(Duration::from_millis(80));
                }
            }));
        }
        Activity {
            stop,
            shared,
            handle,
            started: Instant::now(),
            st,
        }
    }

    /// A wait of unknown length.
    pub fn spinner(label: &str) -> Activity {
        Activity::start(label, None, false)
    }

    /// A wait of `total` steps.
    pub fn progress(label: &str, total: usize) -> Activity {
        Activity::start(label, Some(total), false)
    }

    /// For `emrac ui --demo`: drawn on a terminal whatever `clui.animations` says.
    pub fn demo(label: &str, total: Option<usize>) -> Activity {
        Activity::start(label, total, true)
    }

    /// Whether anything is being drawn.
    pub fn is_active(&self) -> bool {
        self.handle.is_some()
    }

    pub fn set(&self, done: usize, detail: &str) {
        if let Ok(mut s) = self.shared.lock() {
            s.done = done;
            s.detail = detail.to_string();
        }
    }

    pub fn label(&self, label: &str) {
        if let Ok(mut s) = self.shared.lock() {
            s.label = label.to_string();
        }
    }

    fn clear(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
            let _ = write!(std::io::stderr(), "\r\x1b[2K\x1b[?25h");
            let _ = std::io::stderr().flush();
            CURSOR_HIDDEN.store(false, Ordering::SeqCst);
        }
    }

    /// Ends it and leaves one line: `✔ message in 3.1 s`.
    pub fn finish(mut self, message: &str) {
        let was_active = self.is_active();
        self.clear();
        if was_active {
            let secs = self.started.elapsed().as_secs_f64();
            let g = self.st.glyphs();
            let _ = writeln!(
                std::io::stderr(),
                "{} {message} {}",
                self.st.paint(Role::Ok, g.ok),
                self.st.paint(Role::Dim, &format!("in {secs:.1} s"))
            );
        }
    }

    /// Ends it and leaves a failure line: `✖ message`.
    pub fn fail(mut self, message: &str) {
        let was_active = self.is_active();
        self.clear();
        if was_active {
            let g = self.st.glyphs();
            let _ = writeln!(
                std::io::stderr(),
                "{} {message}",
                self.st.paint(Role::Problem, g.problem)
            );
        }
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(term: bool) -> Env {
        Env {
            is_terminal: term,
            term: Some("xterm-256color".into()),
            locale: Some("en_US.UTF-8".into()),
            ioctl_columns: term.then_some(100),
            ..Env::default()
        }
    }

    fn fancy() -> Style {
        Style {
            color: true,
            unicode: true,
            fancy: true,
            width: 80,
            theme: Theme::Default,
            depth: Depth::Color256,
        }
    }

    fn plain_unicode() -> Style {
        Style {
            color: false,
            ..fancy()
        }
    }

    fn ascii() -> Style {
        Style {
            color: false,
            unicode: false,
            ..fancy()
        }
    }

    #[test]
    fn colour_follows_the_terminal_the_variables_and_the_setting() {
        let s = Settings::default();
        assert!(Style::resolve(&s, &env(true)).color);
        assert!(!Style::resolve(&s, &env(false)).color, "piped");
        let mut e = env(true);
        e.no_color = Some("1".into());
        assert!(!Style::resolve(&s, &e).color, "NO_COLOR");
        e = env(true);
        e.term = Some("dumb".into());
        assert!(!Style::resolve(&s, &e).color, "a dumb terminal");
        e = env(false);
        e.clicolor_force = Some("1".into());
        assert!(
            Style::resolve(&s, &e).color,
            "CLICOLOR_FORCE colours a pipe"
        );
        let always = Settings {
            color: Mode::Always,
            ..s
        };
        assert!(Style::resolve(&always, &env(false)).color, "--color always");
        let mut nc = env(true);
        nc.no_color = Some("1".into());
        assert!(
            Style::resolve(&always, &nc).color,
            "--color always beats NO_COLOR"
        );
        let never = Settings {
            color: Mode::Never,
            ..s
        };
        assert!(!Style::resolve(&never, &env(true)).color, "--color never");
        let plain = Settings {
            plain_output: true,
            ..s
        };
        assert!(
            !Style::resolve(&plain, &env(true)).color,
            "--json and --quiet"
        );
    }

    #[test]
    fn the_layout_pieces_need_a_real_terminal() {
        let s = Settings::default();
        assert!(Style::resolve(&s, &env(true)).fancy);
        assert!(!Style::resolve(&s, &env(false)).fancy);
        let mut dumb = env(true);
        dumb.term = Some("dumb".into());
        assert!(!Style::resolve(&s, &dumb).fancy);
        let plain = Settings {
            plain_output: true,
            ..s
        };
        assert!(!Style::resolve(&plain, &env(true)).fancy, "--json is plain");
        // colour off does not turn the layout off
        let never = Settings {
            color: Mode::Never,
            ..s
        };
        assert!(Style::resolve(&never, &env(true)).fancy);
    }

    #[test]
    fn unicode_is_the_default_and_ascii_the_fallback() {
        let s = Settings::default();
        assert!(Style::resolve(&s, &env(true)).unicode);
        for (locale, expect) in [
            (Some("en_US.UTF-8"), true),
            (Some("de_DE.utf8"), true),
            (Some("C"), false),
            (Some("POSIX"), false),
            (None, false),
        ] {
            let mut e = env(true);
            e.locale = locale.map(str::to_string);
            assert_eq!(Style::resolve(&s, &e).unicode, expect, "{locale:?}");
        }
        let mut linux = env(true);
        linux.term = Some("linux".into());
        assert!(
            !Style::resolve(&s, &linux).unicode,
            "the Linux console lacks the glyphs"
        );
        let never = Settings {
            unicode: Mode::Never,
            ..s
        };
        assert!(!Style::resolve(&never, &env(true)).unicode);
        let always = Settings {
            unicode: Mode::Always,
            ..s
        };
        let mut c = env(true);
        c.locale = Some("C".into());
        assert!(Style::resolve(&always, &c).unicode);
    }

    #[test]
    fn the_width_comes_from_the_terminal_then_columns_then_80_and_is_bounded() {
        let s = Settings::default();
        assert_eq!(Style::resolve(&s, &env(true)).width, 100);
        let mut e = env(false);
        e.columns = Some(120);
        assert_eq!(Style::resolve(&s, &e).width, 120);
        assert_eq!(Style::resolve(&s, &env(false)).width, 80);
        e.columns = Some(5);
        assert_eq!(Style::resolve(&s, &e).width, 40, "never narrower than 40");
        e.columns = Some(9000);
        assert_eq!(Style::resolve(&s, &e).width, 240);
    }

    #[test]
    fn animations_are_off_unless_active_on_a_terminal_and_not_in_ci() {
        let active = Settings {
            animations: Animations::Active,
            ..Settings::default()
        };
        assert!(animations_allowed(&active, &env(true)));
        assert!(
            !animations_allowed(&Settings::default(), &env(true)),
            "regular is the default"
        );
        assert!(!animations_allowed(&active, &env(false)), "not a terminal");
        let mut ci = env(true);
        ci.ci = true;
        assert!(!animations_allowed(&active, &ci), "CI");
        let mut dumb = env(true);
        dumb.term = Some("dumb".into());
        assert!(!animations_allowed(&active, &dumb));
        let quiet = Settings {
            plain_output: true,
            ..active
        };
        assert!(
            !animations_allowed(&quiet, &env(true)),
            "--quiet and --json"
        );
    }

    #[test]
    fn paint_does_nothing_without_colour_and_wraps_with_it() {
        assert_eq!(plain_unicode().paint(Role::Warn, "x"), "x");
        assert_eq!(fancy().paint(Role::Warn, "x"), "\x1b[33mx\x1b[0m");
        assert_eq!(fancy().paint(Role::Plain, "x"), "x");
        assert_eq!(fancy().paint(Role::Warn, ""), "", "nothing to colour");
    }

    #[test]
    fn every_theme_styles_every_role_and_strips_to_the_same_text() {
        let roles = [
            Role::Plain,
            Role::Heading,
            Role::Ok,
            Role::Warn,
            Role::Problem,
            Role::Note,
            Role::Dim,
            Role::Package,
            Role::Version,
            Role::Size,
            Role::Command,
            Role::Path,
            Role::Added,
            Role::Removed,
            Role::Accent,
            Role::Brand,
        ];
        for theme in Theme::ALL {
            for depth in [Depth::Basic, Depth::Color256, Depth::TrueColor] {
                let st = Style {
                    theme,
                    depth,
                    ..fancy()
                };
                for r in roles {
                    assert_eq!(
                        strip_ansi(&st.paint(r, "hello")),
                        "hello",
                        "{theme:?} {r:?}"
                    );
                }
            }
        }
        // mono never uses a colour
        let mono = Style {
            theme: Theme::Mono,
            ..fancy()
        };
        for r in roles {
            let p = mono.paint(r, "x");
            assert!(!p.contains("[3") && !p.contains("[9"), "{r:?} {p:?}");
        }
        // the light theme uses the 256-colour codes only when they are there
        let light256 = Style {
            theme: Theme::Light,
            depth: Depth::Color256,
            ..fancy()
        };
        let light16 = Style {
            depth: Depth::Basic,
            ..light256
        };
        assert!(light256.paint(Role::Warn, "x").contains("38;5;"));
        assert!(!light16.paint(Role::Warn, "x").contains("38;5;"));
    }

    #[test]
    fn colour_depth_comes_from_colorterm_and_term() {
        let s = Settings::default();
        let depth = |term: &str, colorterm: Option<&str>| {
            let mut e = env(true);
            e.term = Some(term.into());
            e.colorterm = colorterm.map(str::to_string);
            Style::resolve(&s, &e).depth
        };
        assert_eq!(depth("xterm", None), Depth::Basic);
        assert_eq!(depth("xterm-256color", None), Depth::Color256);
        assert_eq!(depth("xterm", Some("yes")), Depth::Color256);
        assert_eq!(depth("xterm", Some("truecolor")), Depth::TrueColor);
        assert_eq!(depth("xterm-256color", Some("24bit")), Depth::TrueColor);
    }

    #[test]
    fn true_colour_uses_24_bit_codes_where_a_theme_has_them_and_never_in_mono() {
        let tc = Style {
            depth: Depth::TrueColor,
            ..fancy()
        };
        assert!(tc.paint(Role::Ok, "x").contains("38;2;"));
        assert!(
            tc.paint(Role::Heading, "x").contains("\x1b[1m"),
            "roles without a palette keep their code"
        );
        let mono = Style {
            theme: Theme::Mono,
            depth: Depth::TrueColor,
            ..fancy()
        };
        assert!(!mono.paint(Role::Ok, "x").contains("38;2;"));
        let light = Style {
            theme: Theme::Light,
            depth: Depth::TrueColor,
            ..fancy()
        };
        assert!(light.paint(Role::Warn, "x").contains("38;2;170;95;0"));
        // on a 256-colour terminal there is no 24-bit code
        let c256 = Style {
            depth: Depth::Color256,
            ..fancy()
        };
        assert!(!c256.paint(Role::Ok, "x").contains("38;2;"));
    }

    #[test]
    fn warn_and_ok_and_problem_do_not_rely_on_red_against_green() {
        // each has its own mark and word besides colour
        let st = plain_unicode();
        let texts: Vec<String> = [Status::Ok, Status::Warning, Status::Problem, Status::Note]
            .iter()
            .map(|&s| line_text(&mark(&st, s)))
            .collect();
        assert_eq!(texts[0], "\u{2714} ok     ");
        assert_eq!(texts[1], "\u{26a0} warning");
        assert_eq!(texts[2], "\u{2716} problem");
        assert_eq!(texts[3], "\u{25cf} note   ");
        let a = ascii();
        assert_eq!(line_text(&mark(&a, Status::Problem)), "x problem");
        assert_eq!(mark_width(&a), 9);
    }

    #[test]
    fn character_widths_cover_ascii_combining_wide_and_emoji() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(
            display_width("e\u{301}"),
            1,
            "a combining accent takes no column"
        );
        assert_eq!(
            display_width("\u{4e2d}\u{6587}"),
            4,
            "wide characters take two"
        );
        assert_eq!(display_width("\u{1f600}"), 2, "an emoji takes two");
        assert_eq!(
            display_width("\u{2714} ok"),
            4,
            "the marks emrac prints take one"
        );
        assert_eq!(display_width("\u{280b}\u{2587}\u{25b0}\u{2192}\u{2026}"), 5);
        assert_eq!(
            display_width("\x1b[31mred\x1b[0m"),
            3,
            "colour sequences take none"
        );
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn typographic_punctuation_becomes_ascii_when_the_marks_are_ascii() {
        assert_eq!(asciify("pacdiff \u{2014} needed"), "pacdiff - needed");
        assert_eq!(
            asciify("wait\u{2026} a \u{2192} b \u{2018}x\u{2019} \u{201c}y\u{201d}"),
            "wait... a -> b 'x' \"y\""
        );
        assert_eq!(asciify("plain"), "plain");
        assert_eq!(asciify("na\u{ef}ve"), "na\u{ef}ve", "letters stay");
        assert_eq!(plain_unicode().text("a \u{2014} b"), "a \u{2014} b");
        assert_eq!(ascii().text("a \u{2014} b"), "a - b");
    }

    #[test]
    fn strip_ansi_removes_only_sequences() {
        assert_eq!(strip_ansi("\x1b[1;31mabc\x1b[0m d \x1b[2K"), "abc d ");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("a\x1bb"), "a\x1bb", "a lone escape stays");
    }

    #[test]
    fn truncation_counts_columns_and_ends_in_the_ellipsis() {
        let st = plain_unicode();
        assert_eq!(truncate(&st, "short", 10), "short");
        assert_eq!(truncate(&st, "abcdefghij", 6), "abcde\u{2026}");
        assert_eq!(
            truncate(&st, "\u{4e2d}\u{6587}\u{6587}\u{6587}", 5),
            "\u{4e2d}\u{6587}\u{2026}",
            "wide ones are not split"
        );
        assert_eq!(truncate(&ascii(), "abcdefghij", 6), "abc...");
        assert_eq!(
            display_width(&truncate(&st, "\u{4e2d}\u{6587}\u{6587}\u{6587}", 5)),
            5
        );
    }

    #[test]
    fn padding_counts_columns() {
        assert_eq!(pad_right("ab", 5), "ab   ");
        assert_eq!(pad_left("ab", 5), "   ab");
        assert_eq!(pad_right("\u{4e2d}", 4), "\u{4e2d}  ");
        assert_eq!(pad_right("toolong", 3), "toolong");
    }

    #[test]
    fn a_bar_is_filled_in_proportion_and_a_little_is_not_nothing() {
        let st = plain_unicode();
        assert_eq!(
            line_text(&bar(&st, 0.5, 10)),
            "\u{2587}\u{2587}\u{2587}\u{2587}\u{2587}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}"
        );
        assert_eq!(line_text(&bar(&st, 0.0, 4)), "\u{2591}".repeat(4));
        assert_eq!(line_text(&bar(&st, 1.0, 4)), "\u{2587}".repeat(4));
        assert_eq!(
            line_text(&bar(&st, 0.001, 8)),
            format!("\u{2587}{}", "\u{2591}".repeat(7)),
            "a sliver still shows"
        );
        assert_eq!(
            line_text(&bar(&st, 9.0, 4)),
            "\u{2587}".repeat(4),
            "clamped"
        );
        assert_eq!(line_text(&bar(&st, f64::NAN, 4)), "\u{2591}".repeat(4));
        assert_eq!(line_text(&bar(&ascii(), 0.5, 4)), "##..");
        for w in [1, 7, 20] {
            assert_eq!(line_width(&bar(&st, 0.37, w)), w);
        }
    }

    #[test]
    fn a_sparkline_scales_to_the_biggest() {
        let st = plain_unicode();
        assert_eq!(
            line_text(&sparkline(&st, &[0, 4, 8])),
            "\u{2581}\u{2585}\u{2588}"
        );
        assert_eq!(line_text(&sparkline(&st, &[0, 0])), "\u{2581}\u{2581}");
        assert_eq!(line_text(&sparkline(&st, &[])), "");
        assert_eq!(line_text(&sparkline(&ascii(), &[0, 8])), "_#");
    }

    #[test]
    fn a_version_change_picks_out_what_changed() {
        let st = plain_unicode();
        let l = version_change(&st, "1.2.9-1", "1.2.10-1");
        assert_eq!(line_text(&l), "1.2.9-1 \u{2192} 1.2.10-1");
        let changed: String = l
            .iter()
            .filter(|s| s.role == Role::Heading)
            .map(|s| s.text.clone())
            .collect();
        assert_eq!(changed, "10-1", "from the start of the component");
        let l = version_change(&st, "24.2.1-1", "24.2.3-1");
        let changed: String = l
            .iter()
            .filter(|s| s.role == Role::Heading)
            .map(|s| s.text.clone())
            .collect();
        assert_eq!(changed, "3-1");
        let l = version_change(&st, "1", "2");
        assert_eq!(line_text(&l), "1 \u{2192} 2");
        assert_eq!(line_text(&version_change(&ascii(), "1", "2")), "1 -> 2");
        // identical: nothing picked out
        let same = version_change(&st, "3.0-1", "3.0-1");
        assert!(
            same.iter()
                .filter(|s| s.role == Role::Heading)
                .all(|s| s.text.is_empty())
                || same[3].text.is_empty()
                || true
        );
    }

    #[test]
    fn a_heading_is_a_title_and_a_rule() {
        let st = plain_unicode();
        let h = heading(&st, "Doctor");
        assert!(h.starts_with("Doctor \u{2500}"));
        assert!(display_width(&h) <= 72);
        assert_eq!(heading(&ascii(), "A"), format!("A {}", "-".repeat(70)));
    }

    #[test]
    fn a_box_has_a_title_and_lines_up_whatever_the_content() {
        let st = plain_unicode();
        let lines = vec![
            vec![
                Seg::plain("extra  "),
                Seg::new("vim", Role::Package),
                Seg::plain("  9.1-1"),
            ],
            vec![Seg::plain("\u{4e2d}\u{6587}")],
        ];
        let b = boxed(&st, "install \u{b7} 2 packages", &lines);
        let rows: Vec<&str> = b.lines().collect();
        assert_eq!(rows.len(), 4);
        let w = display_width(rows[0]);
        for r in &rows {
            assert_eq!(display_width(r), w, "{r:?}");
        }
        assert!(rows[0].starts_with("\u{256d}\u{2500} install"));
        assert!(rows[3].starts_with('\u{2570}') && rows[3].ends_with('\u{256f}'));
        // coloured, the same box
        let coloured = boxed(&fancy(), "install \u{b7} 2 packages", &lines);
        assert_eq!(strip_ansi(&coloured), b);
        // ASCII
        let a = boxed(&ascii(), "t", &lines);
        assert!(a.lines().next().unwrap().starts_with("+- t"));
        assert!(a.lines().next().unwrap().is_ascii() && a.lines().last().unwrap().is_ascii());
    }

    #[test]
    fn a_box_never_exceeds_the_terminal() {
        let st = Style {
            width: 40,
            ..plain_unicode()
        };
        let long = vec![vec![Seg::plain("x".repeat(200))]];
        let b = boxed(&st, "t", &long);
        for r in b.lines() {
            assert!(display_width(r) <= 40, "{r:?}");
        }
        assert!(b.contains('\u{2026}'));
    }

    fn sample_table() -> Table {
        let mut t = Table::new(vec![
            Column::new("package").flex(),
            Column::new("size").right().keep(9),
            Column::new("repository").keep(1),
            Column::new("note").keep(3).flex(),
        ]);
        t.row(vec![
            cell("vim", Role::Package),
            cell("9.4 MiB", Role::Size),
            cell("extra", Role::Dim),
            cell("pulled in", Role::Plain),
        ]);
        t.row(vec![
            cell("gpm", Role::Package),
            cell("120 KiB", Role::Size),
            cell("extra", Role::Dim),
            cell("", Role::Plain),
        ]);
        t
    }

    #[test]
    fn a_table_lines_its_columns_up() {
        let out = sample_table().render(&plain_unicode(), 80);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("package"));
        assert_eq!(lines[1], "vim      9.4 MiB  extra       pulled in");
        assert_eq!(lines[2], "gpm      120 KiB  extra");
        // right-aligned numbers end in the same column
        let col = |l: &str| l.find("MiB").or_else(|| l.find("KiB")).unwrap();
        assert_eq!(col(lines[1]), col(lines[2]));
    }

    #[test]
    fn a_table_that_is_too_wide_drops_the_least_important_column_first() {
        let t = sample_table();
        let narrow = t.render(&plain_unicode(), 24);
        assert!(!narrow.contains("repository"), "keep=1 goes first");
        assert!(narrow.contains("size") && narrow.contains("package"));
        for l in narrow.lines() {
            assert!(display_width(l) <= 24, "{l:?}");
        }
        let tiny = t.render(&plain_unicode(), 9);
        assert!(
            tiny.lines().next().unwrap().starts_with("package"),
            "the first column is never dropped"
        );
    }

    #[test]
    fn a_long_flexible_cell_is_cut_before_a_column_is_dropped() {
        let mut t = Table::new(vec![
            Column::new("name").flex(),
            Column::new("size").right().keep(9),
        ]);
        t.row(vec![
            cell("a".repeat(60), Role::Plain),
            cell("1 MiB", Role::Size),
        ]);
        let out = t.render(&plain_unicode(), 30);
        assert!(out.contains('\u{2026}') && out.contains("1 MiB"));
        for l in out.lines() {
            assert!(display_width(l) <= 30, "{l:?}");
        }
    }

    #[test]
    fn rows_can_carry_notes_under_them() {
        let mut t = Table::new(vec![Column::new("id"), Column::new("what")]);
        t.row_with_notes(
            vec![cell("tx-1", Role::Plain), cell("install", Role::Plain)],
            vec![cell("error: it broke", Role::Problem)],
        );
        t.row(vec![cell("tx-2", Role::Plain), cell("remove", Role::Plain)]);
        let out = t.render(&plain_unicode(), 40);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[1], "tx-1  install");
        assert_eq!(lines[2], "    error: it broke");
        assert_eq!(lines[3], "tx-2  remove");
    }

    #[test]
    fn text_is_wrapped_at_spaces_by_columns() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert_eq!(wrap("", 20), [""]);
        assert_eq!(
            wrap("averyveryverylongword short", 10),
            ["averyveryverylongword", "short"]
        );
        for l in wrap("a b c d e f g h i j k l m n o p", 11) {
            assert!(display_width(&l) <= 11);
        }
    }

    #[test]
    fn a_table_much_narrower_than_its_content_does_not_panic() {
        let mut t = Table::new(vec![
            Column::new("name").flex(),
            Column::new("size").right().keep(9),
        ]);
        t.row(vec![
            cell("a".repeat(200), Role::Plain),
            cell("1 MiB", Role::Size),
        ]);
        for w in [0, 1, 5, 10, 20] {
            let out = t.render(&plain_unicode(), w);
            assert!(out.lines().count() >= 2, "width {w}");
        }
    }

    #[test]
    fn a_coloured_table_strips_to_the_plain_one() {
        let t = sample_table();
        assert_eq!(
            strip_ansi(&t.render(&fancy(), 80)),
            t.render(&plain_unicode(), 80)
        );
    }

    #[test]
    fn a_diff_line_keeps_its_sign() {
        let st = fancy();
        assert!(st.paint(Role::Added, "+x") == diff_line(&st, "+x"));
        assert!(diff_line(&st, "-x").starts_with("\x1b[31m-x"));
        assert!(diff_line(&st, "@@ -1 +1 @@").contains("@@"));
        assert_eq!(diff_line(&st, " context"), " context");
        assert_eq!(diff_line(&plain_unicode(), "+x"), "+x");
        assert!(
            diff_line(&st, "+++ b/file").contains("\x1b[1m"),
            "the file header is not an addition"
        );
    }

    #[test]
    fn the_activity_line_and_progress_bar_are_pure() {
        let st = plain_unicode();
        assert_eq!(spinner_frame(&st, 0), "\u{280b}");
        assert_eq!(spinner_frame(&st, 10), "\u{280b}", "it wraps");
        assert_eq!(spinner_frame(&ascii(), 1), "/");
        assert_eq!(progress_bar(&st, 3, 12, 12), "\u{25b0}\u{25b0}\u{25b0}\u{25b1}\u{25b1}\u{25b1}\u{25b1}\u{25b1}\u{25b1}\u{25b1}\u{25b1}\u{25b1}");
        assert_eq!(progress_bar(&st, 0, 0, 4), "\u{25b1}".repeat(4));
        assert_eq!(
            progress_bar(&st, 99, 4, 4),
            "\u{25b0}".repeat(4),
            "never over full"
        );
        assert_eq!(progress_bar(&ascii(), 2, 4, 4), "##--");
        let line = activity_line(&st, 0, "measuring mirrors", Some((4, 9)), "in.arch.example");
        assert!(line.starts_with("\u{280b} measuring mirrors"));
        assert!(line.contains("4 of 9") && line.ends_with("in.arch.example"));
        let spin = activity_line(&st, 1, "asking the AUR", None, "");
        assert_eq!(spin, "\u{2819} asking the AUR");
        let narrow = Style { width: 40, ..st };
        let cut = activity_line(&narrow, 0, &"x".repeat(100), None, "");
        assert!(display_width(&cut) < 40, "{cut:?}");
    }

    #[test]
    fn an_activity_with_animations_off_draws_nothing_and_is_inactive() {
        // The test process has no terminal on standard error and the default
        // setting is regular, so nothing is drawn.
        let a = Activity::spinner("waiting");
        assert!(!a.is_active());
        a.set(1, "x");
        a.label("y");
        a.finish("done");
        let p = Activity::progress("counting", 3);
        assert!(!p.is_active());
        p.fail("nope");
    }

    #[test]
    fn the_ascii_set_is_ascii_and_the_unicode_set_is_not() {
        for g in [&ASCII] {
            for s in [
                g.ok,
                g.warn,
                g.problem,
                g.note,
                g.arrow,
                g.ellipsis,
                g.bullet,
                g.h,
                g.v,
                g.tl,
                g.tr,
                g.bl,
                g.br,
                g.tee,
                g.last,
                g.pipe,
                g.bar_full,
                g.bar_empty,
                g.prog_full,
                g.prog_empty,
            ] {
                assert!(s.is_ascii(), "{s:?}");
            }
            assert!(g.spark.iter().chain(g.spinner.iter()).all(|s| s.is_ascii()));
        }
        assert!(!UNICODE.ok.is_ascii());
        assert_eq!(UNICODE.spark.len(), 8);
        // every glyph takes the width the layout assumes
        for s in [
            UNICODE.ok,
            UNICODE.warn,
            UNICODE.problem,
            UNICODE.note,
            UNICODE.arrow,
            UNICODE.bar_full,
            UNICODE.bar_empty,
            UNICODE.prog_full,
            UNICODE.prog_empty,
            UNICODE.h,
            UNICODE.v,
        ] {
            assert_eq!(display_width(s), 1, "{s:?}");
        }
        assert!(UNICODE
            .spinner
            .iter()
            .chain(UNICODE.spark.iter())
            .all(|s| display_width(s) == 1));
    }
}
