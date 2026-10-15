use emrac_core::{say, Voice};
use std::collections::HashMap;
use std::io::Write;

use emrac_core::{
    AuditReport, ConfigState, DepEntry, DepsReport, DoctorReport, DoctorStatus, DowngradePlan,
    Download, IssueKind, Manifest, Orphan, PackageDetails, PackageList, PackageSummary, Pin, Plan,
    PlanAction, PurgePlan, RdepsReport, ReinstallPlan, ReinstallSource, RestorePlan, RestoreSource,
    RollbackPlan, Severity, Snapshot, TransactionRecord, TxStatus, Verdict, VerifyReport,
    VersionList, WhyReport,
};

pub fn print_search_results(query: &str, results: &[PackageSummary], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(results).unwrap());
        return;
    }

    if results.is_empty() {
        say!(
            Voice::Found,
            "nothing matching '{query}'. Try a shorter or different term."
        );
        return;
    }

    say!(
        Voice::Found,
        "{} matching '{query}'\n",
        plural(results.len(), "package")
    );

    for pkg in results {
        println!("{}/{} {}", pkg.repo, pkg.name, pkg.version);
        if let Some(desc) = pkg.description.as_deref().filter(|d| !d.is_empty()) {
            println!("    {desc}");
        }
    }
}

pub fn print_package_details(pkg: &PackageDetails, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(pkg).unwrap());
        return;
    }

    say!(Voice::Found, "{} in {}\n", pkg.name, pkg.repo);

    println!("Name            : {}", pkg.name);
    println!("Version         : {}", pkg.version);
    println!("Repository      : {}", pkg.repo);
    println!(
        "Description     : {}",
        pkg.description.as_deref().unwrap_or("-")
    );
    println!("URL             : {}", pkg.url.as_deref().unwrap_or("-"));
    println!("Licenses        : {}", join_or_dash(&pkg.license));
    println!("Depends On      : {}", join_or_dash(&pkg.depends));
    println!("Provides        : {}", join_or_dash(&pkg.provides));
    println!(
        "Installed Size  : {}",
        pkg.installed_size.map(human_size).as_deref().unwrap_or("-")
    );

    if let Some(maintainer) = &pkg.maintainer {
        println!("Maintainer      : {maintainer}");
    }
    if let Some(votes) = pkg.votes {
        println!("Votes           : {votes}");
    }
    if let Some(popularity) = pkg.popularity {
        println!("Popularity      : {popularity:.2}");
    }
    if pkg.out_of_date.is_some() {
        println!("Out of Date     : yes");
    }
}

pub fn print_plan(plan: &Plan, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(plan).unwrap());
        return;
    }

    if plan.is_empty() {
        let note = match plan.action {
            PlanAction::Install => "nothing to install — you're already up to date.",
            PlanAction::Remove => "nothing to remove.",
            PlanAction::Upgrade => "nothing to upgrade — you're already up to date.",
            PlanAction::Downgrade => "nothing to downgrade.",
            PlanAction::Reinstall => "nothing to reinstall.",
            PlanAction::Purge => "nothing to purge.",
        };
        say!(Voice::Notes, "{note}");
        return;
    }

    let verb = match plan.action {
        PlanAction::Install => "install",
        PlanAction::Remove => "remove",
        PlanAction::Upgrade => "upgrade",
        PlanAction::Downgrade => "downgrade",
        PlanAction::Reinstall => "reinstall",
        PlanAction::Purge => "purge",
    };

    say!(
        Voice::Plans,
        "{verb} {}\n",
        plural(plan.packages.len(), "package")
    );
    for pkg in &plan.packages {
        let suffix = if pkg.explicit { "" } else { " (dependency)" };
        println!("  {}/{} {}{suffix}", pkg.repo, pkg.name, pkg.version);
    }
    println!();

    // Removals nobody asked for: the packages pacman removes to resolve
    // conflicts, or because another package replaces them.
    if !plan.removals.is_empty() {
        println!(
            "Will also REMOVE {} (pacman asks first; with --yes a conflict is refused and a replacement accepted):",
            plural(plan.removals.len(), "installed package"),
        );
        for r in &plan.removals {
            println!("  local/{} {}  — {}", r.name, r.version, r.reason);
        }
        println!();
    }
    // Dependencies several packages could satisfy: pacman asks which one (and
    // takes the first with --yes).
    if !plan.provider_choices.is_empty() {
        println!("Provider choices (pacman asks; with --yes it takes the first):");
        for choice in &plan.provider_choices {
            let first = choice.providers.first().map(String::as_str).unwrap_or("?");
            println!(
                "  {}: {}  (default: {first})",
                choice.dependency,
                choice.providers.join(", ")
            );
        }
        println!();
    }
    for warning in &plan.warnings {
        say!(Voice::Warns, "{warning}");
    }

    if plan.total_download_size > 0 {
        println!(
            "Download Size    : {}",
            human_size(plan.total_download_size)
        );
    }

    let (label, size) = if plan.total_installed_size_delta >= 0 {
        ("Installed Size (+)", plan.total_installed_size_delta as u64)
    } else {
        (
            "Installed Size (-)",
            plan.total_installed_size_delta.unsigned_abs(),
        )
    };
    println!("{label} : {}", human_size(size));
}

