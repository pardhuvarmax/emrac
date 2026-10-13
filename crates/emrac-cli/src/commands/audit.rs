use std::path::Path;

use emrac_core::{audit_path, Error, Result, Severity, Sources};

use crate::output;

/// A target is a path when it exists on disk or looks like one; otherwise an
/// AUR package name.
pub fn is_path(target: &str) -> bool {
    Path::new(target).exists() || target.contains('/')
}

/// Prints every report, then fails (exit 1) if any target has high or
/// critical findings, so scripts can gate on it. `sources` may be `None` only
/// when every target is a path.
pub fn run(sources: Option<&Sources>, targets: &[String], json: bool) -> Result<()> {
    let mut reports = Vec::new();
    for target in targets {
        reports.push(match sources {
            Some(sources) if !is_path(target) => sources.audit_aur(target)?,
            _ => audit_path(Path::new(target))?,
        });
    }

    output::print_audits(&reports, json);

    let bad: Vec<&str> = reports
        .iter()
        .filter(|r| r.highest() >= Some(Severity::High))
        .map(|r| r.target.as_str())
        .collect();
    if bad.is_empty() {
        Ok(())
    } else {
        Err(Error::AuditFlagged(bad.join(", ")))
    }
}
