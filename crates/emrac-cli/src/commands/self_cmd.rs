use emrac_core::self_check;
use emrac_core::{DoctorStatus, Error, Result};

use crate::output;

/// `emrac self check`. Exits 1 when a check found a *problem*, like `doctor`.
pub fn check(json: bool) -> Result<()> {
    let report = self_check::evaluate(&self_check::gather());
    output::print_doctor(&report, json);
    let problems = report.count(DoctorStatus::Problem);
    if problems > 0 {
        return Err(Error::DoctorProblems(if problems == 1 {
            "1 problem".to_string()
        } else {
            format!("{problems} problems")
        }));
    }
    Ok(())
}
