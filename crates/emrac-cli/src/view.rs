//! The layouts emrac uses on a terminal (u17): pure functions from data and a
//! `Style` to text. The commands call them only when `Style::fancy` is set (a
//! real terminal); piped, redirected, `--json` and on a dumb terminal they keep
//! printing what they always printed. See `docs/plan/r3/u17-cli-style.md`.

use std::collections::{HashMap, HashSet};

use emrac_core::benchmark::BenchRecord;
use emrac_core::build_stats::BuildStats;
use emrac_core::bundle::{Change, ChangeKind};
use emrac_core::mirrors::{Group, Ranked, Sample};
use emrac_core::style::{
    bar, boxed, cell, heading, mark, mark_width, pad_right, render_line, sparkline, version_change,
    wrap, Column, Line, Role, Seg, Status, Style, Table,
};
use emrac_core::timeline::{self, Event, Kind};
use emrac_core::{
    DoctorCheck, DoctorStatus, Orphan, Pin, Plan, PlanAction, Snapshot, TransactionRecord, TxStatus,
};

use crate::output::human_size;

fn plural(n: usize, singular: &str) -> String {
    if n == 1 {
        format!("{n} {singular}")
    } else {
        format!("{n} {singular}s")
    }
}

fn verb(action: PlanAction) -> &'static str {
    match action {
        PlanAction::Install => "install",
        PlanAction::Remove => "remove",
        PlanAction::Upgrade => "upgrade",
        PlanAction::Downgrade => "downgrade",
        PlanAction::Reinstall => "reinstall",
        PlanAction::Purge => "purge",
    }
}

// ---------------------------------------------------------------------- plan

/// The packages of a plan in a box: repository, name, version (what changed in
/// it picked out for an upgrade), and which were pulled in.
pub fn plan_box(st: &Style, plan: &Plan) -> String {
    let rw = plan
        .packages
        .iter()
        .map(|p| p.repo.chars().count())
        .max()
        .unwrap_or(0);
    let nw = plan
        .packages
        .iter()
        .map(|p| p.name.chars().count())
        .max()
        .unwrap_or(0);
    let versions: Vec<Line> = plan
        .packages
        .iter()
        .map(|p| match &p.previous_version {
            Some(prev) if prev != &p.version => version_change(st, prev, &p.version),
            _ => vec![Seg::new(p.version.clone(), Role::Version)],
        })
        .collect();
    let vw = versions
        .iter()
        .map(emrac_core::style::line_width)
        .max()
        .unwrap_or(0);
    let lines: Vec<Line> = plan
        .packages
        .iter()
        .zip(versions)
        .map(|(p, mut v)| {
            let pad = vw - emrac_core::style::line_width(&v);
            let mut line = vec![
                Seg::new(pad_right(&p.repo, rw), Role::Dim),
                Seg::plain("  "),
                Seg::new(pad_right(&p.name, nw), Role::Package),
                Seg::plain("  "),
            ];
            line.append(&mut v);
            if !p.explicit {
                line.push(Seg::plain(" ".repeat(pad + 2)));
                line.push(Seg::new("dependency", Role::Dim));
            }
            line
        })
        .collect();
    let title = format!(
        "{} {} {}",
        verb(plan.action),
        st.glyphs().bullet,
        plural(plan.packages.len(), "package")
    );
    boxed(st, &title, &lines)
}

/// What the plan costs, in aligned lines; the disk bar is the share of the free
/// space on `/` (only when that is known and the plan grows the disk).
pub fn plan_summary(st: &Style, plan: &Plan, free: Option<u64>, digest: &str) -> String {
    let mut out = String::new();
    let key = |k: &str| st.paint(Role::Dim, &pad_right(k, 10));
    if plan.total_download_size > 0 {
        out.push_str(&format!(
            "  {}{}\n",
            key("download"),
            st.paint(Role::Size, &human_size(plan.total_download_size))
        ));
    }
    let (sign, size) = if plan.total_installed_size_delta >= 0 {
        ("+", plan.total_installed_size_delta as u64)
    } else {
        ("-", plan.total_installed_size_delta.unsigned_abs())
    };
    let mut disk = format!(
        "  {}{}",
        key("disk"),
        st.paint(Role::Size, &format!("{sign}{}", human_size(size)))
    );
    if let (Some(free), true) = (free, plan.total_installed_size_delta > 0) {
        if free > 0 {
            let frac = size as f64 / free as f64;
            let pct = frac * 100.0;
            let share = if pct < 0.1 {
                "<0.1%".to_string()
            } else {
                format!("{pct:.1}%")
            };
            disk.push_str(&format!(
                "   {}  {}",
                render_line(st, &bar(st, frac, 12)),
                st.paint(Role::Dim, &format!("{share} of the free space on /"))
            ));
        }
    }
    out.push_str(&disk);
    out.push('\n');
    out.push_str(&format!(
        "  {}{}   {}\n",
        key("plan id"),
        st.paint(Role::Heading, digest),
        st.paint(
            Role::Dim,
            &format!("(--expect-plan {digest} runs only this plan)")
        )
    ));
    out
}

// -------------------------------------------------------- doctor / pre-flight

fn status_of(s: DoctorStatus) -> Status {
    match s {
        DoctorStatus::Ok => Status::Ok,
        DoctorStatus::Note => Status::Note,
        DoctorStatus::Warning => Status::Warning,
        DoctorStatus::Problem => Status::Problem,
    }
}

