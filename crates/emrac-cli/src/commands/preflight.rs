use emrac_core::{say, Config, Error, Plan, Result, Sources, Voice};

use emrac_core::DoctorStatus as Status;

use crate::output;

/// What a command tells the pre-flight about its run.
pub struct Options {
    pub dry_run: bool,
    pub offline: bool,
    pub json: bool,
    /// `--allow-partial`: this one run may be a partial upgrade.
    pub allow_partial: bool,
    /// Apply the partial-upgrade gate (only `install` and `upgrade` do).
    pub check_partial: bool,
    pub aur_builds: bool,
}

/// Runs the pre-flight checks for `plan` and stops the command on a
/// problem, as the `[safety]` settings say. A dry run only reports.
pub fn run(sources: &Sources, plan: &Plan, opts: &Options) -> Result<()> {
    let safety = Config::load()?.safety;
    if safety.preflight == "off" {
        return Ok(());
    }
    let mut checks = sources.preflight(plan, opts.aur_builds, opts.offline, opts.check_partial)?;

    let mut allowed = false;
    checks.retain_mut(|c| {
        if c.id != "partial-upgrade" {
            return true;
        }
        match safety.partial_upgrade.as_str() {
            "off" => false,
            "warn" => {
                if c.status == Status::Problem {
                    c.status = Status::Warning;
                }
                true
            }
            _ if opts.allow_partial && c.status == Status::Problem => {
                c.status = Status::Warning;
                allowed = true;
                true
            }
            _ => true,
        }
    });
    if safety.preflight == "warn" {
        for c in &mut checks {
            if c.status == Status::Problem {
                c.status = Status::Warning;
            }
        }
    }

    if !opts.json {
        output::print_preflight(&checks, allowed);
    }
    let problems: Vec<&str> = checks
        .iter()
        .filter(|c| c.status == Status::Problem)
        .map(|c| c.title)
        .collect();
    if problems.is_empty() || opts.dry_run {
        if !problems.is_empty() && !opts.json {
            say!(
                Voice::Notes,
                "this is a dry run, so nothing was stopped; the real run would refuse."
            );
        }
        return Ok(());
    }
    Err(Error::Preflight(format!(
        "{}. Nothing was changed. (The settings `safety.preflight` and `safety.partial_upgrade` say how strict this is.)",
        problems.join("; ")
    )))
}
