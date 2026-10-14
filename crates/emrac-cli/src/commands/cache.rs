use std::collections::BTreeMap;

use emrac_core::{
    parse_duration, parse_size, CacheDecision, CacheOp, CacheScope, Error, Result, Sources,
};

use crate::cli::{CacheArgs, CacheCommand, CacheScopes};
use crate::output::human_size;
use crate::prompt::confirm_default_no;

pub fn run(
    sources: &Sources,
    command: Option<&CacheCommand>,
    dry_run: bool,
    yes: bool,
    json: bool,
    verbose: bool,
) -> Result<()> {
    match command {
        None => overview(sources, &CacheScopes::default(), false, json),
        Some(CacheCommand::List(scopes)) => overview(sources, scopes, true, json),
        Some(CacheCommand::Stats(scopes)) => overview(sources, scopes, false, json),
        Some(CacheCommand::Clean(args)) => {
            change(sources, args, CacheOp::Clean, dry_run, yes, json, verbose)
        }
        Some(CacheCommand::Purge(args)) => {
            change(sources, args, CacheOp::Purge, dry_run, yes, json, verbose)
        }
        Some(CacheCommand::Trim { size_limit, args }) => change(
            sources,
            args,
            CacheOp::Trim(parse_size(size_limit)?),
            dry_run,
            yes,
            json,
            verbose,
        ),
    }
}

/// The scopes named by words, letters or flags, or `default` when none are.
fn scopes_of(s: &CacheScopes, default: &[CacheScope]) -> Result<Vec<CacheScope>> {
    let mut out: Vec<CacheScope> = Vec::new();
    for word in &s.scope {
        out.push(
            CacheScope::parse(word).ok_or_else(|| Error::InvalidCacheValue {
                what: "cache category",
                value: word.clone(),
                hint: "use source|s, build|b, binary|n or package|p",
            })?,
        );
    }
    for (set, scope) in [
        (s.source, CacheScope::Source),
        (s.build, CacheScope::Build),
        (s.binary, CacheScope::Binary),
        (s.package, CacheScope::Package),
    ] {
        if set {
            out.push(scope);
        }
    }
    out.sort();
    out.dedup();
    Ok(if out.is_empty() {
        default.to_vec()
    } else {
        out
    })
}

fn overview(sources: &Sources, scopes: &CacheScopes, largest: bool, json: bool) -> Result<()> {
    let scopes = scopes_of(scopes, &CacheScope::ALL)?;
    let decisions = sources.cache_plan(&scopes, CacheOp::Clean, None)?;
    let mut rows: BTreeMap<CacheScope, (usize, u64, u64)> = BTreeMap::new();
    for d in &decisions {
        let row = rows.entry(d.entry.scope).or_default();
        row.0 += 1;
        row.1 += d.entry.bytes;
        if d.remove {
            row.2 += d.entry.bytes;
        }
    }
    let mut per_package: BTreeMap<(CacheScope, String), u64> = BTreeMap::new();
    for d in &decisions {
        *per_package
            .entry((d.entry.scope, d.entry.package.clone()))
            .or_default() += d.entry.bytes;
    }
    let mut top: Vec<_> = per_package.into_iter().collect();
    top.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
    top.truncate(12);

    if json {
        let scopes_json: Vec<_> = rows
            .iter()
            .map(|(s, (files, bytes, reclaim))| {
                serde_json::json!({ "scope": s, "files": files, "bytes": bytes, "reclaimable": reclaim })
            })
            .collect();
        let top_json: Vec<_> = top
            .iter()
            .map(|((s, p), b)| serde_json::json!({ "scope": s, "package": p, "bytes": b }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({ "scopes": scopes_json, "largest": top_json })
            )
            .unwrap()
        );
        return Ok(());
    }

    let total: u64 = rows.values().map(|r| r.1).sum();
    let reclaim: u64 = rows
        .iter()
        .filter(|(s, _)| **s != CacheScope::Source)
        .map(|(_, r)| r.2)
        .sum();
    println!(
        "emrac found: {} in the caches; `emrac cache clean` would free {}.\n",
        human_size(total),
        human_size(reclaim)
    );
    println!(
        "  {:<8} {:>7} {:>12} {:>14}",
        "scope", "files", "size", "clean frees"
    );
    for (scope, (files, bytes, reclaim)) in &rows {
        let note = if *scope == CacheScope::Source {
            "  (only when named)"
        } else {
            ""
        };
        println!(
            "  {:<8} {:>7} {:>12} {:>14}{note}",
            scope.label(),
            files,
            human_size(*bytes),
            human_size(*reclaim)
        );
    }
    if largest && !top.is_empty() {
        println!("\nLargest:");
        for ((scope, package), bytes) in &top {
            println!(
                "  {:>12}  {:<8} {package}",
                human_size(*bytes),
                scope.label()
            );
        }
    }
    Ok(())
}