/// The checks of the doctor or the pre-flight, one mark each; what is wrong
/// is wrapped under it, and the fix when there is one and the check is not ok.
pub fn checks_view(st: &Style, checks: &[DoctorCheck]) -> String {
    let tw = checks
        .iter()
        .map(|c| c.title.chars().count())
        .max()
        .unwrap_or(0);
    let indent = 1 + mark_width(st) + 2;
    // The summary goes beside the title when there is room, else under it.
    let beside = st.width.saturating_sub(indent + tw + 2);
    let side_by_side = beside >= 30;
    let under_w = st.width.saturating_sub(indent + 1).max(16);
    let mut out = String::new();
    for c in checks {
        let m = render_line(st, &mark(st, status_of(c.status)));
        let title = st.paint(Role::Package, &pad_right(c.title, tw));
        if side_by_side {
            let mut summary = wrap(&st.text(&c.summary), beside).into_iter();
            out.push_str(&format!(
                " {m}  {title}  {}\n",
                summary.next().unwrap_or_default()
            ));
            let pad = " ".repeat(indent + tw + 2);
            for more in summary {
                out.push_str(&format!("{pad}{more}\n"));
            }
        } else {
            out.push_str(&format!(" {m}  {}\n", st.paint(Role::Package, c.title)));
            for line in wrap(&st.text(&c.summary), under_w) {
                out.push_str(&format!("{}{line}\n", " ".repeat(indent + 1)));
            }
        }
        let detail_indent = " ".repeat(indent + 1);
        for d in &c.detail {
            for (i, l) in wrap(&st.text(d), st.width.saturating_sub(indent + 3).max(16))
                .into_iter()
                .enumerate()
            {
                let lead = if i == 0 { st.glyphs().bullet } else { " " };
                out.push_str(&format!(
                    "{detail_indent}{} {l}\n",
                    st.paint(Role::Dim, lead)
                ));
            }
        }
        if let Some(fix) = &c.fix {
            if c.status != DoctorStatus::Ok {
                let room = st.width.saturating_sub(indent + 1 + 10).max(16);
                for (i, l) in wrap(&st.text(fix), room).into_iter().enumerate() {
                    let lead = if i == 0 {
                        st.paint(Role::Ok, "suggests:")
                    } else {
                        " ".repeat(9)
                    };
                    out.push_str(&format!("{detail_indent}{lead} {l}\n"));
                }
            }
        }
    }
    out
}

// ----------------------------------------------------------------- history

fn summarize(record: &TransactionRecord) -> String {
    const SHOWN: usize = 3;
    let names: Vec<&str> = record
        .packages
        .iter()
        .take(SHOWN)
        .map(|p| p.name.as_str())
        .collect();
    let mut s = names.join(", ");
    if record.packages.len() > SHOWN {
        s.push_str(&format!(" +{} more", record.packages.len() - SHOWN));
    }
    s
}

pub fn history_view(
    st: &Style,
    records: &[TransactionRecord],
    rolled_back: &HashMap<String, String>,
    acknowledged: &HashSet<String>,
) -> String {
    let g = st.glyphs();
    let mut t = Table::new(vec![
        Column::new("id").keep(9),
        Column::new("time (UTC)").keep(7),
        Column::new("action").keep(8),
        Column::new("result").keep(9),
        Column::new("packages").flex().keep(6),
    ]);
    for r in records {
        let status: Line = match r.status {
            TxStatus::Committed => vec![Seg::new(format!("{} done", g.ok), Role::Ok)],
            TxStatus::Failed if acknowledged.contains(&r.id) => {
                vec![Seg::new(format!("{} failed (seen)", g.warn), Role::Warn)]
            }
            TxStatus::Failed => vec![Seg::new(format!("{} failed", g.problem), Role::Problem)],
        };
        let action = verb(r.action);
        let action_role = match r.action {
            PlanAction::Install => Role::Added,
            PlanAction::Remove | PlanAction::Purge => Role::Removed,
            PlanAction::Upgrade => Role::Version,
            PlanAction::Downgrade => Role::Warn,
            PlanAction::Reinstall => Role::Accent,
        };
        let mut notes: Vec<Line> = Vec::new();
        if !r.removed.is_empty() {
            let removed: Vec<String> = r
                .removed
                .iter()
                .map(|x| format!("{} ({})", x.name, x.reason))
                .collect();
            notes.push(vec![
                Seg::new("also removed: ", Role::Dim),
                Seg::plain(st.text(&removed.join(", "))),
            ]);
        }
        if let Some(original) = &r.rollback_of {
            notes.push(vec![Seg::new(format!("rollback of {original}"), Role::Dim)]);
        }
        if let Some(by) = rolled_back.get(&r.id) {
            notes.push(vec![Seg::new(format!("rolled back by {by}"), Role::Dim)]);
        }
        if let Some(error) = &r.error {
            let first = error.lines().next().unwrap_or("");
            notes.push(vec![
                Seg::new("error: ", Role::Problem),
                Seg::plain(st.text(first)),
            ]);
        }
        t.row_with_notes(
            vec![
                cell(r.id.clone(), Role::Dim),
                cell(r.timestamp_utc(), Role::Plain),
                cell(action, action_role),
                status,
                cell(summarize(r), Role::Package),
            ],
            notes,
        );
    }
    t.render(st, st.width)
}

pub fn snapshots_view(st: &Style, snapshots: &[Snapshot]) -> String {
    let mut t = Table::new(vec![
        Column::new("snapshot").flex().keep(9),
        Column::new("taken (UTC)").keep(7),
        Column::new("packages").right().keep(6),
        Column::new("note").flex().keep(2),
    ]);
    for s in snapshots {
        t.row(vec![
            cell(s.name.clone(), Role::Package),
            cell(s.timestamp_utc(), Role::Plain),
            cell(s.packages.len().to_string(), Role::Size),
            cell(s.note.clone().unwrap_or_default(), Role::Dim),
        ]);
    }
    t.render(st, st.width)
}

pub fn orphans_view(st: &Style, orphans: &[Orphan]) -> String {
    let max = orphans
        .iter()
        .map(|o| o.installed_size)
        .max()
        .unwrap_or(0)
        .max(1);
    let mut t = Table::new(vec![
        Column::new("package").flex().keep(9),
        Column::new("version").keep(6),
        Column::new("size").right().keep(8),
        Column::new("").keep(2),
    ]);
    for o in orphans {
        t.row(vec![
            cell(o.name.clone(), Role::Package),
            cell(o.version.clone(), Role::Version),
            cell(human_size(o.installed_size), Role::Size),
            bar(st, o.installed_size as f64 / max as f64, 10),
        ]);
    }
    t.render(st, st.width)
}

