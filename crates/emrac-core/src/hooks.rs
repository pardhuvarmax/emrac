//! Which pacman hooks a transaction will run, worked out before it runs.
//!
//! pacman reads `*.hook` files from `/usr/share/libalpm/hooks` and the
//! `HookDir` entries of `pacman.conf`, and runs the ones whose triggers match
//! the transaction: before it (`PreTransaction`, where a failure can cancel
//! everything) and after it. It prints nothing about them until they run.
//!
//! The rules here were written from `alpm-hooks(5)` and then checked by running
//! real hooks in a container (`research/alpm-hooks.md`); see
//! `docs/plan/r3/u4-hook-preview.md`. Everything but [`load_hooks`] is pure.

use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// What a transaction does to one package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Install,
    Upgrade,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerType {
    Package,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum When {
    Pre,
    Post,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub operations: Vec<Operation>,
    pub kind: TriggerType,
    /// In file order; a leading `!` is kept and means "not".
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hook {
    /// The file name, `.hook` included.
    pub file: String,
    pub description: Option<String>,
    pub when: When,
    pub exec: String,
    pub depends: Vec<String>,
    pub abort_on_fail: bool,
    pub needs_targets: bool,
    pub triggers: Vec<Trigger>,
}

impl Hook {
    /// The file name without `.hook`: what hooks are ordered and overridden by.
    pub fn name(&self) -> &str {
        self.file.strip_suffix(".hook").unwrap_or(&self.file)
    }
}

/// Reads one hook file. The errors are pacman's own rules for an unusable
/// hook: a trigger needs operations, a type and targets, an action needs
/// `Exec` and `When`, and `AbortOnFail` makes no sense after the transaction.
/// An option nobody knows is ignored.
pub fn parse_hook(file: &str, text: &str) -> Result<Hook, String> {
    #[derive(PartialEq)]
    enum Section {
        None,
        Trigger,
        Action,
    }
    let mut section = Section::None;
    let mut triggers: Vec<Trigger> = Vec::new();
    let mut pending: Option<(Vec<Operation>, Option<TriggerType>, Vec<String>)> = None;
    let mut description = None;
    let mut when = None;
    let mut exec = None;
    let mut depends = Vec::new();
    let (mut abort_on_fail, mut needs_targets) = (false, false);

    fn close(
        pending: &mut Option<(Vec<Operation>, Option<TriggerType>, Vec<String>)>,
        triggers: &mut Vec<Trigger>,
        file: &str,
    ) -> Result<(), String> {
        if let Some((operations, kind, targets)) = pending.take() {
            if targets.is_empty() {
                return Err(format!("Missing trigger targets in hook: {file}"));
            }
            if operations.is_empty() {
                return Err(format!("Missing trigger operation in hook: {file}"));
            }
            let Some(kind) = kind else {
                return Err(format!("Missing trigger type in hook: {file}"));
            };
            triggers.push(Trigger {
                operations,
                kind,
                targets,
            });
        }
        Ok(())
    }

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            close(&mut pending, &mut triggers, file)?;
            match name {
                "Trigger" => {
                    section = Section::Trigger;
                    pending = Some((Vec::new(), None, Vec::new()));
                }
                "Action" => section = Section::Action,
                other => return Err(format!("hook {file}: unknown section [{other}]")),
            }
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((k, v)) => (k.trim(), Some(v.trim())),
            None => (line, None),
        };
        match (&section, key, value) {
            (Section::Trigger, "Operation", Some(v)) => {
                let op = match v {
                    "Install" => Operation::Install,
                    "Upgrade" => Operation::Upgrade,
                    "Remove" => Operation::Remove,
                    other => return Err(format!("hook {file}: invalid operation {other}")),
                };
                if let Some(p) = pending.as_mut() {
                    p.0.push(op);
                }
            }
            (Section::Trigger, "Type", Some(v)) => {
                let kind = match v {
                    "Package" => TriggerType::Package,
                    // `File` is the deprecated name of `Path`.
                    "Path" | "File" => TriggerType::Path,
                    other => return Err(format!("hook {file}: invalid type {other}")),
                };
                if let Some(p) = pending.as_mut() {
                    p.1 = Some(kind);
                }
            }
            (Section::Trigger, "Target", Some(v)) => {
                if let Some(p) = pending.as_mut() {
                    p.2.push(v.to_string());
                }
            }
            (Section::Action, "Description", Some(v)) => description = Some(v.to_string()),
            (Section::Action, "When", Some(v)) => {
                when = Some(match v {
                    "PreTransaction" => When::Pre,
                    "PostTransaction" => When::Post,
                    other => return Err(format!("hook {file}: invalid When {other}")),
                })
            }
            (Section::Action, "Exec", Some(v)) => exec = Some(v.to_string()),
            (Section::Action, "Depends", Some(v)) => depends.push(v.to_string()),
            (Section::Action, "AbortOnFail", _) => abort_on_fail = true,
            (Section::Action, "NeedsTargets", _) => needs_targets = true,
            _ => {}
        }
    }
    close(&mut pending, &mut triggers, file)?;

    if triggers.is_empty() {
        return Err(format!("Missing trigger targets in hook: {file}"));
    }
    let Some(exec) = exec else {
        return Err(format!("Missing Exec option in hook: {file}"));
    };
    let Some(when) = when else {
        return Err(format!("Missing When option in hook: {file}"));
    };
    if abort_on_fail && when == When::Post {
        return Err(format!("AbortOnFail set for PostTransaction hook: {file}"));
    }
    Ok(Hook {
        file: file.to_string(),
        description,
        when,
        exec,
        depends,
        abort_on_fail,
        needs_targets,
        triggers,
    })
}

