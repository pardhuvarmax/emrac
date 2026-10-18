use emrac_core::style::Activity;
use emrac_core::{say, Voice};
use emrac_core::{Result, SearchScope, Sources};

use crate::output;

#[allow(clippy::too_many_arguments)]
pub fn run(
    sources: &Sources,
    query: &str,
    limit: Option<usize>,
    official: bool,
    aur: bool,
    offline: bool,
    json: bool,
    quiet: bool,
) -> Result<()> {
    let mut scope = if !official && !aur {
        SearchScope::default()
    } else {
        SearchScope { official, aur }
    };

    // clap rejects an explicit --aur alongside --offline (see cli.rs); this
    // only handles the implicit default-scope case.
    if offline {
        scope.aur = false;
    }

    let activity = Activity::spinner(if scope.aur {
        "searching the repositories and the AUR"
    } else {
        "searching the repositories"
    });
    let found = sources.search(query, scope, limit);
    match &found {
        Ok(r) => activity.finish(&format!("searched: {} found", r.packages.len())),
        Err(_) => activity.fail("the search failed"),
    }
    let results = found?;

    if let Some(warning) = &results.aur_warning {
        if !quiet {
            say!(Voice::Warns, "{warning}");
        }
    }

    output::print_search_results(query, &results.packages, json);
    Ok(())
}
