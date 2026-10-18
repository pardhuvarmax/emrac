//! `emrac score` (u18): a report card for a package.

use emrac_core::score;
use emrac_core::style::{Activity, Style};
use emrac_core::{Result, Sources};

use crate::view;

pub fn run(sources: &Sources, pkg: &str, details: bool, offline: bool, json: bool) -> Result<()> {
    let activity = Activity::spinner(&format!("gathering what is known about {pkg}"));
    let facts = sources.score_facts(pkg, offline);
    match &facts {
        Ok(_) => activity.finish(&format!("gathered what is known about {pkg}")),
        Err(_) => activity.fail(&format!("could not look up {pkg}")),
    }
    let card = score::card(&facts?);
    if json {
        println!("{}", serde_json::to_string_pretty(&card).unwrap());
        return Ok(());
    }
    let st = Style::stdout();
    if st.fancy {
        print!("{}", view::score_view(&st, &card, details));
    } else {
        print!("{}", view::score_plain(&card, details));
    }
    Ok(())
}
