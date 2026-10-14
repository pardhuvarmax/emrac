use emrac_core::{DoctorStatus, Error, Result, Sources};

use crate::output;

/// `emrac doctor`. Exits 1 when any check found a *problem* (warnings and
/// notes don't fail a script).
pub fn run(sources: &Sources, report_for_bug: bool, online: bool, json: bool) -> Result<()> {
    let report = sources.doctor(online)?;

    if report_for_bug {
        print!("{}", sources.doctor_markdown(&report));
    } else {
        output::print_doctor(&report, json);
    }

    let problems = report.count(DoctorStatus::Problem);
    if problems > 0 {
        let what = if problems == 1 {
            "1 problem".to_string()
        } else {
            format!("{problems} problems")
        };
        return Err(Error::DoctorProblems(what));
    }
    Ok(())
}
