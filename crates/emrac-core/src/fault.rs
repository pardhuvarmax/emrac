//! Fault points for the container tests, compiled in only with
//! `RUSTFLAGS="--cfg emrac_fault"`. A normal build has no code here at all: the
//! function is empty and inlined away, and there is no switch to turn one on.
//! In the test build, `EMRAC_FAULT=<point>` makes the process abort (no cleanup,
//! as in a kill or a power cut) when it reaches that point, so that recovery can
//! be tested from the state a real crash leaves.

/// A place where the tests may cut emrac off, or change things under it:
/// `EMRAC_FAULT=<point>` aborts there; `EMRAC_FAULT_TAMPER='<point>:<command>'`
/// runs the command there (to alter files between a review and a build).
#[cfg(emrac_fault)]
pub fn point(name: &str) {
    if std::env::var("EMRAC_FAULT").as_deref() == Ok(name) {
        eprintln!("emrac fault: cut off at '{name}'");
        std::process::abort();
    }
    if let Ok(spec) = std::env::var("EMRAC_FAULT_TAMPER") {
        if let Some(command) = spec.strip_prefix(&format!("{name}:")) {
            eprintln!("emrac fault: running a command at '{name}'");
            let _ = std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .status();
        }
    }
}

#[cfg(not(emrac_fault))]
#[inline(always)]
pub fn point(_name: &str) {}