pub fn print_pkgbuild(pkg: &str, content: &str) {
    say!(Voice::Notes, "PKGBUILD for '{pkg}' (first build):\n");
    println!("{content}");
}

pub fn print_pkgbuild_diff(pkg: &str, diff: &str) {
    say!(
        Voice::Remembers,
        "PKGBUILD changes for '{pkg}' since your last build:\n"
    );
    println!("{diff}");
}

pub fn print_verify(report: &VerifyReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(report).unwrap());
        return;
    }

    let files: usize = report.packages.iter().map(|p| p.files_checked).sum();
    say!(
        Voice::Found,
        "checked {} ({} files)\n",
        plural(report.packages.len(), "package"),
        files
    );

    let mut unreadable = 0;
    for pkg in &report.packages {
        unreadable += pkg.unreadable_count();
        let problems = pkg.problem_count();
        if problems == 0 {
            println!(
                "{} {}: ok ({} files)",
                pkg.package, pkg.version, pkg.files_checked
            );
        } else {
            println!(
                "{} {}: {} ({} files)",
                pkg.package,
                pkg.version,
                plural(problems, "problem"),
                pkg.files_checked
            );
        }
        for issue in pkg.problems() {
            let what = match issue.kind {
                IssueKind::Missing => "missing",
                IssueKind::WrongType => "wrong type",
                IssueKind::SizeMismatch => "size differs",
                IssueKind::ContentMismatch => "contents differ",
                IssueKind::ModeMismatch => "permissions differ",
                IssueKind::LinkMismatch => "symlink differs",
                IssueKind::Unreadable => continue,
            };
            match &issue.detail {
                Some(detail) => println!("    {what:<18} {} ({detail})", issue.path),
                None => println!("    {what:<18} {}", issue.path),
            }
        }
        for path in &pkg.modified_config {
            println!("    config edited      {path}");
        }
    }

    if unreadable > 0 {
        println!(
            "\nemrac notes: {} couldn't be read, so they weren't checked — run as root for a complete check.",
            plural(unreadable, "file")
        );
    }
}

pub fn print_purge(purge: &PurgePlan, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(purge).unwrap());
        return;
    }

    print_plan(&purge.plan, false);

    let at_risk = purge.at_risk();
    if at_risk.is_empty() {
        return;
    }
    println!(
        "\nThese config files have local changes, or couldn't be checked. `remove` would keep them as .pacsave; purge deletes them for good:"
    );
    for file in at_risk {
        let note = match file.state {
            ConfigState::Unknown => " (couldn't be read — may be modified)",
            _ => "",
        };
        println!("  {} ({}){note}", file.path, file.package);
    }
}

pub fn print_reinstall(reinstall: &ReinstallPlan, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(reinstall).unwrap());
        return;
    }

    say!(
        Voice::Plans,
        "reinstall {}\n",
        plural(reinstall.items.len(), "package")
    );
    for (item, pkg) in reinstall.items.iter().zip(&reinstall.plan.packages) {
        let from = match item.source {
            ReinstallSource::File => "from a package file on this machine",
            ReinstallSource::Repository => "from the repositories",
            ReinstallSource::Archive => "from the Arch Linux Archive",
            ReinstallSource::Rebuild => "rebuilt from your existing AUR checkout",
        };
        println!("  {}/{} {} ({from})", pkg.repo, pkg.name, pkg.version);
    }
    print_downloads(&reinstall.downloads);
}

pub fn print_downgrade(downgrade: &DowngradePlan, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(downgrade).unwrap());
        return;
    }

    let pkg = &downgrade.plan.packages[0];
    say!(Voice::Plans, "downgrade 1 package\n");
    match &pkg.previous_version {
        Some(from) => println!("  {}/{} {} (from {from})", pkg.repo, pkg.name, pkg.version),
        None => println!("  {}/{} {}", pkg.repo, pkg.name, pkg.version),
    }
    print_downloads(&downgrade.downloads);
}

