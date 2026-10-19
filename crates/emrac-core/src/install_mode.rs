//! Source or binary on `install` (u23): which of the packages named come from the
//! repositories as binaries, which are built from source, and why. The choice is
//! yours (`--source`, `--binary`) or, with `install.prefer = "recommend"`, the
//! verdict of `emrac recommend`. Pure: the callers read the facts.

use serde::{Deserialize, Serialize};

use crate::recommend::{Confidence, Verdict};

/// How `install` treats the packages it is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode {
    /// What the setting `install.prefer` says.
    #[default]
    Auto,
    /// Official packages are built from Arch's packaging source here.
    Source,
    /// Nothing is compiled: a package that exists only as source is refused.
    Binary,
}

/// What `install.prefer` says to do when you did not choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Prefer {
    /// The repository's binary, always (the default).
    #[default]
    Repository,
    /// Build from source only where `recommend` says source with some confidence.
    Recommend,
}

impl Prefer {
    pub fn parse(word: &str) -> Option<Prefer> {
        match word {
            "repository" => Some(Prefer::Repository),
            "recommend" => Some(Prefer::Recommend),
            _ => None,
        }
    }
}

/// Where an official package comes from in a plan: its repository, `+source`
/// added when it is built here.
pub const SOURCE_SUFFIX: &str = "+source";

pub fn is_source_repo(repo: &str) -> bool {
    repo.ends_with(SOURCE_SUFFIX)
}

/// The repository a plan entry belongs to, without the `+source`.
pub fn base_repo(repo: &str) -> &str {
    repo.strip_suffix(SOURCE_SUFFIX).unwrap_or(repo)
}

/// Whether to build an official package from source. `verdict` is what
/// `recommend` says (when it was asked).
pub fn build_from_source(
    mode: SourceMode,
    prefer: Prefer,
    verdict: Option<(Verdict, Confidence)>,
) -> bool {
    match mode {
        SourceMode::Source => true,
        SourceMode::Binary => false,
        SourceMode::Auto => {
            prefer == Prefer::Recommend
                && matches!(
                    verdict,
                    Some((Verdict::Source, Confidence::Medium | Confidence::High))
                )
        }
    }
}

/// The package a built file holds, from its name: `sl-5.05-6-x86_64.pkg.tar.zst`
/// is `sl`. `None` when it does not look like a package file.
pub fn package_of_file(file_name: &str) -> Option<&str> {
    let stem = file_name.split(".pkg.tar").next()?;
    let mut parts = stem.rsplitn(4, '-');
    let _arch = parts.next()?;
    let _pkgrel = parts.next()?;
    let _pkgver = parts.next()?;
    parts.next().filter(|n| !n.is_empty())
}

/// The git tags Arch's packaging repository may use for a version: epochs are
/// written with a dash (`1-2.0-1`) there.
pub fn tag_candidates(version: &str) -> Vec<String> {
    let mut tags = vec![version.replace(':', "-")];
    if version.contains(':') {
        tags.push(version.to_string());
    }
    tags
}

/// A name that may be put in a repository address.
pub fn valid_base(base: &str) -> bool {
    !base.is_empty()
        && base.len() <= 100
        && !base.starts_with(['-', '.'])
        && base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-'))
}