/// Hooks found on disk, and the files that could not be used.
#[derive(Debug, Default)]
pub struct LoadedHooks {
    pub hooks: Vec<Hook>,
    /// `(file name, why)` for each hook pacman would refuse or that could not be read.
    pub unusable: Vec<(String, String)>,
}

/// Reads the hook directories, `dirs` ordered from the lowest priority (the
/// system directory) to the highest: a file name seen in a higher directory
/// replaces the same name in a lower one, and a symlink to `/dev/null`
/// disables it. The result is in the order pacman runs them.
pub fn load_hooks(dirs: &[PathBuf]) -> LoadedHooks {
    let mut out = LoadedHooks::default();
    let mut claimed: HashSet<String> = HashSet::new();
    for dir in dirs.iter().rev() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(".hook"))
            .collect();
        names.sort();
        for name in names {
            if !claimed.insert(name.clone()) {
                continue;
            }
            let path = dir.join(&name);
            if path.is_dir() {
                continue;
            }
            if std::fs::canonicalize(&path).is_ok_and(|p| p == Path::new("/dev/null")) {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => match parse_hook(&name, &text) {
                    Ok(hook) => out.hooks.push(hook),
                    Err(why) => out.unusable.push((name, why)),
                },
                Err(err) => out.unusable.push((name, err.to_string())),
            }
        }
    }
    sort_hooks(&mut out.hooks);
    out
}

/// pacman's order: by file name with `.hook` removed, byte by byte.
pub fn sort_hooks(hooks: &mut [Hook]) {
    hooks.sort_by(|a, b| a.name().as_bytes().cmp(b.name().as_bytes()));
}

/// Shell-style matching as `fnmatch` does it with no flags: `*` and `?` match
/// any character including `/`, `[...]` is a class (`!` or `^` negates it),
/// and a backslash quotes the next character.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    fn go(p: &[char], t: &[char]) -> bool {
        let Some((&c, rest)) = p.split_first() else {
            return t.is_empty();
        };
        match c {
            '*' => (0..=t.len()).any(|i| go(rest, &t[i..])),
            '?' => !t.is_empty() && go(rest, &t[1..]),
            '[' => {
                let Some(&ch) = t.first() else {
                    return false;
                };
                match class(rest, ch) {
                    Some((hit, after)) => hit && go(after, &t[1..]),
                    // No closing bracket: the bracket is an ordinary character.
                    None => ch == '[' && go(rest, &t[1..]),
                }
            }
            '\\' => match rest.split_first() {
                Some((&q, after)) => t.first() == Some(&q) && go(after, &t[1..]),
                None => t.first() == Some(&'\\') && t.len() == 1,
            },
            _ => t.first() == Some(&c) && go(rest, &t[1..]),
        }
    }
    /// A bracket class after its `[`: whether `ch` is in it, and what follows.
    fn class(p: &[char], ch: char) -> Option<(bool, &[char])> {
        let mut i = 0;
        let negate = matches!(p.first(), Some('!') | Some('^'));
        if negate {
            i += 1;
        }
        let start = i;
        let mut hit = false;
        loop {
            let &c = p.get(i)?;
            if c == ']' && i > start {
                return Some((hit != negate, &p[i + 1..]));
            }
            if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|&e| e != ']') {
                let end = p[i + 2];
                if c <= ch && ch <= end {
                    hit = true;
                }
                i += 3;
            } else {
                if c == ch {
                    hit = true;
                }
                i += 1;
            }
        }
    }
    go(&p, &t)
}