pub fn print_versions(list: &VersionList, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(list).unwrap());
        return;
    }

    if list.versions.is_empty() {
        say!(
            Voice::Notes,
            "no other versions of '{}' found on this machine or in the Arch Linux Archive.",
            list.package
        );
        return;
    }

    match &list.installed {
        Some(v) => say!(
            Voice::Found,
            "{} of '{}' (installed: {v})\n",
            plural(list.versions.len(), "version"),
            list.package
        ),
        None => say!(
            Voice::Found,
            "{} of '{}' (not installed)\n",
            plural(list.versions.len(), "version"),
            list.package
        ),
    }
    for entry in &list.versions {
        let marker = if entry.installed { " (installed)" } else { "" };
        println!("  {:<24} {}{marker}", entry.version, entry.source);
    }
}

fn print_downloads(downloads: &[Download]) {
    if downloads.is_empty() {
        return;
    }
    println!("\nWill download from the Arch Linux Archive:");
    for download in downloads {
        println!("  {} {}", download.name, download.version);
    }
}

pub fn print_rollback(rollback: &RollbackPlan, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(rollback).unwrap());
        return;
    }

    let plan = &rollback.plan;
    if plan.is_empty() {
        say!(
            Voice::Notes,
            "nothing left to roll back for {} — none of its packages are as it left them.",
            rollback.target
        );
    } else {
        let verb = match plan.action {
            PlanAction::Remove => "remove",
            PlanAction::Install => "reinstall",
            PlanAction::Downgrade => "downgrade",
            PlanAction::Upgrade => "upgrade",
            PlanAction::Reinstall => "reinstall",
            PlanAction::Purge => "purge",
        };
        let what = match &rollback.reapplies {
            Some(original) => format!("redo {original} (by reversing {})", rollback.target),
            None => format!("roll back {}", rollback.target),
        };
        say!(
            Voice::Plans,
            "{what} — {verb} {}\n",
            plural(plan.packages.len(), "package")
        );
        for pkg in &plan.packages {
            match (&plan.action, &pkg.previous_version) {
                (PlanAction::Downgrade, Some(from)) => {
                    println!("  {}/{} {} (from {from})", pkg.repo, pkg.name, pkg.version)
                }
                _ => println!("  {}/{} {}", pkg.repo, pkg.name, pkg.version),
            }
        }
    }

    print_downloads(&rollback.downloads);

    if let Some(note) = &rollback.note {
        println!("\nemrac notes: {note}");
    }

    if !rollback.skipped.is_empty() {
        println!("\nLeft alone (no longer as {} left them):", rollback.target);
        for skipped in &rollback.skipped {
            println!("  {} — {}", skipped.name, skipped.reason);
        }
    }
}

pub fn print_history(
    records: &[TransactionRecord],
    rolled_back: &HashMap<String, String>,
    acknowledged: &std::collections::HashSet<String>,
    archives: &[emrac_core::ArchiveInfo],
    json: bool,
) {
    if json {
        println!("{}", serde_json::to_string_pretty(records).unwrap());
        return;
    }

    if records.is_empty() {
        say!(Voice::Notes, "no transactions found.");
        return;
    }

    say!(
        Voice::Found,
        "{} (times are UTC)\n",
        plural(records.len(), "transaction")
    );
    if !archives.is_empty() {
        let n: usize = archives.iter().map(|a| a.records).sum();
        println!(
            "{} archived ({} to {}); `emrac history --archive` lists them.\n",
            plural(n, "older transaction"),
            archives[0].first_id,
            archives[archives.len() - 1].last_id
        );
    }

    for record in records {
        let action = match record.action {
            PlanAction::Install => "install",
            PlanAction::Remove => "remove",
            PlanAction::Upgrade => "upgrade",
            PlanAction::Downgrade => "downgrade",
            PlanAction::Reinstall => "reinstall",
            PlanAction::Purge => "purge",
        };
        let status = match record.status {
            TxStatus::Committed => "committed",
            TxStatus::Failed if acknowledged.contains(&record.id) => "FAILED (acknowledged)",
            TxStatus::Failed => "FAILED",
        };
        println!(
            "{:<7} {}  {:<9} {:<9} {}",
            record.id,
            record.timestamp_utc(),
            action,
            status,
            summarize_packages(record)
        );
        if !record.removed.is_empty() {
            let removed: Vec<String> = record
                .removed
                .iter()
                .map(|r| format!("{} ({})", r.name, r.reason))
                .collect();
            println!("        also removed: {}", removed.join(", "));
        }
        if let Some(original) = &record.rollback_of {
            println!("        rollback of {original}");
        }
        if let Some(by) = rolled_back.get(&record.id) {
            println!("        rolled back by {by}");
        }
        if let Some(error) = &record.error {
            println!("        error: {error}");
        }
    }
}

