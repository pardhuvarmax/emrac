//! Fault points for the container tests, compiled in only with
//! `RUSTFLAGS="--cfg emrac_fault"`. A normal build has no code here at all: the
//! function is empty and inlined away, and there is no switch to turn one on.
//! In the test build, `EMRAC_FAULT=<point>` makes the process abort (no cleanup,
//! as in a kill or a power cut) when it reaches that point, so that recovery can
//! be tested from the state a real crash leaves.

/// A place where the tests may cut emrac off.
#[cfg(emrac_fault)]
pub fn point(name: &str) {
    if std::env::var("EMRAC_FAULT").as_deref() == Ok(name) {
        eprintln!("emrac fault: cut off at '{name}'");
        std::process::abort();
    }
}

#[cfg(not(emrac_fault))]
#[inline(always)]
pub fn point(_name: &str) {}