pub fn pins_view(st: &Style, pins: &[Pin], at: u64) -> String {
    let mut t = Table::new(vec![
        Column::new("package").flex().keep(9),
        Column::new("state").keep(8),
        Column::new("reason and expiry").flex().keep(3),
    ]);
    for p in pins {
        let state = if p.is_active(at) {
            vec![Seg::new(format!("{} held", st.glyphs().ok), Role::Ok)]
        } else {
            vec![Seg::new(
                format!("{} expired", st.glyphs().warn),
                Role::Warn,
            )]
        };
        t.row(vec![
            cell(p.name.clone(), Role::Package),
            state,
            cell(p.describe(), Role::Dim),
        ]);
    }
    t.render(st, st.width)
}

// -------------------------------------------------------------------- tree

/// The lines in front of each entry of a dependency tree, from the depth of
/// every entry in order (`├─`, `└─` and `│ ` columns).
pub fn tree_prefixes(st: &Style, depths: &[usize]) -> Vec<String> {
    let g = st.glyphs();
    let mut out = Vec::with_capacity(depths.len());
    for (i, &d) in depths.iter().enumerate() {
        // the entry is last among its siblings when no later entry of the same
        // depth comes before one that is shallower
        let is_last = |idx: usize, depth: usize| -> bool {
            for &later in &depths[idx + 1..] {
                if later == depth {
                    return false;
                }
                if later < depth {
                    return true;
                }
            }
            true
        };
        let mut prefix = String::from(" ");
        for level in 1..d {
            // does an ancestor at `level` have later siblings?
            let ancestor = (0..i).rev().find(|&k| depths[k] == level);
            let open = ancestor.is_some_and(|k| !is_last(k, level));
            prefix.push_str(if open { g.pipe } else { "  " });
        }
        prefix.push_str(if is_last(i, d) { g.last } else { g.tee });
        prefix.push(' ');
        out.push(prefix);
    }
    out
}

// ---------------------------------------------------------------- timeline

fn kind_glyph(st: &Style, kind: Kind) -> (String, Role) {
    let g = st.glyphs();
    match kind {
        Kind::Installed => ("+".into(), Role::Added),
        Kind::Removed => ("-".into(), Role::Removed),
        Kind::Upgraded => (g.up.into(), Role::Version),
        Kind::Downgraded => (g.down.into(), Role::Warn),
        Kind::Reinstalled => ("=".into(), Role::Accent),
        Kind::Failed | Kind::BuildFailed => (g.problem.into(), Role::Problem),
        Kind::RolledBack => ("<".into(), Role::Warn),
        Kind::Built => (g.ok.into(), Role::Ok),
        Kind::Snapshot => (g.note.into(), Role::Accent),
        Kind::Config => (g.note.into(), Role::Note),
    }
}

pub fn timeline_view(st: &Style, events: Vec<Event>, now: u64) -> String {
    let mut out = String::new();
    for (label, day) in timeline::group(events, now) {
        out.push_str(&heading(st, &label));
        out.push('\n');
        for e in day {
            let (glyph, role) = kind_glyph(st, e.kind);
            let room = st.width.saturating_sub(10);
            let text = emrac_core::style::truncate(st, &st.text(&e.text), room);
            out.push_str(&format!(
                "  {}  {}  {}\n",
                st.paint(Role::Dim, &timeline::clock(e.time)),
                st.paint(role, &glyph),
                text
            ));
        }
        out.push('\n');
    }
    out
}

// ------------------------------------------------------------------- stats

fn secs(s: u64) -> String {
    if s < 90 {
        format!("{s} s")
    } else if s < 5400 {
        format!("{} min", (s + 30) / 60)
    } else {
        format!("{:.1} h", s as f64 / 3600.0)
    }
}

pub fn builds_view(st: &Style, b: &BuildStats) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "  {} built, {} failed, {} different packages\n",
        st.paint(Role::Size, &b.total.to_string()),
        if b.failed > 0 {
            st.paint(Role::Problem, &b.failed.to_string())
        } else {
            b.failed.to_string()
        },
        b.packages
    ));
    if b.total > b.failed {
        out.push_str(&format!(
            "  time: {} in all, {} on average",
            st.paint(Role::Size, &secs(b.total_secs)),
            st.paint(Role::Size, &secs(b.average_secs))
        ));
        if !b.recent_secs.is_empty() {
            out.push_str(&format!(
                "   {} {}",
                render_line(st, &sparkline(st, &b.recent_secs)),
                st.paint(Role::Dim, &format!("last {} builds", b.recent_secs.len()))
            ));
        }
        out.push('\n');
    }
    if let Some((name, s)) = &b.longest {
        out.push_str(&format!(
            "  longest: {} {}\n",
            st.paint(Role::Package, name),
            secs(*s)
        ));
    }
    if !b.most_built.is_empty() {
        let mut t = Table::new(vec![
            Column::new("built most").flex().keep(9),
            Column::new("times").right().keep(8),
            Column::new("average").right().keep(6),
            Column::new("last").right().keep(5),
        ]);
        for p in &b.most_built {
            t.row(vec![
                cell(p.name.clone(), Role::Package),
                cell(p.builds.to_string(), Role::Size),
                cell(secs(p.average_secs), Role::Plain),
                cell(secs(p.last_secs), Role::Plain),
            ]);
        }
        out.push('\n');
        for l in t.render(st, st.width.saturating_sub(2)).lines() {
            out.push_str(&format!("  {l}\n"));
        }
    }
    out
}

// ----------------------------------------------------------------- mirrors

fn age(s: u64) -> String {
    if s < 5400 {
        format!("{} min", (s + 30) / 60)
    } else if s < 2 * 86_400 {
        format!("{:.1} h", s as f64 / 3600.0)
    } else {
        format!("{} d", s / 86_400)
    }
}