/// `a, b, c` for short transactions, `a, b, c +4 more` for long ones.
fn summarize_packages(record: &TransactionRecord) -> String {
    const SHOWN: usize = 3;
    let names: Vec<&str> = record
        .packages
        .iter()
        .take(SHOWN)
        .map(|p| p.name.as_str())
        .collect();
    let mut summary = names.join(", ");
    if record.packages.len() > SHOWN {
        summary.push_str(&format!(" +{} more", record.packages.len() - SHOWN));
    }
    summary
}

fn plural(n: usize, singular: &str) -> String {
    if n == 1 {
        format!("{n} {singular}")
    } else {
        format!("{n} {singular}s")
    }
}

fn join_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join("  ")
    }
}

pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.2} {}", UNITS[unit])
}

pub fn print_snapshot_created(snapshot: &Snapshot, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(snapshot).unwrap());
        return;
    }
    say!(
        Voice::Confirms,
        "saved snapshot '{}' ({}).",
        snapshot.name,
        plural(snapshot.packages.len(), "package")
    );
}

pub fn print_snapshots(snapshots: &[Snapshot], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(snapshots).unwrap());
        return;
    }
    if snapshots.is_empty() {
        say!(
            Voice::Notes,
            "no snapshots yet — `emrac snapshot` saves one."
        );
        return;
    }
    say!(
        Voice::Found,
        "{} (times are UTC)\n",
        plural(snapshots.len(), "snapshot")
    );
    for s in snapshots {
        let note = s
            .note
            .as_deref()
            .map(|n| format!("  {n}"))
            .unwrap_or_default();
        println!(
            "  {}  {}  {}{note}",
            s.name,
            s.timestamp_utc(),
            plural(s.packages.len(), "package")
        );
    }
}

pub fn print_snapshot(snapshot: &Snapshot, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(snapshot).unwrap());
        return;
    }
    say!(
        Voice::Found,
        "snapshot '{}' from {} (UTC), {}",
        snapshot.name,
        snapshot.timestamp_utc(),
        plural(snapshot.packages.len(), "package")
    );
    if let Some(note) = &snapshot.note {
        println!("  note: {note}");
    }
    println!();
    for p in &snapshot.packages {
        let tags = match (p.explicit, p.foreign) {
            (true, true) => "  [explicit, foreign]",
            (true, false) => "  [explicit]",
            (false, true) => "  [dependency, foreign]",
            (false, false) => "",
        };
        println!("  {} {}{tags}", p.name, p.version);
    }
}

pub fn print_restore(restore: &RestorePlan, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(restore).unwrap());
        return;
    }

    if restore.entries.is_empty() && restore.extras.is_empty() {
        say!(
            Voice::Notes,
            "the installed packages already match '{}'.",
            restore.snapshot
        );
        return;
    }

    if !restore.entries.is_empty() {
        say!(
            Voice::Plans,
            "restore {} to match '{}'\n",
            plural(restore.entries.len(), "package"),
            restore.snapshot
        );
        for entry in &restore.entries {
            let item = &entry.item;
            let what = match item.action {
                PlanAction::Install => "install  ",
                PlanAction::Upgrade => "upgrade  ",
                _ => "downgrade",
            };
            let from = match &item.from {
                Some(from) => format!(" (from {from})"),
                None => String::new(),
            };
            let source = match entry.source {
                RestoreSource::File => "package file on this machine",
                RestoreSource::Archive => "Arch Linux Archive",
                RestoreSource::Repository => "repositories",
            };
            println!("  {what} {} {}{from}  [{source}]", item.name, item.to);
        }
        print_downloads(&restore.downloads);
        println!(
            "\nNote: restored packages are marked as explicitly installed; their original install reason isn't restored."
        );
    }

    if !restore.extras.is_empty() {
        let verb = if restore.prune {
            "Will remove"
        } else {
            "Not in the snapshot, left alone (--prune removes them)"
        };
        println!("\n{verb}:");
        for (name, version) in &restore.extras {
            println!("  {name} {version}");
        }
    }
}

fn print_dep_entries(entries: &[DepEntry]) {
    for dep in entries {
        let state = match &dep.satisfied_by {
            Some((name, version)) if dep.spec.split(['<', '>', '=']).next() == Some(name) => {
                format!("installed {version}")
            }
            Some((name, version)) => format!("provided by {name} {version}"),
            None => "not installed".to_string(),
        };
        match dep.description.as_deref().filter(|d| !d.is_empty()) {
            Some(desc) => println!("  {}  [{state}]  {desc}", dep.spec),
            None => println!("  {}  [{state}]", dep.spec),
        }
    }
}