/// The targets of `trigger` that match `candidate`: targets are read in order
/// and the last one that matches decides, `!` meaning "no".
fn target_matches(targets: &[String], candidate: &str) -> bool {
    let mut matched = false;
    for target in targets {
        match target.strip_prefix('!') {
            Some(pattern) => {
                if glob_match(pattern, candidate) {
                    matched = false;
                }
            }
            None => {
                if glob_match(target, candidate) {
                    matched = true;
                }
            }
        }
    }
    matched
}

/// What a transaction does to one package, with the files libalpm knows about.
#[derive(Debug, Clone)]
pub struct PackageFact {
    pub name: String,
    pub operation: Operation,
    /// The files the package puts on the system (install, upgrade) or takes
    /// off it (remove), directories with a trailing `/`. `None`: not known.
    pub files: Option<Vec<String>>,
    /// For an upgrade, the files of the version being replaced.
    pub old_files: Vec<String>,
}

/// One hook that will run.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlannedHook {
    pub file: String,
    pub description: Option<String>,
    pub when: When,
    pub exec: String,
    pub abort_on_fail: bool,
    pub needs_targets: bool,
    /// What matched, sorted and without repeats: what the hook reads on stdin
    /// with `NeedsTargets`. Package names or paths.
    pub targets: Vec<String>,
    /// `Depends` nothing installed (or coming) provides: pacman cannot run the
    /// hook and treats it as failed.
    pub missing_depends: Vec<String>,
}

