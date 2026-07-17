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

    let results = sources.search(query, scope, limit)?;

    if let Some(warning) = &results.aur_warning {
        if !quiet {
            eprintln!("emrac warns: {warning}");
        }
    }

    output::print_search_results(query, &results.packages, json);
    Ok(())
}
