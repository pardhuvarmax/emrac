use emrac_core::{parse_date, PinStore, Result, Sources};
use emrac_core::{say, Voice};

use crate::output;

pub fn pin(
    sources: &Sources,
    pkgs: &[String],
    reason: Option<&str>,
    until: Option<&str>,
    json: bool,
) -> Result<()> {
    let until = until.map(parse_date).transpose()?;
    let pins = sources.pin(pkgs, reason, until)?;
    output::print_pinned(&pins, json);
    Ok(())
}

pub fn list(json: bool) -> Result<()> {
    output::print_pins(&PinStore::list()?, json);
    Ok(())
}

pub fn unpin(pkgs: &[String], json: bool) -> Result<()> {
    PinStore::unpin(pkgs)?;
    if json {
        println!("{}", serde_json::json!({ "unpinned": pkgs }));
    } else {
        say!(Voice::Confirms, "unpinned {}.", pkgs.join(", "));
    }
    Ok(())
}