impl PlannedHook {
    /// pacman cancels the transaction when this hook fails.
    pub fn would_abort(&self) -> bool {
        self.abort_on_fail && !self.missing_depends.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct HookPreview {
    /// Before the transaction, then after it, each in pacman's order.
    pub hooks: Vec<PlannedHook>,
    /// Packages whose file list is unknown, so a hook with a path trigger
    /// could not be checked against them (they need `pacman -Fy`).
    pub unchecked: Vec<String>,
    /// Hooks pacman could not use, with the reason.
    pub unusable: Vec<(String, String)>,
    /// Hooks the transaction itself puts on the system, as `file (package)`.
    /// pacman reads the post-transaction hooks again after the files are
    /// extracted, so these run too, but what they do is inside the package.
    pub shipped: Vec<String>,
}

impl HookPreview {
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
            && self.unchecked.is_empty()
            && self.unusable.is_empty()
            && self.shipped.is_empty()
    }
}

/// Where a package puts hooks that pacman reads.
const SHIPPED_HOOK_DIR: &str = "usr/share/libalpm/hooks/";

/// The hooks `hooks` (already in order) that this transaction runs.
/// `satisfied(dep)` says whether something installed or coming provides
/// `dep`; `exists(path)` whether a file is already on the system (`path`
/// without a leading `/`).
pub fn plan_hooks(
    hooks: &[Hook],
    facts: &[PackageFact],
    satisfied: &dyn Fn(&str) -> bool,
    exists: &dyn Fn(&str) -> bool,
) -> HookPreview {
    let mut preview = HookPreview::default();
    let mut unchecked: BTreeSet<String> = BTreeSet::new();

    for hook in hooks {
        let mut targets: BTreeSet<String> = BTreeSet::new();
        for trigger in &hook.triggers {
            for fact in facts {
                match trigger.kind {
                    TriggerType::Package => {
                        if trigger.operations.contains(&fact.operation)
                            && target_matches(&trigger.targets, &fact.name)
                        {
                            targets.insert(fact.name.clone());
                        }
                    }
                    TriggerType::Path => {
                        let Some(files) = &fact.files else {
                            // Only worth reporting when the operation could apply.
                            if trigger.operations.contains(&fact.operation)
                                || (fact.operation == Operation::Upgrade
                                    && trigger.operations.contains(&Operation::Install))
                            {
                                unchecked.insert(fact.name.clone());
                            }
                            continue;
                        };
                        for file in files {
                            let op = match fact.operation {
                                Operation::Remove => Operation::Remove,
                                _ => {
                                    let known = fact.old_files.iter().any(|f| f == file)
                                        || exists(file.trim_end_matches('/'));
                                    if known {
                                        Operation::Upgrade
                                    } else {
                                        Operation::Install
                                    }
                                }
                            };
                            if trigger.operations.contains(&op)
                                && target_matches(&trigger.targets, file)
                            {
                                targets.insert(file.clone());
                            }
                        }
                    }
                }
            }
        }
        if targets.is_empty() {
            continue;
        }
        let missing_depends: Vec<String> = hook
            .depends
            .iter()
            .filter(|d| !satisfied(d))
            .cloned()
            .collect();
        preview.hooks.push(PlannedHook {
            file: hook.file.clone(),
            description: hook.description.clone(),
            when: hook.when,
            exec: hook.exec.clone(),
            abort_on_fail: hook.abort_on_fail,
            needs_targets: hook.needs_targets,
            targets: targets.into_iter().collect(),
            missing_depends,
        });
    }
    // Stable: the hooks keep their own order inside each pass.
    preview.hooks.sort_by_key(|h| h.when);
    preview.unchecked = unchecked.into_iter().collect();

    // Hooks coming with the transaction that are not on the system yet.
    for fact in facts.iter().filter(|f| f.operation != Operation::Remove) {
        for file in fact.files.iter().flatten() {
            let Some(name) = file.strip_prefix(SHIPPED_HOOK_DIR) else {
                continue;
            };
            if name.ends_with(".hook")
                && !name.contains('/')
                && !hooks.iter().any(|h| h.file == name)
            {
                preview.shipped.push(format!("{name} ({})", fact.name));
            }
        }
    }
    preview.shipped.sort();
    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(file: &str, text: &str) -> Hook {
        parse_hook(file, text).unwrap()
    }

    /// The probe hooks of the container run, one trigger each.
    fn probe(file: &str, ops: &[&str], kind: &str, targets: &[&str]) -> Hook {
        let mut text = String::from("[Trigger]\n");
        for o in ops {
            text.push_str(&format!("Operation = {o}\n"));
        }
        text.push_str(&format!("Type = {kind}\n"));
        for t in targets {
            text.push_str(&format!("Target = {t}\n"));
        }
        text.push_str("[Action]\nWhen = PostTransaction\nExec = /usr/bin/true\n");
        hook(file, &text)
    }

    fn fact(name: &str, op: Operation, files: Option<&[&str]>, old: &[&str]) -> PackageFact {
        PackageFact {
            name: name.into(),
            operation: op,
            files: files.map(|f| f.iter().map(|s| s.to_string()).collect()),
            old_files: old.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn names(preview: &HookPreview) -> Vec<&str> {
        preview.hooks.iter().map(|h| h.file.as_str()).collect()
    }

    const TP_FILES: &[&str] = &[
        "usr/share/tp/",
        "usr/share/tp/a.txt",
        "usr/share/tp/sub/",
        "usr/share/tp/sub/deep.txt",
    ];

    fn run(hooks: &[Hook], facts: &[PackageFact], disk: &[&str]) -> HookPreview {
        let on_disk: HashSet<String> = disk.iter().map(|s| s.to_string()).collect();
        plan_hooks(hooks, facts, &|_| true, &|p| on_disk.contains(p))
    }

    #[test]
    fn a_hook_file_is_read_with_every_option() {
        let h = hook(
            "30-x.hook",
            "# comment\n[Trigger]\nOperation = Install\nOperation = Upgrade\nType = File\n\
             Target = usr/lib/*.so\n[Action]\nDescription = Doing x\nWhen = PreTransaction\n\
             Exec = /usr/bin/x --flag\nDepends = bash\nDepends = coreutils\nAbortOnFail\nNeedsTargets\n",
        );
        assert_eq!(h.name(), "30-x");
        assert_eq!(h.when, When::Pre);
        assert!(h.abort_on_fail && h.needs_targets);
        assert_eq!(h.depends, ["bash", "coreutils"]);
        assert_eq!(h.triggers[0].kind, TriggerType::Path); // File is an alias of Path
        assert_eq!(h.triggers[0].operations.len(), 2);
        assert_eq!(h.description.as_deref(), Some("Doing x"));
    }

    #[test]
    fn a_hook_pacman_would_refuse_is_refused_with_its_reason() {
        let bad = |text: &str| parse_hook("b.hook", text).unwrap_err();
        let action = "[Action]\nWhen = PostTransaction\nExec = /bin/true\n";
        assert!(bad(&format!(
            "[Trigger]\nOperation = Install\nType = Path\n{action}"
        ))
        .contains("Missing trigger targets"));
        assert!(
            bad(&format!("[Trigger]\nType = Path\nTarget = x\n{action}"))
                .contains("Missing trigger operation")
        );
        assert!(bad(&format!(
            "[Trigger]\nOperation = Install\nTarget = x\n{action}"
        ))
        .contains("Missing trigger type"));
        let trig = "[Trigger]\nOperation = Install\nType = Path\nTarget = x\n";
        assert!(bad(&format!("{trig}[Action]\nWhen = PostTransaction\n")).contains("Missing Exec"));
        assert!(bad(&format!("{trig}[Action]\nExec = /bin/true\n")).contains("Missing When"));
        assert!(bad(&format!("{trig}{action}AbortOnFail\n"))
            .contains("AbortOnFail set for PostTransaction"));
    }

    #[test]
    fn hooks_run_in_name_order_without_the_suffix() {
        // 10-A < 10-a < 10-a-b < 10-a_b, and 9-x after 65-x: plain bytes.
        let mut hooks: Vec<Hook> = ["9-x", "10-a_b", "10-a", "65-x", "10-A", "10-a-b"]
            .iter()
            .map(|n| probe(&format!("{n}.hook"), &["Install"], "Package", &["tp"]))
            .collect();
        sort_hooks(&mut hooks);
        let order: Vec<&str> = hooks.iter().map(|h| h.name()).collect();
        assert_eq!(order, ["10-A", "10-a", "10-a-b", "10-a_b", "65-x", "9-x"]);
    }

    #[test]
    fn globs_follow_fnmatch_with_no_flags() {
        assert!(glob_match("usr/share/*.txt", "usr/share/tp/a.txt")); // * crosses /
        assert!(glob_match("usr/share/tp/?.txt", "usr/share/tp/a.txt"));
        assert!(!glob_match("usr/share/tp/?.txt", "usr/share/tp/ab.txt"));
        assert!(glob_match("usr/share/tp/[a-b].txt", "usr/share/tp/a.txt"));
        assert!(!glob_match("usr/share/tp/[c-d].txt", "usr/share/tp/a.txt"));
        assert!(glob_match("[!c-d]x", "ax"));
        assert!(glob_match("t*", "tp"));
        assert!(!glob_match("t*", "xtp"));
        assert!(glob_match("a\\*b", "a*b"));
        assert!(!glob_match("a\\*b", "axb"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn package_triggers_match_by_name_operation_and_negation() {
        let facts = [fact("tp", Operation::Install, Some(TP_FILES), &[])];
        let hooks = [
            probe("a.hook", &["Install", "Upgrade"], "Package", &["tp"]),
            probe("b.hook", &["Install"], "Package", &["t*"]),
            probe("c.hook", &["Upgrade"], "Package", &["tp"]),
            probe("d.hook", &["Remove"], "Package", &["tp"]),
            probe("e.hook", &["Install"], "Package", &["*", "!tp"]),
            probe("f.hook", &["Install"], "Package", &["!tp"]),
        ];
        assert_eq!(names(&run(&hooks, &facts, &[])), ["a.hook", "b.hook"]);
    }

    #[test]
    fn the_last_matching_target_decides() {
        let facts = [fact("tp", Operation::Install, Some(TP_FILES), &[])];
        let hooks = [
            probe(
                "a.hook",
                &["Install"],
                "Path",
                &["!usr/share/tp/a.txt", "usr/share/tp/a.txt"],
            ),
            probe(
                "b.hook",
                &["Install"],
                "Path",
                &["usr/share/tp/a.txt", "!usr/share/tp/a.txt"],
            ),
            probe("c.hook", &["Install"], "Path", &["!usr/share/tp/zzz"]),
        ];
        assert_eq!(names(&run(&hooks, &facts, &[])), ["a.hook"]);
    }

    #[test]
    fn a_path_is_an_install_unless_the_old_version_or_the_disk_has_it() {
        let new_files = [
            "usr/share/tp/",
            "usr/share/tp/a.txt",
            "usr/share/tp/new.txt",
        ];
        let upgrade = [fact(
            "tp",
            Operation::Upgrade,
            Some(&new_files),
            &["usr/share/tp/", "usr/share/tp/a.txt"],
        )];
        let hooks = [
            probe(
                "inst-new.hook",
                &["Install"],
                "Path",
                &["usr/share/tp/new.txt"],
            ),
            probe(
                "upg-new.hook",
                &["Upgrade"],
                "Path",
                &["usr/share/tp/new.txt"],
            ),
            probe("inst-a.hook", &["Install"], "Path", &["usr/share/tp/a.txt"]),
            probe("upg-a.hook", &["Upgrade"], "Path", &["usr/share/tp/a.txt"]),
        ];
        assert_eq!(
            names(&run(&hooks, &upgrade, &[])),
            ["inst-new.hook", "upg-a.hook"]
        );
        // Already on the system although the old version did not own it: an upgrade.
        assert_eq!(
            names(&run(&hooks, &upgrade, &["usr/share/tp/new.txt"])),
            ["upg-new.hook", "upg-a.hook"]
        );
    }

    #[test]
    fn a_removal_matches_every_file_the_package_owned() {
        let remove = [fact("tp", Operation::Remove, Some(TP_FILES), &[])];
        let hooks = [
            probe("a.hook", &["Remove"], "Path", &["usr/share/tp/a.txt"]),
            probe("b.hook", &["Remove"], "Package", &["tp"]),
            probe("c.hook", &["Install"], "Path", &["usr/share/tp/a.txt"]),
        ];
        assert_eq!(names(&run(&hooks, &remove, &[])), ["a.hook", "b.hook"]);
    }

    #[test]
    fn any_trigger_is_enough_and_the_targets_are_what_matched() {
        let text = "[Trigger]\nOperation = Install\nType = Package\nTarget = nomatch\n\
                    [Trigger]\nOperation = Install\nType = Path\nTarget = usr/share/tp/*.txt\n\
                    [Action]\nWhen = PreTransaction\nExec = /bin/true\nNeedsTargets\n";
        let facts = [fact("tp", Operation::Install, Some(TP_FILES), &[])];
        let preview = run(&[hook("m.hook", text)], &facts, &[]);
        assert_eq!(preview.hooks.len(), 1);
        assert_eq!(
            preview.hooks[0].targets,
            ["usr/share/tp/a.txt", "usr/share/tp/sub/deep.txt"]
        );
        assert!(preview.hooks[0].needs_targets);
    }

    #[test]
    fn pre_hooks_come_first_and_each_pass_keeps_its_order() {
        let pre = "[Trigger]\nOperation = Install\nType = Package\nTarget = tp\n\
                   [Action]\nWhen = PreTransaction\nExec = /bin/true\n";
        let mut hooks = vec![
            probe("10-post.hook", &["Install"], "Package", &["tp"]),
            hook("20-pre.hook", pre),
            probe("30-post.hook", &["Install"], "Package", &["tp"]),
        ];
        sort_hooks(&mut hooks);
        let facts = [fact("tp", Operation::Install, None, &[])];
        assert_eq!(
            names(&run(&hooks, &facts, &[])),
            ["20-pre.hook", "10-post.hook", "30-post.hook"]
        );
    }

    #[test]
    fn a_missing_depends_is_reported_and_aborts_only_with_abort_on_fail() {
        let text = |abort: &str| {
            format!(
                "[Trigger]\nOperation = Install\nType = Package\nTarget = tp\n\
                 [Action]\nWhen = PreTransaction\nExec = /bin/true\nDepends = dkms\nDepends = bash\n{abort}"
            )
        };
        let facts = [fact("tp", Operation::Install, None, &[])];
        let satisfied = |d: &str| d == "bash";
        let strict = plan_hooks(
            &[hook("a.hook", &text("AbortOnFail\n"))],
            &facts,
            &satisfied,
            &|_| false,
        );
        assert_eq!(strict.hooks[0].missing_depends, ["dkms"]);
        assert!(strict.hooks[0].would_abort());
        let lax = plan_hooks(&[hook("a.hook", &text(""))], &facts, &satisfied, &|_| false);
        assert_eq!(lax.hooks[0].missing_depends, ["dkms"]);
        assert!(!lax.hooks[0].would_abort());
    }

    #[test]
    fn a_package_without_a_file_list_is_reported_not_guessed() {
        let facts = [fact("tp", Operation::Install, None, &[])];
        let hooks = [
            probe("path.hook", &["Install"], "Path", &["usr/share/tp/a.txt"]),
            probe("pkg.hook", &["Install"], "Package", &["tp"]),
            probe("rm.hook", &["Remove"], "Path", &["usr/share/tp/a.txt"]),
        ];
        let preview = run(&hooks, &facts, &[]);
        assert_eq!(names(&preview), ["pkg.hook"]);
        assert_eq!(preview.unchecked, ["tp"]);
    }

    #[test]
    fn hooks_a_package_brings_are_listed_unless_already_installed() {
        let files = [
            "usr/share/libalpm/hooks/",
            "usr/share/libalpm/hooks/new-one.hook",
            "usr/share/libalpm/hooks/already.hook",
            "usr/share/libalpm/hooks/sub/deep.hook",
            "usr/share/libalpm/scripts/x.sh",
        ];
        let facts = [
            fact("perl", Operation::Install, Some(&files), &[]),
            fact("gone", Operation::Remove, Some(&files), &[]),
        ];
        let already = probe("already.hook", &["Remove"], "Package", &["nothing"]);
        let preview = run(&[already], &facts, &[]);
        assert_eq!(preview.shipped, ["new-one.hook (perl)"]);
        assert!(!preview.is_empty());
    }

    #[test]
    fn a_custom_directory_replaces_the_system_one_and_dev_null_disables() {
        let tmp = std::env::temp_dir().join(format!("emrac-hooks-{}", std::process::id()));
        let (sys, custom) = (tmp.join("sys"), tmp.join("custom"));
        std::fs::create_dir_all(&sys).unwrap();
        std::fs::create_dir_all(&custom).unwrap();
        let body = |d: &str| {
            format!(
                "[Trigger]\nOperation = Install\nType = Package\nTarget = tp\n\
                 [Action]\nDescription = {d}\nWhen = PostTransaction\nExec = /bin/true\n"
            )
        };
        std::fs::write(sys.join("50-prec.hook"), body("from system")).unwrap();
        std::fs::write(custom.join("50-prec.hook"), body("from custom")).unwrap();
        std::fs::write(sys.join("20-off.hook"), body("disabled")).unwrap();
        std::os::unix::fs::symlink("/dev/null", custom.join("20-off.hook")).unwrap();
        std::fs::write(sys.join("30-bad.hook"), "[Action]\nExec = x\n").unwrap();
        std::fs::write(sys.join("notes.txt"), "not a hook").unwrap();

        let loaded = load_hooks(&[sys.clone(), custom.clone()]);
        let files: Vec<&str> = loaded.hooks.iter().map(|h| h.file.as_str()).collect();
        assert_eq!(files, ["50-prec.hook"]);
        assert_eq!(loaded.hooks[0].description.as_deref(), Some("from custom"));
        assert_eq!(loaded.unusable.len(), 1);
        assert_eq!(loaded.unusable[0].0, "30-bad.hook");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