fn change(
    sources: &Sources,
    args: &CacheArgs,
    op: CacheOp,
    dry_run: bool,
    yes: bool,
    json: bool,
    verbose: bool,
) -> Result<()> {
    let default: &[CacheScope] = match op {
        CacheOp::Purge => &CacheScope::ALL,
        _ => &[CacheScope::Build, CacheScope::Binary, CacheScope::Package],
    };
    let named = scopes_of(&args.scopes, &[])?;
    if op == CacheOp::Purge && named.is_empty() && !args.force {
        return Err(Error::CacheNeedsForce);
    }
    let scopes = if named.is_empty() {
        default.to_vec()
    } else {
        named
    };
    let older_than = args.older_than.as_deref().map(parse_duration).transpose()?;
    let decisions = sources.cache_plan(&scopes, op, older_than)?;
    let doomed: Vec<&CacheDecision> = decisions.iter().filter(|d| d.remove).collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&decisions).unwrap());
        if dry_run || doomed.is_empty() {
            return Ok(());
        }
    } else if dry_run {
        // What goes is the point of a dry run; what stays is summarized by
        // reason unless asked for (-v).
        let mut kept: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
        for d in &decisions {
            if d.remove || verbose {
                println!(
                    "{} {:>12}  {:<8} {}  — {}",
                    if d.remove { "remove" } else { "keep  " },
                    human_size(d.entry.bytes),
                    d.entry.scope.label(),
                    d.entry.path.display(),
                    d.reason
                );
            } else {
                let row = kept.entry(d.reason.as_str()).or_default();
                row.0 += 1;
                row.1 += d.entry.bytes;
            }
        }
        if !kept.is_empty() {
            println!("\nKept (add -v to list each):");
            for (reason, (count, bytes)) in &kept {
                println!("  {:>6}  {:>12}  {reason}", count, human_size(*bytes));
            }
        }
        println!(
            "\nemrac plans: {} would be removed, freeing {} (dry run, nothing changed).",
            plural(doomed.len(), "entry", "entries"),
            human_size(doomed.iter().map(|d| d.entry.bytes).sum())
        );
        return Ok(());
    }
    if doomed.is_empty() {
        if !json {
            println!("emrac notes: nothing to remove.");
        }
        return Ok(());
    }

    if !json {
        summary(&doomed, op);
    }
    if !yes
        && !confirm_default_no(&format!(
            "Remove {} ({})?",
            plural(doomed.len(), "entry", "entries"),
            human_size(doomed.iter().map(|d| d.entry.bytes).sum())
        ))
    {
        println!("emrac notes: okay, nothing removed.");
        return Ok(());
    }

    let outcome = sources.cache_apply(&decisions)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome).unwrap());
    } else {
        println!(
            "emrac notes: removed {}, freed {}.",
            plural(outcome.removed, "entry", "entries"),
            human_size(outcome.freed)
        );
        for (path, why) in &outcome.failed {
            eprintln!("emrac warns: couldn't remove {}: {why}", path.display());
        }
    }
    Ok(())
}

fn summary(doomed: &[&CacheDecision], op: CacheOp) {
    let mut per: BTreeMap<CacheScope, (usize, u64)> = BTreeMap::new();
    for d in doomed {
        let row = per.entry(d.entry.scope).or_default();
        row.0 += 1;
        row.1 += d.entry.bytes;
    }
    println!("emrac plans: remove");
    for (scope, (count, bytes)) in &per {
        println!(
            "  {:<8} {} ({})",
            scope.label(),
            plural(*count, "entry", "entries"),
            human_size(*bytes)
        );
    }
    if per.contains_key(&CacheScope::Package) {
        println!("  pacman's cache belongs to root, so those files are removed through sudo.");
    }
    if per.contains_key(&CacheScope::Source) {
        println!(
            "  Some sources can't be downloaded again by a program (a vendor that wants a login)."
        );
    }
    if op == CacheOp::Purge {
        println!("  A purge ignores the protections: no `undo` of those versions, and the next build starts from nothing.");
    }
    println!("  `emrac cache clean --dry-run` lists every path and why.\n");
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