pub fn print_deps(report: &DepsReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(report).unwrap());
        return;
    }

    let source = if report.installed {
        ""
    } else {
        " (not installed; from the repositories)"
    };
    say!(Voice::Found, "{} {}{source}\n", report.name, report.version);
    if report.depends.is_empty() {
        println!("Requires: nothing");
    } else {
        println!("Requires:");
        print_dep_entries(&report.depends);
    }
    if !report.optional.is_empty() {
        println!("\nOptional:");
        print_dep_entries(&report.optional);
    }
    if let Some(all) = &report.recursive {
        println!("\nAll required dependencies, transitively ({}):", all.len());
        for (name, depth) in all {
            println!("  {name}  (level {depth})");
        }
    }
}

pub fn print_rdeps(report: &RdepsReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(report).unwrap());
        return;
    }

    if report.required_by.is_empty() && report.optional_for.is_empty() {
        say!(
            Voice::Notes,
            "no installed package needs '{}'.",
            report.name
        );
        return;
    }
    say!(Voice::Found, "what needs {}\n", report.name);
    if !report.required_by.is_empty() {
        println!("Required by: {}", report.required_by.join("  "));
    }
    if !report.optional_for.is_empty() {
        println!("Optional for: {}", report.optional_for.join("  "));
    }
}

pub fn print_why(report: &WhyReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(report).unwrap());
        return;
    }

    match report {
        WhyReport::Explicit { name, required_by } => {
            say!(
                Voice::Found,
                "{name} is installed because you asked for it."
            );
            if !required_by.is_empty() {
                const SHOWN: usize = 12;
                let mut list = required_by.iter().take(SHOWN).cloned().collect::<Vec<_>>();
                if required_by.len() > SHOWN {
                    list.push(format!("and {} more", required_by.len() - SHOWN));
                }
                println!("It is also required by: {}", list.join("  "));
            }
        }
        WhyReport::Dependency { name, chain } => {
            say!(Voice::Found, "{name} is installed as a dependency.\n");
            println!("  {}", chain.join(" → "));
        }
        WhyReport::Orphan { name } => {
            say!(
                Voice::Found,
                "{name} was installed as a dependency, but no installed package needs it any more."
            );
            println!("`emrac remove {name}` would clear it.");
        }
    }
}

pub fn print_tree(report: &emrac_core::TreeReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(report).unwrap());
        return;
    }
    println!("{} {}", report.name, report.version);
    if report.lines.is_empty() {
        println!("  (no dependencies)");
        return;
    }
    for l in &report.lines {
        let mut tags = Vec::new();
        if l.optional {
            tags.push("optional");
        }
        if l.repeat {
            tags.push("shown above");
        }
        if l.cut {
            tags.push("more below");
        }
        let tag = if tags.is_empty() {
            String::new()
        } else {
            format!("  ({})", tags.join(", "))
        };
        println!("{}{}{tag}", "  ".repeat(l.depth), l.name);
    }
}

pub fn print_graph(report: &emrac_core::GraphReport) {
    println!(
        "{} ({} packages, {} links)",
        report.root,
        report.nodes.len(),
        report.edges.len()
    );
    let mut from: Vec<&str> = report.edges.iter().map(|e| e.from.as_str()).collect();
    from.dedup();
    let mut seen = std::collections::HashSet::new();
    for name in std::iter::once(report.root.as_str()).chain(from) {
        if !seen.insert(name) {
            continue;
        }
        let kids: Vec<String> = report
            .edges
            .iter()
            .filter(|e| e.from == name)
            .map(|e| {
                if e.optional {
                    format!("{} (optional)", e.to)
                } else {
                    e.to.clone()
                }
            })
            .collect();
        if !kids.is_empty() {
            println!("  {name} -> {}", kids.join(", "));
        }
    }
}

pub fn print_cycles(groups: &[Vec<String>], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(groups).unwrap());
        return;
    }
    if groups.is_empty() {
        say!(
            Voice::Notes,
            "no dependency cycles among the installed packages."
        );
        return;
    }
    say!(
        Voice::Found,
        "{} group(s) of packages that require each other:\n",
        groups.len()
    );
    for g in groups {
        println!("  {}", g.join(" ⇄ "));
    }
    println!("\nA cycle is not a fault: pacman installs such a group together.");
}

