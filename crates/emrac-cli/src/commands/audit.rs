use std::path::Path;

use emrac_core::{audit_path, inspect_package, Error, Result, Severity, Sources};

use crate::output::{self, AuditItem};

/// A target is a path when it exists on disk or looks like one; otherwise an
/// AUR package name.
pub fn is_path(target: &str) -> bool {
    Path::new(target).exists() || target.contains('/')
}

/// A built package file, which gets a manifest instead of a source audit.
fn is_package_file(target: &str) -> bool {
    let name = target.rsplit('/').next().unwrap_or(target);
    name.contains(".pkg.tar") && !name.ends_with(".sig")
}

/// Prints every result, then fails (exit 1) if any target has high or
/// critical findings, so scripts can gate on it. `sources` may be `None` only
/// when every target is a path.
pub fn run(sources: Option<&Sources>, targets: &[String], json: bool) -> Result<()> {
    let mut items = Vec::new();
    for target in targets {
        let path = Path::new(target);
        items.push(match sources {
            Some(sources) if !is_path(target) => AuditItem::Source(sources.audit_aur(target)?),
            _ if is_package_file(target) => AuditItem::Package(inspect_package(path)?),
            _ => AuditItem::Source(audit_path(path)?),
        });
    }

    output::print_audit_items(&items, json);

    let bad: Vec<&str> = items
        .iter()
        .filter(|i| i.report().highest() >= Some(Severity::High))
        .map(|i| i.report().target.as_str())
        .collect();
    if bad.is_empty() {
        Ok(())
    } else {
        Err(Error::AuditFlagged(bad.join(", ")))
    }
}
