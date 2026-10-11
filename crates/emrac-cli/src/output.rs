use std::collections::HashMap;

use emrac_core::{
    PackageDetails, PackageSummary, Plan, PlanAction, RollbackPlan, TransactionRecord, TxStatus,
};

pub fn print_search_results(query: &str, results: &[PackageSummary], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(results).unwrap());
        return;
    }

    if results.is_empty() {
        println!("emrac found: nothing matching '{query}'. Try a shorter or different term.");
        return;
    }

    println!(
        "emrac found: {} matching '{query}'\n",
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

    println!("emrac found: {} in {}\n", pkg.name, pkg.repo);

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
        };
        println!("emrac notes: {note}");
        return;
    }

    let verb = match plan.action {
        PlanAction::Install => "install",
        PlanAction::Remove => "remove",
        PlanAction::Upgrade => "upgrade",
        PlanAction::Downgrade => "downgrade",
    };

    println!(
        "emrac plans: {verb} {}\n",
        plural(plan.packages.len(), "package")
    );
    for pkg in &plan.packages {
        let suffix = if pkg.explicit { "" } else { " (dependency)" };
        println!("  {}/{} {}{suffix}", pkg.repo, pkg.name, pkg.version);
    }
    println!();

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
    println!("emrac notes: PKGBUILD for '{pkg}' (first build):\n");
    println!("{content}");
}

pub fn print_pkgbuild_diff(pkg: &str, diff: &str) {
    println!("emrac notes: PKGBUILD changes for '{pkg}' since your last build:\n");
    println!("{diff}");
}

pub fn print_rollback(rollback: &RollbackPlan, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(rollback).unwrap());
        return;
    }

    let plan = &rollback.plan;
    if plan.is_empty() {
        println!(
            "emrac notes: nothing left to roll back for {} — none of its packages are as it left them.",
            rollback.target
        );
    } else {
        let verb = match plan.action {
            PlanAction::Remove => "remove",
            PlanAction::Install => "reinstall",
            PlanAction::Downgrade => "downgrade",
            PlanAction::Upgrade => "upgrade",
        };
        let what = match &rollback.reapplies {
            Some(original) => format!("redo {original} (by reversing {})", rollback.target),
            None => format!("roll back {}", rollback.target),
        };
        println!(
            "emrac plans: {what} — {verb} {}\n",
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

    if !rollback.downloads.is_empty() {
        println!("\nWill download from the Arch Linux Archive:");
        for download in &rollback.downloads {
            println!("  {} {}", download.name, download.version);
        }
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
    json: bool,
) {
    if json {
        println!("{}", serde_json::to_string_pretty(records).unwrap());
        return;
    }

    if records.is_empty() {
        println!("emrac notes: no transactions found.");
        return;
    }

    println!(
        "emrac found: {} (times are UTC)\n",
        plural(records.len(), "transaction")
    );

    for record in records {
        let action = match record.action {
            PlanAction::Install => "install",
            PlanAction::Remove => "remove",
            PlanAction::Upgrade => "upgrade",
            PlanAction::Downgrade => "downgrade",
        };
        let status = match record.status {
            TxStatus::Committed => "committed",
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

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.2} {}", UNITS[unit])
}