pub fn print_impact_remove(r: &emrac_core::RemoveImpactReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(r).unwrap());
        return;
    }
    say!(
        Voice::Found,
        "removing {} {} ({}){}\n",
        r.name,
        r.version,
        human_size(r.installed_size),
        if r.explicit {
            ", which you installed"
        } else {
            ", installed as a dependency"
        }
    );
    if r.breaks.is_empty() {
        println!("Breaks nothing: no installed package needs it.");
    } else {
        println!("Needed by (pacman refuses to remove it unless these go too, `--cascade`):");
        const SHOWN: usize = 30;
        for i in r.breaks.iter().take(SHOWN) {
            println!(
                "  {} {}  ({})",
                i.name,
                i.version,
                human_size(i.installed_size)
            );
        }
        if r.breaks.len() > SHOWN {
            println!(
                "  and {} more (`--json` lists all {})",
                r.breaks.len() - SHOWN,
                r.breaks.len()
            );
        }
    }
    if !r.optional_for.is_empty() {
        println!(
            "\nOptional for (they keep working, but lose a feature): {}",
            r.optional_for.join("  ")
        );
    }
    if !r.unneeded.is_empty() {
        println!("\nDependencies nothing else would need afterwards:");
        for i in r.unneeded.iter().take(30) {
            println!(
                "  {} {}  ({})",
                i.name,
                i.version,
                human_size(i.installed_size)
            );
        }
        if r.unneeded.len() > 30 {
            println!(
                "  and {} more (`--json` lists all {})",
                r.unneeded.len() - 30,
                r.unneeded.len()
            );
        }
    }
    if !r.units.is_empty() {
        println!("\nServices that would go:");
        for u in &r.units {
            println!(
                "  {}  ({}{})",
                u.unit,
                u.package,
                if u.enabled { ", enabled" } else { "" }
            );
        }
    }
    if !r.desktop_entries.is_empty() {
        println!(
            "\nDesktop entries that would go: {}",
            r.desktop_entries.join("  ")
        );
    }
    println!(
        "\nDisk space freed in all: {}",
        human_size(r.reclaimed_bytes)
    );
}

pub fn print_orphans(orphans: &[Orphan], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(orphans).unwrap());
        return;
    }

    if orphans.is_empty() {
        say!(Voice::Notes, "no orphaned dependencies.");
        return;
    }
    let noun = if orphans.len() == 1 {
        "orphaned dependency"
    } else {
        "orphaned dependencies"
    };
    say!(
        Voice::Found,
        "{} {noun}, which no package needs\n",
        orphans.len()
    );
    for o in orphans {
        println!(
            "  {} {}  ({})",
            o.name,
            o.version,
            human_size(o.installed_size)
        );
    }
    let total: u64 = orphans.iter().map(|o| o.installed_size).sum();
    println!("\n{} in total.", human_size(total));
}

pub fn print_pinned(pins: &[Pin], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(pins).unwrap());
        return;
    }
    for pin in pins {
        match pin.describe() {
            d if d.is_empty() => say!(Voice::Confirms, "pinned {}.", pin.name),
            d => say!(Voice::Confirms, "pinned {} ({d}).", pin.name),
        }
    }
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if pins.iter().any(|p| p.is_active(at)) {
        println!("`emrac upgrade` will now hold back what you pinned; `emrac unpin` releases it.");
    } else {
        say!(
            Voice::Warns,
            "that expiry date has already passed, so nothing is held back."
        );
    }
}

pub fn print_pins(pins: &[Pin], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(pins).unwrap());
        return;
    }
    if pins.is_empty() {
        say!(
            Voice::Notes,
            "nothing is pinned — `emrac pin <pkg>` holds a package back."
        );
        return;
    }
    say!(Voice::Found, "{}\n", plural(pins.len(), "pin"));
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    for pin in pins {
        let state = if pin.is_active(at) { "" } else { "  [expired]" };
        match pin.describe() {
            d if d.is_empty() => println!("  {}{state}", pin.name),
            d => println!("  {}  {d}{state}", pin.name),
        }
    }
}

/// Said alongside an upgrade plan, on stderr so `--json` stays parseable.
pub fn print_pin_notes(held: &[Pin], expired: &[Pin], quiet: bool) {
    if !held.is_empty() {
        let list: Vec<String> = held
            .iter()
            .map(|p| match p.describe() {
                d if d.is_empty() => p.name.clone(),
                d => format!("{} ({d})", p.name),
            })
            .collect();
        say!(
            Voice::Notes,
            "holding back {}: {}",
            plural(held.len(), "pinned package"),
            list.join(", ")
        );
    }
    if !quiet {
        for pin in expired {
            say!(
                Voice::Suggests,
                "the pin on {} expired{} and is no longer holding it back — `emrac unpin {}` clears it.",
                pin.name,
                pin.until_date().map(|d| format!(" on {d}")).unwrap_or_default(),
                pin.name
            );
        }
    }
}

fn print_findings(report: &AuditReport, out: &mut dyn std::io::Write) {
    for f in &report.findings {
        let _ = writeln!(
            out,
            "  [{}] {} {} — {}\n      {}",
            f.severity.label(),
            f.rule,
            if f.line == 0 {
                f.file.clone()
            } else {
                format!("{}:{}", f.file, f.line)
            },
            f.message,
            f.excerpt
        );
    }
}