/// What a failed fetch of the packaging source means, from what git said.
pub fn explain_fetch_failure(stderr: &str, base: &str, version: &str, url: &str) -> String {
    let git = stderr.lines().last().unwrap_or("").trim();
    let lower = stderr.to_lowercase();
    let why = if lower.contains("remote branch") && lower.contains("not found") {
        format!(
            "Arch's packaging repository for '{base}' has no tag for {version}. The binary may have been built from a revision that was not tagged; `--source` cannot build exactly it"
        )
    } else if lower.contains("could not read username")
        || lower.contains("repository not found")
        || lower.contains("does not appear to be a git repository")
        || lower.contains("not found")
    {
        format!(
            "there is no public packaging repository for '{base}' at {url} (some packages are not published there). `--source` cannot build it; `--binary` installs the repository's"
        )
    } else if lower.contains("could not resolve host")
        || lower.contains("unable to access")
        || lower.contains("timed out")
    {
        format!(
            "the packaging source at {url} could not be reached; check the network and try again"
        )
    } else {
        format!("the packaging source for '{base}' could not be fetched from {url}")
    };
    format!("{why} (git clone said: {git})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_yours_then_the_setting_then_the_verdict() {
        use Confidence::*;
        use Verdict::*;
        let v = |v, c| Some((v, c));
        // --source and --binary win over everything
        assert!(build_from_source(
            SourceMode::Source,
            Prefer::Repository,
            None
        ));
        assert!(!build_from_source(
            SourceMode::Binary,
            Prefer::Recommend,
            v(Source, High)
        ));
        // by default the repository's binary, whatever recommend would say
        assert!(!build_from_source(
            SourceMode::Auto,
            Prefer::Repository,
            v(Source, High)
        ));
        // with prefer = recommend: source only when it says source, with some confidence
        assert!(build_from_source(
            SourceMode::Auto,
            Prefer::Recommend,
            v(Source, High)
        ));
        assert!(build_from_source(
            SourceMode::Auto,
            Prefer::Recommend,
            v(Source, Medium)
        ));
        assert!(!build_from_source(
            SourceMode::Auto,
            Prefer::Recommend,
            v(Source, Low)
        ));
        assert!(!build_from_source(
            SourceMode::Auto,
            Prefer::Recommend,
            v(Either, High)
        ));
        assert!(!build_from_source(
            SourceMode::Auto,
            Prefer::Recommend,
            v(Binary, High)
        ));
        assert!(
            !build_from_source(SourceMode::Auto, Prefer::Recommend, v(SourceOnly, High)),
            "no binary: nothing to choose"
        );
        assert!(!build_from_source(
            SourceMode::Auto,
            Prefer::Recommend,
            None
        ));
    }

    #[test]
    fn a_plan_entry_says_whether_it_is_built_here() {
        assert!(
            is_source_repo("extra+source") && !is_source_repo("extra") && !is_source_repo("aur")
        );
        assert_eq!(base_repo("extra+source"), "extra");
        assert_eq!(base_repo("core"), "core");
    }

    #[test]
    fn a_built_file_names_its_package() {
        assert_eq!(package_of_file("sl-5.05-6-x86_64.pkg.tar.zst"), Some("sl"));
        assert_eq!(
            package_of_file("python-zope.interface-7.0-1-any.pkg.tar.xz"),
            Some("python-zope.interface")
        );
        assert_eq!(
            package_of_file("lib32-gtk+3-3.24.43-1-x86_64.pkg.tar.zst"),
            Some("lib32-gtk+3")
        );
        assert_eq!(
            package_of_file("openssl-docs-3.3.0-1-x86_64.pkg.tar.zst"),
            Some("openssl-docs")
        );
        assert_eq!(package_of_file("notes.txt"), None);
        assert_eq!(package_of_file("-1-1-any.pkg.tar.zst"), None);
    }

    #[test]
    fn tags_in_arch_packaging_have_a_dash_for_the_epoch() {
        assert_eq!(tag_candidates("5.05-6"), ["5.05-6"]);
        assert_eq!(tag_candidates("1:2.0-1"), ["1-2.0-1", "1:2.0-1"]);
    }

    #[test]
    fn only_a_package_base_name_goes_into_an_address() {
        for ok in ["sl", "python-zope.interface", "gtk+3", "a@b"] {
            assert!(valid_base(ok), "{ok}");
        }
        for bad in ["", "-x", ".hidden", "../x", "a/b", "a b", "a?b#c"] {
            assert!(!valid_base(bad), "{bad}");
        }
    }

    #[test]
    fn a_failed_fetch_says_what_it_means() {
        let url = "https://gitlab.archlinux.org/archlinux/packaging/packages/tree.git";
        let missing = explain_fetch_failure(
            "fatal: could not read Username for 'https://gitlab.archlinux.org': terminal prompts disabled",
            "tree",
            "2.3.2-1",
            url,
        );
        assert!(
            missing.contains("no public packaging repository for 'tree'"),
            "{missing}"
        );
        assert!(missing.contains("git clone said"));
        let tag = explain_fetch_failure(
            "warning: Could not find remote branch 2.3.2-1 to clone.\nfatal: Remote branch 2.3.2-1 not found in upstream origin",
            "tree",
            "2.3.2-1",
            url,
        );
        assert!(tag.contains("has no tag for 2.3.2-1"), "{tag}");
        let net = explain_fetch_failure(
            "fatal: unable to access 'x': Could not resolve host: gitlab",
            "tree",
            "1-1",
            url,
        );
        assert!(net.contains("could not be reached"), "{net}");
    }
}