pub fn short_url(url: &str) -> String {
    let u = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    u[..u.find("/$repo").unwrap_or(u.len())].to_string()
}

fn group_role(g: Group) -> Role {
    match g {
        Group::Good => Role::Ok,
        Group::Stale => Role::Warn,
        Group::Unreliable => Role::Problem,
        Group::Untested => Role::Dim,
    }
}

pub fn mirrors_view(st: &Style, ranked: &[Ranked]) -> String {
    let fastest = ranked
        .iter()
        .filter_map(|r| r.stats.as_ref().and_then(|s| s.kib_per_s))
        .max()
        .unwrap_or(0);
    let mut t = Table::new(vec![
        Column::new("#").right().keep(9),
        Column::new("mirror").flex().keep(9),
        Column::new("group").keep(8),
        Column::new("last 10").keep(5),
        Column::new("median").right().keep(7),
        Column::new("speed").right().keep(6),
        Column::new("").keep(3),
        Column::new("behind Arch").right().keep(2),
    ]);
    for (i, r) in ranked.iter().enumerate() {
        let (ok, median, speed, sbar, sync) = match &r.stats {
            Some(s) if s.attempts > 0 => (
                format!("{}/{}", s.recent_successes, s.recent_attempts),
                s.median_total_ms.map_or("-".into(), |m| format!("{m} ms")),
                s.kib_per_s.map_or("-".into(), |k| format!("{k} KiB/s")),
                s.kib_per_s
                    .map_or(vec![], |k| bar(st, k as f64 / fastest.max(1) as f64, 8)),
                s.sync_age_s.map_or("?".into(), age),
            ),
            _ => ("-".into(), "-".into(), "-".into(), vec![], "-".into()),
        };
        t.row(vec![
            cell((i + 1).to_string(), Role::Dim),
            cell(short_url(&r.url), Role::Package),
            cell(r.group.word(), group_role(r.group)),
            cell(ok, Role::Plain),
            cell(median, Role::Plain),
            cell(speed, Role::Size),
            sbar,
            cell(sync, Role::Dim),
        ]);
    }
    t.render(st, st.width)
}

pub fn mirror_test_view(st: &Style, samples: &[Sample]) -> String {
    let g = st.glyphs();
    let mut t = Table::new(vec![
        Column::new("mirror").flex().keep(9),
        Column::new("result").keep(8),
        Column::new("headers").right().keep(5),
        Column::new("total").right().keep(7),
        Column::new("behind Arch").keep(4),
    ]);
    for s in samples {
        let (result, headers, total, sync) = if s.ok {
            (
                vec![Seg::new(format!("{} ok", g.ok), Role::Ok)],
                format!("{} ms", s.ttfb_ms),
                format!("{} ms", s.total_ms),
                s.sync_age_s.map_or("unknown".into(), age),
            )
        } else {
            (
                vec![Seg::new(format!("{} failed", g.problem), Role::Problem)],
                "-".into(),
                "-".into(),
                st.text(&s.error.clone().unwrap_or_default()),
            )
        };
        t.row(vec![
            cell(short_url(&s.url), Role::Package),
            result,
            cell(headers, Role::Plain),
            cell(total, Role::Plain),
            cell(sync, if s.ok { Role::Dim } else { Role::Problem }),
        ]);
    }
    t.render(st, st.width)
}

// --------------------------------------------------------------- benchmark

fn micros(us: u64) -> String {
    if us < 1_000 {
        format!("{us} \u{b5}s")
    } else if us < 1_000_000 {
        format!("{:.1} ms", us as f64 / 1_000.0)
    } else {
        format!("{:.2} s", us as f64 / 1_000_000.0)
    }
}

pub fn benchmark_view(st: &Style, records: &[BenchRecord]) -> String {
    let mut t = Table::new(vec![
        Column::new("program").flex().keep(9),
        Column::new("size").right().keep(7),
        Column::new("first run").right().keep(8),
        Column::new("warm").right().keep(6),
        Column::new("memory").right().keep(4),
        Column::new("note").keep(3),
    ]);
    for r in records {
        let m = &r.measurement;
        let mem = match m.real_rss_kib() {
            Some(k) if k >= 1024 => format!("{:.1} MiB", k as f64 / 1024.0),
            Some(k) => format!("{k} KiB"),
            None => "small".into(),
        };
        let note = if m.first_micros.is_none() {
            vec![Seg::new("did not finish", Role::Problem)]
        } else if m.nonzero_exit {
            vec![Seg::new("exit status not 0", Role::Warn)]
        } else {
            vec![]
        };
        t.row(vec![
            cell(m.exe.clone(), Role::Package),
            cell(human_size(m.size_bytes), Role::Size),
            cell(m.first_micros.map_or("-".into(), micros), Role::Plain),
            cell(m.warm_micros.map_or("-".into(), micros), Role::Plain),
            cell(mem, Role::Plain),
            note,
        ]);
    }
    t.render(st, st.width)
}

// ------------------------------------------------------------------ bundle