/// One-line list of the critical findings, for the refusal message.
pub fn critical_summary(report: &AuditReport) -> String {
    report
        .of_severity(Severity::Critical)
        .map(|f| format!("{} ({}:{})", f.rule, f.file, f.line))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The audit shown during a build review: only when there is something to
/// say, on stderr so structured stdout stays clean.
pub fn print_audit(report: &AuditReport, to_stderr: bool) {
    if report.findings.iter().all(|f| f.severity == Severity::Info) {
        return;
    }
    let mut buf = Vec::new();
    let voice = match report.findings.iter().map(|f| f.severity).max() {
        Some(Severity::Critical) => Voice::Refuses,
        Some(Severity::High) => Voice::Warns,
        _ => Voice::Suspects,
    };
    let _ = writeln!(
        buf,
        "{} findings for '{}'\n",
        voice.prefix_for(to_stderr),
        report.target
    );
    print_findings(report, &mut buf);
    let _ = writeln!(
        buf,
        "\n(static reading only — it can miss things and can flag legitimate code)\n"
    );
    if to_stderr {
        let _ = std::io::stderr().write_all(&buf);
    } else {
        let _ = std::io::stdout().write_all(&buf);
    }
}

pub fn print_audits(reports: &[AuditReport], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(reports).unwrap());
        return;
    }
    for report in reports {
        if report.findings.is_empty() {
            say!(Voice::Found, "no findings in '{}'.", report.target);
            continue;
        }
        let verdict = match report.verdict() {
            Verdict::Allow => "nothing that blocks or needs a decision",
            Verdict::Confirm => "high-risk: it would stop and ask before building",
            Verdict::Block => "critical: it would refuse to build this",
        };
        // The worst finding sets the voice: static reading can only suspect a
        // warning, a high-risk finding is a warning, a critical one a refusal.
        // The report itself is the command's output, so it stays on stdout.
        let voice = match report.findings.iter().map(|f| f.severity).max() {
            Some(Severity::Critical) => Voice::Refuses,
            Some(Severity::High) => Voice::Warns,
            Some(Severity::Warning) => Voice::Suspects,
            _ => Voice::Found,
        };
        emrac_core::voice::emit_on(
            voice,
            false,
            format_args!(
                "{} in '{}' — {verdict}\n",
                plural(report.findings.len(), "finding"),
                report.target
            ),
        );
        let mut out = std::io::stdout();
        print_findings(report, &mut out);
        println!();
    }
    println!("(static reading only — it can miss things and can flag legitimate code)");
}

/// What a built package would change, shown before it is installed.
pub fn print_manifest(m: &Manifest, to_stderr: bool) {
    let mut buf = Vec::new();
    let _ = writeln!(
        buf,
        "{} {} {} — {}, {} installed\n",
        Voice::Found.prefix_for(to_stderr),
        m.package,
        m.version,
        plural(m.files, "file"),
        human_size(m.installed_size)
    );
    for line in &m.summary {
        let _ = writeln!(buf, "  - {line}");
    }
    if !m.summary.is_empty() {
        let _ = writeln!(buf);
    }
    if m.report
        .findings
        .iter()
        .all(|f| f.severity == Severity::Info)
    {
        let _ = writeln!(buf, "  nothing here needs your attention.\n");
    } else {
        print_findings(&m.report, &mut buf);
        let _ = writeln!(buf);
    }
    if to_stderr {
        let _ = std::io::stderr().write_all(&buf);
    } else {
        let _ = std::io::stdout().write_all(&buf);
    }
}

/// One `emrac audit` target's result: a source audit or a package manifest.
pub enum AuditItem {
    Source(AuditReport),
    Package(Manifest),
}

impl AuditItem {
    pub fn report(&self) -> &AuditReport {
        match self {
            AuditItem::Source(r) => r,
            AuditItem::Package(m) => &m.report,
        }
    }
}

pub fn print_audit_items(items: &[AuditItem], json: bool) {
    if json {
        let values: Vec<serde_json::Value> = items
            .iter()
            .map(|i| match i {
                AuditItem::Source(r) => serde_json::to_value(r),
                AuditItem::Package(m) => serde_json::to_value(m),
            })
            .map(|v| v.unwrap())
            .collect();
        println!("{}", serde_json::to_string_pretty(&values).unwrap());
        return;
    }
    for item in items {
        match item {
            AuditItem::Source(report) => print_audits(std::slice::from_ref(report), false),
            AuditItem::Package(m) => {
                print_manifest(m, false);
                let verdict = match m.report.verdict() {
                    Verdict::Allow => None,
                    Verdict::Confirm => Some((
                        Voice::Warns,
                        "it would stop and ask before installing this (high-risk changes)",
                    )),
                    Verdict::Block => Some((
                        Voice::Refuses,
                        "it would not install this (a critical finding)",
                    )),
                };
                if let Some((voice, v)) = verdict {
                    say!(voice, "{v}.");
                }
            }
        }
    }
}

