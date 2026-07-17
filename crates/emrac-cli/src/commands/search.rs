use anyhow::Result;
use emrac_core::AlpmBackend;

use crate::output;

pub fn run(backend: &AlpmBackend, query: &str, limit: Option<usize>, json: bool) -> Result<()> {
    let results = backend.search(query, limit);
    output::print_search_results(&results, json);
    Ok(())
}