pub fn bundle_changes_view(st: &Style, changes: &[Change]) -> String {
    let mut t = Table::new(vec![
        Column::new("what").keep(9),
        Column::new("package").flex().keep(9),
        Column::new("version").keep(8),
        Column::new("note").flex().keep(2),
    ]);
    for c in changes {
        let (what, role) = match c.kind {
            ChangeKind::Install => ("install", Role::Added),
            ChangeKind::Upgrade => ("upgrade", Role::Version),
            ChangeKind::Same => ("same", Role::Dim),
            ChangeKind::Older => ("refused", Role::Problem),
        };
        let version = match (&c.from, c.kind) {
            (Some(from), ChangeKind::Upgrade) => version_change(st, from, &c.to),
            _ => vec![Seg::new(c.to.clone(), Role::Version)],
        };
        let note = match c.kind {
            ChangeKind::Same => "already installed, skipped".to_string(),
            ChangeKind::Older => st.text(&format!(
                "{} is installed, which is newer (never downgraded)",
                c.from.as_deref().unwrap_or("?")
            )),
            _ => String::new(),
        };
        t.row(vec![
            cell(what, role),
            cell(c.name.clone(), Role::Package),
            version,
            cell(note, Role::Dim),
        ]);
    }
    t.render(st, st.width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emrac_core::style::{display_width, strip_ansi, Depth, Theme};
    use emrac_core::{PlannedPackage, PlannedRemoval};

    fn colour() -> Style {
        Style {
            color: true,
            unicode: true,
            fancy: true,
            width: 80,
            theme: Theme::Default,
            depth: Depth::Color256,
        }
    }

    fn uncoloured() -> Style {
        Style {
            color: false,
            ..colour()
        }
    }

    fn ascii() -> Style {
        Style {
            color: false,
            unicode: false,
            ..colour()
        }
    }

    fn pkg(name: &str, version: &str, prev: Option<&str>, explicit: bool) -> PlannedPackage {
        PlannedPackage {
            name: name.into(),
            version: version.into(),
            repo: "extra".into(),
            explicit,
            previous_version: prev.map(str::to_string),
        }
    }

    fn plan(action: PlanAction, packages: Vec<PlannedPackage>, download: u64, delta: i64) -> Plan {
        Plan {
            action,
            packages,
            total_download_size: download,
            total_installed_size_delta: delta,
            removals: vec![],
            provider_choices: vec![],
            warnings: vec![],
        }
    }

    /// The same text with colour on and off: colour only decorates.
    fn same_without_colour(f: impl Fn(&Style) -> String) {
        assert_eq!(strip_ansi(&f(&colour())), f(&uncoloured()));
    }

    fn fits(text: &str, width: usize) {
        for l in text.lines() {
            assert!(display_width(l) <= width, "{l:?} is wider than {width}");
        }
    }

    #[test]
    fn a_plan_box_lines_up_and_marks_dependencies() {
        let p = plan(
            PlanAction::Install,
            vec![
                pkg("vim", "9.1.1-1", None, true),
                pkg("vim-runtime", "9.1.1-1", None, false),
                pkg("gpm", "1.20.7-5", None, false),
            ],
            9 << 20,
            41 << 20,
        );
        let out = plan_box(&uncoloured(), &p);
        assert!(
            out.starts_with("\u{256d}\u{2500} install \u{b7} 3 packages "),
            "{out}"
        );
        assert!(out.contains("extra  vim          9.1.1-1"), "{out}");
        assert!(
            out.contains("vim-runtime  9.1.1-1   dependency") || out.contains("dependency"),
            "{out}"
        );
        assert_eq!(out.matches("dependency").count(), 2);
        fits(&out, 80);
        let widths: Vec<usize> = out.lines().map(display_width).collect();
        assert!(widths.iter().all(|&w| w == widths[0]), "{widths:?}");
        same_without_colour(|st| plan_box(st, &p));
        assert!(plan_box(&ascii(), &p)
            .lines()
            .next()
            .unwrap()
            .starts_with("+- install - 3 packages"));
    }

    #[test]
    fn an_upgrade_shows_the_old_and_new_version_with_the_change_picked_out() {
        let p = plan(
            PlanAction::Upgrade,
            vec![pkg("mesa", "24.2.3-1", Some("24.2.1-1"), true)],
            0,
            0,
        );
        let out = plan_box(&uncoloured(), &p);
        assert!(out.contains("24.2.1-1 \u{2192} 24.2.3-1"), "{out}");
        let coloured = plan_box(&colour(), &p);
        assert!(
            coloured.contains("\x1b[1m3-1"),
            "the changed part is bold: {coloured:?}"
        );
    }

    #[test]
    fn the_summary_has_the_sizes_a_bar_and_the_plan_id() {
        let p = plan(
            PlanAction::Install,
            vec![pkg("a", "1", None, true)],
            10 << 20,
            40 << 20,
        );
        let out = plan_summary(&uncoloured(), &p, Some(100 << 30), "9d0ddb545688");
        assert!(out.contains("download  10.00 MiB"), "{out}");
        assert!(out.contains("disk      +40.00 MiB"), "{out}");
        assert!(out.contains("<0.1% of the free space on /"), "{out}");
        assert!(
            out.contains(
                "plan id   9d0ddb545688   (--expect-plan 9d0ddb545688 runs only this plan)"
            ),
            "{out}"
        );
        // half the free space: half the bar
        let big = plan(
            PlanAction::Install,
            vec![pkg("a", "1", None, true)],
            0,
            50 << 30,
        );
        let out = plan_summary(&uncoloured(), &big, Some(100 << 30), "x");
        assert!(out.contains("50.0% of the free space"), "{out}");
        assert!(
            out.contains("\u{2587}\u{2587}\u{2587}\u{2587}\u{2587}\u{2587}\u{2591}"),
            "{out}"
        );
        assert!(!out.contains("download"), "no download, no line");
        // a removal: no bar, a minus
        let rm = plan(
            PlanAction::Remove,
            vec![pkg("a", "1", None, true)],
            0,
            -(5 << 20),
        );
        let out = plan_summary(&uncoloured(), &rm, Some(100 << 30), "x");
        assert!(
            out.contains("-5.00 MiB") && !out.contains("free space"),
            "{out}"
        );
        // free space unknown: no bar
        assert!(!plan_summary(&uncoloured(), &p, None, "x").contains("free space"));
        same_without_colour(|st| plan_summary(st, &p, Some(100 << 30), "9d0ddb545688"));
    }

    fn check(
        id: &'static str,
        title: &'static str,
        status: DoctorStatus,
        summary: &str,
        detail: &[&str],
        fix: Option<&str>,
    ) -> DoctorCheck {
        DoctorCheck {
            id,
            title,
            status,
            summary: summary.into(),
            detail: detail.iter().map(|s| s.to_string()).collect(),
            fix: fix.map(str::to_string),
        }
    }

    #[test]
    fn checks_get_a_mark_a_title_column_and_their_details() {
        let checks = vec![
            check(
                "deps",
                "Dependencies",
                DoctorStatus::Ok,
                "every installed package's dependencies are satisfied",
                &[],
                Some("ignored: it is fine"),
            ),
            check(
                "units",
                "systemd units",
                DoctorStatus::Warning,
                "1 unit has failed",
                &["vboxdrv.service (system)"],
                Some("systemctl status <unit>"),
            ),
            check(
                "orph",
                "Orphans",
                DoctorStatus::Note,
                "20 dependencies that nothing needs",
                &[],
                None,
            ),
            check(
                "lock",
                "pacman",
                DoctorStatus::Problem,
                "pacman is running",
                &[],
                Some("wait for it to finish"),
            ),
        ];
        let out = checks_view(&uncoloured(), &checks);
        assert!(
            out.contains(" \u{2714} ok       Dependencies   every installed"),
            "{out}"
        );
        assert!(
            out.contains(" \u{26a0} warning  systemd units  1 unit has failed"),
            "{out}"
        );
        assert!(out.contains("\u{b7} vboxdrv.service (system)"), "{out}");
        assert!(out.contains("suggests: systemctl status <unit>"), "{out}");
        assert!(!out.contains("it is fine"), "no fix under an ok check");
        assert!(out.contains(" \u{2716} problem  pacman"), "{out}");
        assert!(out.contains(" \u{25cf} note     Orphans"), "{out}");
        fits(&out, 80);
        same_without_colour(|st| checks_view(st, &checks));
        let a = checks_view(&ascii(), &checks);
        assert!(
            a.contains(" + ok ") && a.contains(" x problem") && a.is_ascii(),
            "{a}"
        );
    }

    #[test]
    fn ascii_checks_are_ascii_even_when_the_text_has_dashes() {
        let checks = vec![check(
            "x",
            "Tools",
            DoctorStatus::Warning,
            "1 tool is missing \u{2014} needed",
            &["pacdiff \u{2014} for merging"],
            Some("sudo pacman -S x \u{2192} done"),
        )];
        let out = checks_view(&ascii(), &checks);
        assert!(out.is_ascii(), "{out}");
        assert!(
            out.contains("missing - needed") && out.contains("pacdiff - for merging"),
            "{out}"
        );
        assert!(
            checks_view(&uncoloured(), &checks).contains('\u{2014}'),
            "unicode keeps the dash"
        );
    }

    #[test]
    fn narrow_terminals_put_the_summary_under_the_title_and_wrap_the_fix() {
        let checks = vec![check(
            "sb",
            "Build sandbox",
            DoctorStatus::Warning,
            "AUR builds will be refused: bwrap can't create a sandbox here (bwrap: Can't mount proc on /proc: Operation not permitted)",
            &["a detail that is quite long indeed and goes on"],
            Some("sudo pacman -S bubblewrap, or build with --no-sandbox and accept the risks"),
        )];
        for w in [40, 50, 60, 80, 120] {
            let out = checks_view(
                &Style {
                    width: w,
                    ..ascii()
                },
                &checks,
            );
            fits(&out, w);
            assert!(out.contains("suggests:"), "{out}");
        }
        let narrow = checks_view(
            &Style {
                width: 50,
                ..ascii()
            },
            &checks,
        );
        assert!(
            narrow.lines().next().unwrap().ends_with("Build sandbox"),
            "{narrow}"
        );
        let wide = checks_view(
            &Style {
                width: 120,
                ..ascii()
            },
            &checks,
        );
        assert!(
            wide.lines()
                .next()
                .unwrap()
                .contains("Build sandbox  AUR builds"),
            "beside the title when there is room: {wide}"
        );
    }

    #[test]
    fn a_long_check_summary_wraps_under_itself_and_never_runs_past_the_width() {
        let long = "a very long summary ".repeat(12);
        let checks = vec![check(
            "x",
            "Title",
            DoctorStatus::Warning,
            &long,
            &[&long],
            None,
        )];
        let out = checks_view(
            &Style {
                width: 60,
                ..uncoloured()
            },
            &checks,
        );
        fits(&out, 60);
        assert!(out.lines().count() > 3);
    }

    fn record(
        id: &str,
        time: u64,
        action: PlanAction,
        status: TxStatus,
        names: &[&str],
    ) -> TransactionRecord {
        TransactionRecord {
            id: id.into(),
            timestamp: time,
            action,
            packages: names.iter().map(|n| pkg(n, "1", None, true)).collect(),
            removed: vec![],
            status,
            error: None,
            rollback_of: None,
            plan_digest: None,
        }
    }

    #[test]
    fn the_history_is_a_table_with_marks_and_notes_under_failures() {
        let mut failed = record(
            "tx-2",
            1_790_000_000,
            PlanAction::Upgrade,
            TxStatus::Failed,
            &["a", "b", "c", "d", "e"],
        );
        failed.error = Some("conflicting files\nmore".into());
        let mut removed = record(
            "tx-3",
            1_790_100_000,
            PlanAction::Install,
            TxStatus::Committed,
            &["vim"],
        );
        removed.removed = vec![PlannedRemoval {
            name: "gvim".into(),
            version: "1".into(),
            reason: "conflicts with vim".into(),
        }];
        let records = vec![
            record(
                "tx-1",
                1_789_000_000,
                PlanAction::Remove,
                TxStatus::Committed,
                &["sl"],
            ),
            failed,
            removed,
        ];
        let rolled = HashMap::from([("tx-1".to_string(), "tx-4".to_string())]);
        let seen = HashSet::new();
        let out = history_view(&uncoloured(), &records, &rolled, &seen);
        assert!(
            out.contains("\u{2714} done") && out.contains("\u{2716} failed"),
            "{out}"
        );
        assert!(out.contains("a, b, c +2 more"), "{out}");
        assert!(out.contains("    error: conflicting files"), "{out}");
        assert!(
            !out.contains("more\n    error: more"),
            "only the first line of the error"
        );
        assert!(
            out.contains("    also removed: gvim (conflicts with vim)"),
            "{out}"
        );
        assert!(out.contains("    rolled back by tx-4"), "{out}");
        fits(&out, 80);
        same_without_colour(|st| history_view(st, &records, &rolled, &seen));
        let acknowledged = HashSet::from(["tx-2".to_string()]);
        assert!(
            history_view(&uncoloured(), &records, &rolled, &acknowledged)
                .contains("\u{26a0} failed (seen)")
        );
    }

    #[test]
    fn snapshots_orphans_and_pins_are_tables() {
        let snaps = vec![Snapshot {
            name: "before".into(),
            timestamp: 1_790_000_000,
            note: Some("pre-upgrade".into()),
            packages: vec![],
        }];
        let out = snapshots_view(&uncoloured(), &snaps);
        assert!(
            out.contains("before") && out.contains("pre-upgrade") && out.contains("packages"),
            "{out}"
        );
        let orphans = vec![
            Orphan {
                name: "big".into(),
                version: "1-1".into(),
                installed_size: 100 << 20,
            },
            Orphan {
                name: "small".into(),
                version: "2-1".into(),
                installed_size: 1 << 20,
            },
        ];
        let out = orphans_view(&uncoloured(), &orphans);
        let big = out.lines().find(|l| l.starts_with("big")).unwrap();
        let small = out.lines().find(|l| l.starts_with("small")).unwrap();
        assert!(big.matches('\u{2587}').count() > small.matches('\u{2587}').count());
        let pins = vec![
            Pin {
                name: "linux".into(),
                reason: Some("dkms".into()),
                until: None,
                created: 1,
            },
            Pin {
                name: "mesa".into(),
                reason: None,
                until: Some(100),
                created: 1,
            },
        ];
        let out = pins_view(&uncoloured(), &pins, 1_000_000);
        assert!(
            out.contains("\u{2714} held")
                && out.contains("\u{26a0} expired")
                && out.contains("dkms"),
            "{out}"
        );
        for f in [
            snapshots_view(&colour(), &snaps),
            orphans_view(&colour(), &orphans),
            pins_view(&colour(), &pins, 1_000_000),
        ] {
            fits(&strip_ansi(&f), 80);
        }
        assert_eq!(
            strip_ansi(&snapshots_view(&colour(), &snaps)),
            snapshots_view(&uncoloured(), &snaps)
        );
    }

    #[test]
    fn the_timeline_groups_by_day_with_a_glyph_for_each_kind() {
        let day = 86_400u64;
        let events = vec![
            Event {
                time: 10 * day + 3600,
                kind: Kind::Upgraded,
                text: "Upgraded mesa 1 -> 2".into(),
                source: "history",
                packages: vec![],
            },
            Event {
                time: 10 * day + 60,
                kind: Kind::Built,
                text: "Built nvim 0.11 in 4 min".into(),
                source: "build times",
                packages: vec![],
            },
            Event {
                time: 9 * day + 5,
                kind: Kind::Failed,
                text: "Failed to install foo".into(),
                source: "history",
                packages: vec![],
            },
        ];
        let out = timeline_view(&uncoloured(), events.clone(), 10 * day + 7200);
        assert!(out.starts_with("Today "), "{out}");
        assert!(out.contains("Yesterday "), "{out}");
        assert!(
            out.contains("01:00  \u{2191}  Upgraded mesa 1 -> 2"),
            "{out}"
        );
        assert!(out.contains("00:01  \u{2714}  Built nvim"), "{out}");
        assert!(
            out.contains("00:00  \u{2716}  Failed to install foo"),
            "{out}"
        );
        fits(&out, 80);
        same_without_colour(|st| timeline_view(st, events.clone(), 10 * day + 7200));
        let a = timeline_view(&ascii(), events, 10 * day + 7200);
        assert!(a.contains("^  Upgraded") && a.is_ascii());
    }

    #[test]
    fn the_build_view_shows_a_sparkline_and_a_table() {
        let b = BuildStats {
            total: 5,
            failed: 1,
            packages: 2,
            total_secs: 600,
            average_secs: 150,
            longest: Some(("neovim".into(), 300)),
            most_built: vec![emrac_core::build_stats::PackageBuilds {
                name: "neovim".into(),
                builds: 3,
                average_secs: 200,
                last_secs: 300,
            }],
            recent_secs: vec![100, 300, 200, 50],
        };
        let out = builds_view(&uncoloured(), &b);
        assert!(
            out.contains("5 built, 1 failed, 2 different packages"),
            "{out}"
        );
        assert!(
            out.contains("10 min in all, 150 s on average") || out.contains("in all"),
            "{out}"
        );
        assert!(
            out.contains("last 4 builds") && out.contains('\u{2588}'),
            "{out}"
        );
        assert!(out.contains("longest: neovim 5 min"), "{out}");
        assert!(
            out.contains("built most") && out.contains("neovim"),
            "{out}"
        );
        fits(&out, 80);
        same_without_colour(|st| builds_view(st, &b));
    }

    #[test]
    fn the_mirror_views_show_groups_and_a_speed_bar() {
        use emrac_core::mirrors::Stats;
        let fast = Stats {
            attempts: 10,
            successes: 10,
            recent_attempts: 10,
            recent_successes: 10,
            last_ok: true,
            median_total_ms: Some(80),
            median_ttfb_ms: Some(20),
            kib_per_s: Some(1600),
            sync_age_s: Some(1500),
            last_error: None,
        };
        let slow = Stats {
            kib_per_s: Some(200),
            median_total_ms: Some(600),
            ..fast.clone()
        };
        let ranked = vec![
            Ranked {
                url: "https://fast.example/archlinux/$repo/os/$arch".into(),
                group: Group::Good,
                stats: Some(fast),
            },
            Ranked {
                url: "https://slow.example/$repo/os/$arch".into(),
                group: Group::Good,
                stats: Some(slow),
            },
            Ranked {
                url: "https://new.example/$repo/os/$arch".into(),
                group: Group::Untested,
                stats: None,
            },
        ];
        let out = mirrors_view(
            &Style {
                width: 120,
                ..uncoloured()
            },
            &ranked,
        );
        assert!(
            out.contains("fast.example/archlinux")
                && out.contains("1600 KiB/s")
                && out.contains("25 min"),
            "{out}"
        );
        let f = out.lines().find(|l| l.contains("fast.example")).unwrap();
        let s = out.lines().find(|l| l.contains("slow.example")).unwrap();
        assert!(
            f.matches('\u{2587}').count() > s.matches('\u{2587}').count(),
            "{out}"
        );
        assert!(out.contains("untested"));
        fits(&out, 120);
        same_without_colour(|st| mirrors_view(st, &ranked));
        let narrow = mirrors_view(&uncoloured(), &ranked);
        fits(&narrow, 80);
        assert!(narrow.contains('\u{2026}'), "the long name is cut to fit");
        let samples = vec![
            Sample {
                time: 1,
                url: "https://a.example/$repo".into(),
                ok: true,
                ttfb_ms: 50,
                total_ms: 90,
                bytes: 1000,
                sync_age_s: Some(120),
                error: None,
            },
            Sample {
                time: 1,
                url: "https://b.example/$repo".into(),
                ok: false,
                ttfb_ms: 0,
                total_ms: 0,
                bytes: 0,
                sync_age_s: None,
                error: Some("timed out".into()),
            },
        ];
        let out = mirror_test_view(&uncoloured(), &samples);
        assert!(
            out.contains("\u{2714} ok")
                && out.contains("\u{2716} failed")
                && out.contains("timed out")
                && out.contains("2 min"),
            "{out}"
        );
        same_without_colour(|st| mirror_test_view(st, &samples));
    }

    #[test]
    fn the_benchmark_and_bundle_views() {
        use emrac_core::benchmark::Measurement;
        let rec = |first: Option<u64>, nonzero: bool| BenchRecord {
            time: 1,
            package: "p".into(),
            version: "1".into(),
            installed_size: 1,
            cores: 4,
            measurement: Measurement {
                exe: "/usr/bin/p".into(),
                size_bytes: 150 << 10,
                first_micros: first,
                warm_micros: first.map(|f| f / 2),
                runs: 5,
                max_rss_kib: 40_000,
                floor_rss_kib: 28_000,
                nonzero_exit: nonzero,
                timeouts: 0,
            },
        };
        let out = benchmark_view(
            &uncoloured(),
            &[
                rec(Some(4300), false),
                rec(None, false),
                rec(Some(1500), true),
            ],
        );
        assert!(
            out.contains("4.3 ms") && out.contains("2.1 ms") && out.contains("39.1 MiB"),
            "{out}"
        );
        assert!(
            out.contains("did not finish") && out.contains("exit status not 0"),
            "{out}"
        );
        let changes = vec![
            Change {
                name: "a".into(),
                from: None,
                to: "1-1".into(),
                kind: ChangeKind::Install,
            },
            Change {
                name: "b".into(),
                from: Some("1-1".into()),
                to: "1-2".into(),
                kind: ChangeKind::Upgrade,
            },
            Change {
                name: "c".into(),
                from: Some("2-1".into()),
                to: "1-1".into(),
                kind: ChangeKind::Older,
            },
            Change {
                name: "d".into(),
                from: Some("1-1".into()),
                to: "1-1".into(),
                kind: ChangeKind::Same,
            },
        ];
        let out = bundle_changes_view(&uncoloured(), &changes);
        assert!(
            out.contains("install")
                && out.contains("1-1 \u{2192} 1-2")
                && out.contains("never downgraded")
                && out.contains("already installed"),
            "{out}"
        );
        fits(&out, 80);
        same_without_colour(|st| bundle_changes_view(st, &changes));
        same_without_colour(|st| benchmark_view(st, &[rec(Some(4300), false)]));
    }

    #[test]
    fn a_tree_has_branches_that_close_where_the_siblings_end() {
        // a
        //   b
        //     c
        //     d
        //   e
        // f
        let depths = [1, 2, 3, 3, 2, 1];
        let p = tree_prefixes(&uncoloured(), &depths);
        let lines: Vec<String> = p
            .iter()
            .zip(["a", "b", "c", "d", "e", "f"])
            .map(|(p, n)| format!("{p}{n}"))
            .collect();
        assert_eq!(
            lines,
            [
                " \u{251c}\u{2500} a",
                " \u{2502} \u{251c}\u{2500} b",
                " \u{2502} \u{2502} \u{251c}\u{2500} c",
                " \u{2502} \u{2502} \u{2514}\u{2500} d",
                " \u{2502} \u{2514}\u{2500} e",
                " \u{2514}\u{2500} f",
            ]
        );
        let a = tree_prefixes(&ascii(), &[1, 2, 1]);
        assert_eq!(a, [" |- ", " | `- ", " `- "]);
        assert!(tree_prefixes(&uncoloured(), &[]).is_empty());
    }

    #[test]
    fn every_view_fits_a_narrow_terminal() {
        let st = Style {
            width: 40,
            ..uncoloured()
        };
        let records = vec![record(
            "tx-1",
            1_790_000_000,
            PlanAction::Install,
            TxStatus::Committed,
            &["a-very-long-package-name-indeed", "b"],
        )];
        fits(
            &history_view(&st, &records, &HashMap::new(), &HashSet::new()),
            40,
        );
        let p = plan(
            PlanAction::Install,
            vec![pkg(&"n".repeat(60), "1", None, false)],
            0,
            1,
        );
        fits(&plan_box(&st, &p), 40);
    }
}