/// The result of inspecting official packages before installing them: only
/// packages with something to say are shown in full.
pub fn print_official_manifests(manifests: &[&Manifest], missing: &[String]) {
    let flagged: Vec<&&Manifest> = manifests
        .iter()
        .filter(|m| {
            m.report
                .findings
                .iter()
                .any(|f| f.severity > Severity::Info)
        })
        .collect();
    let mut buf = Vec::new();
    for m in &flagged {
        let _ = writeln!(
            buf,
            "{} {} {}\n",
            Voice::Found.prefix_for(true),
            m.package,
            m.version
        );
        print_findings(&m.report, &mut buf);
        let _ = writeln!(buf);
    }
    let _ = writeln!(
        buf,
        "{} inspected {}: {} with something to note, {} clean.{}",
        Voice::Notes.prefix_for(true),
        plural(manifests.len(), "official package"),
        flagged.len(),
        manifests.len() - flagged.len(),
        if missing.is_empty() {
            String::new()
        } else {
            format!(
                " Not found in the cache to inspect: {}.",
                missing.join(", ")
            )
        }
    );
    let _ = std::io::stderr().write_all(&buf);
}

pub fn print_doctor(report: &DoctorReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(report).unwrap());
        return;
    }

    for c in &report.checks {
        let mark = match c.status {
            DoctorStatus::Ok => "  ok     ",
            DoctorStatus::Note => "  note   ",
            DoctorStatus::Warning => "  warning",
            DoctorStatus::Problem => "  PROBLEM",
        };
        println!("{mark}  {}: {}", c.title, c.summary);
        for d in &c.detail {
            println!("            - {d}");
        }
        if let Some(fix) = &c.fix {
            if c.status != DoctorStatus::Ok {
                println!("            {} {fix}", Voice::Suggests.short_for(false));
            }
        }
    }

    let (problems, warnings) = (
        report.count(DoctorStatus::Problem),
        report.count(DoctorStatus::Warning),
    );
    println!();
    if problems + warnings == 0 {
        say!(Voice::Found, "nothing wrong.");
    } else {
        say!(
            Voice::Found,
            "{} and {}.",
            plural(problems, "problem"),
            plural(warnings, "warning")
        );
    }
}

/// Said alongside an upgrade plan that was not computed from freshly synced
/// repositories. `None` when the plan can be trusted as it stands.
pub fn stale_list_note(list: &PackageList) -> Option<String> {
    let PackageList::Stale { days, why } = list else {
        return None;
    };
    let age = match days {
        Some(d) => format!("{d} days old"),
        None => "of unknown age".to_string(),
    };
    let reason = match why.as_deref() {
        Some("--offline") => " (you passed --offline)".to_string(),
        Some(err) => {
            let err = err
                .strip_prefix("couldn't refresh the package list: ")
                .unwrap_or(err);
            format!(" (it couldn't be refreshed for this preview: {err})")
        }
        None => String::new(),
    };
    Some(format!(
        "this plan comes from a package list that is {age}{reason}, so anything the repositories published since is not in it. The real upgrade refreshes first and may include more."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_list_needs_no_note() {
        assert_eq!(stale_list_note(&PackageList::Fresh), None);
    }

    #[test]
    fn a_stale_list_says_how_old_it_is_and_why() {
        let note = stale_list_note(&PackageList::Stale {
            days: Some(10),
            why: Some("couldn't refresh the package list: `fakeroot` isn't installed".into()),
        })
        .unwrap();
        assert!(note.contains("10 days old"), "{note}");
        assert!(
            note.contains(
                "(it couldn't be refreshed for this preview: `fakeroot` isn't installed)"
            ),
            "{note}"
        );
        assert!(note.contains("may include more"), "{note}");
        assert!(!note.contains("up to date"), "{note}");
    }

    #[test]
    fn the_offline_flag_and_an_unknown_age_are_named_plainly() {
        let offline = stale_list_note(&PackageList::Stale {
            days: Some(3),
            why: Some("--offline".into()),
        })
        .unwrap();
        assert!(offline.contains("(you passed --offline)"), "{offline}");
        let unknown = stale_list_note(&PackageList::Stale {
            days: None,
            why: None,
        })
        .unwrap();
        assert!(unknown.contains("of unknown age"), "{unknown}");
    }
}
