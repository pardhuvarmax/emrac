use emrac_core::{PackageDetails, PackageSummary};

pub fn print_search_results(results: &[PackageSummary], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(results).unwrap());
        return;
    }

    if results.is_empty() {
        println!("No packages found.");
        return;
    }

    for pkg in results {
        println!("{}/{} {}", pkg.repo, pkg.name, pkg.version);
        if let Some(desc) = pkg.description.as_deref().filter(|d| !d.is_empty()) {
            println!("    {desc}");
        }
    }
}

pub fn print_package_details(pkg: &PackageDetails, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(pkg).unwrap());
        return;
    }

    println!("Name            : {}", pkg.name);
    println!("Version         : {}", pkg.version);
    println!("Repository      : {}", pkg.repo);
    println!(
        "Description     : {}",
        pkg.description.as_deref().unwrap_or("-")
    );
    println!("URL             : {}", pkg.url.as_deref().unwrap_or("-"));
    println!("Licenses        : {}", join_or_dash(&pkg.license));
    println!("Depends On      : {}", join_or_dash(&pkg.depends));
    println!("Provides        : {}", join_or_dash(&pkg.provides));
    println!(
        "Installed Size  : {}",
        pkg.installed_size.map(human_size).as_deref().unwrap_or("-")
    );

    if let Some(maintainer) = &pkg.maintainer {
        println!("Maintainer      : {maintainer}");
    }
    if let Some(votes) = pkg.votes {
        println!("Votes           : {votes}");
    }
    if let Some(popularity) = pkg.popularity {
        println!("Popularity      : {popularity:.2}");
    }
    if pkg.out_of_date.is_some() {
        println!("Out of Date     : yes");
    }
}

fn join_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join("  ")
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.2} {}", UNITS[unit])
}
