//! `emrac score` (u18): a report card for a package.

use emrac_core::score::{self, Weights};
use emrac_core::style::{Activity, Style};
use emrac_core::{Error, Result, Sources};

use crate::view;

/// How the overall number is weighted: the config, then `--care`, then `--weights`.
pub struct Asked<'a> {
    pub base: Weights,
    pub weights: Option<&'a str>,
    pub care: Option<&'a str>,
}

impl Asked<'_> {
    fn resolve(&self) -> Result<Weights> {
        let mut w = self.base;
        if let Some(care) = self.care {
            w = w.caring_about(care).map_err(Error::Help)?;
        }
        if let Some(list) = self.weights {
            w = w.with_list(list).map_err(Error::Help)?;
        }
        Ok(w)
    }
}

pub fn run(
    sources: &Sources,
    pkg: &str,
    details: bool,
    asked: Asked<'_>,
    offline: bool,
    json: bool,
) -> Result<()> {
    let weights = asked.resolve()?;
    let activity = Activity::spinner(&format!("gathering what is known about {pkg}"));
    let facts = sources.score_facts(pkg, offline);
    match &facts {
        Ok(_) => activity.finish(&format!("gathered what is known about {pkg}")),
        Err(_) => activity.fail(&format!("could not look up {pkg}")),
    }
    let card = score::card_with(&facts?, &weights);
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
