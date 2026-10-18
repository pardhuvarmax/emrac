//! Which config files a plan will leave as `.pacnew` or `.pacsave`, worked out
//! before it runs.
//!
//! pacman keeps a package's `backup=()` files safe by comparing three
//! versions: the one the package shipped when it was installed (its recorded
//! md5), the one on disk now, and the one in the new package. Edits you made
//! are never overwritten; the new version is put next to them as `.pacnew`.
//! Today that is learned after the upgrade. The rules here were found by
//! running real upgrades and removals in a container
//! (`research/pacman-commit-order.md`); see `docs/plan/r3/u5-pacnew-preview.md`.

use serde::Serialize;

use crate::purge::Observed;

/// What will happen to one config file the user changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// The new version differs from both yours and the one you started from:
    /// it is written as `<file>.pacnew` and yours is kept.
    Pacnew,
    /// Yours differs from the original, so a `.pacnew` appears if the new
    /// package changed the file. Its content is not known yet.
    MaybePacnew,
    /// A removal keeps your edited file as `<file>.pacsave`.
    Pacsave,
    /// A removal keeps it as `.pacsave` if it was edited; it could not be read.
    MaybePacsave,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConfigEffect {
    pub package: String,
    /// Absolute path of the live file.
    pub path: String,
    pub effect: Effect,
    /// One line: why.
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ConfigPreview {
    pub effects: Vec<ConfigEffect>,
}

impl ConfigPreview {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

/// What an upgrade does to a config file. `original` is the md5 recorded when
/// the installed version was installed, `current` the file now, `new` the md5 of
/// the file in the new package (`None`: that package is not at hand),
/// `still_shipped` whether the new package contains the file at all (`None`:
/// not known), and `no_upgrade` whether `pacman.conf` lists it under `NoUpgrade`.
/// `None` back means nothing to tell: the file is replaced, or kept as it is,
/// without a `.pacnew`.
pub(crate) fn on_upgrade(
    original: &str,
    current: &Observed,
    new: Option<&str>,
    still_shipped: Option<bool>,
    no_upgrade: bool,
) -> Option<(Effect, &'static str)> {
    // The new package no longer has the file: it is removed like any file of
    // the old version, an edited one kept as `.pacsave`.
    if still_shipped == Some(false) {
        return on_removal(original, current).map(|(effect, _)| {
            (
                effect,
                "the new package no longer ships it, so your edited copy is kept",
            )
        });
    }
    let current_md5 = match current {
        // Nothing to protect: the new file is simply written.
        Observed::Missing => return None,
        Observed::Unreadable => {
            return Some((
                Effect::MaybePacnew,
                "the file could not be read, so it may have been edited",
            ))
        }
        Observed::Hash(md5) => md5,
    };
    if no_upgrade {
        // Listed under NoUpgrade: an existing file is never replaced and the
        // new version always lands beside it, even when they are identical.
        return Some((
            Effect::Pacnew,
            "it is listed under NoUpgrade in pacman.conf",
        ));
    }
    if current_md5 == original {
        // Untouched since it was installed: replaced without a word.
        return None;
    }
    match new {
        // The package did not change the file, or already has your version.
        Some(n) if n == original || n == current_md5 => None,
        Some(_) => Some((
            Effect::Pacnew,
            "you edited it and the new package changes it too",
        )),
        None => Some((
            Effect::MaybePacnew,
            "you edited it; the new package is not downloaded yet, so whether it changes the file is not known",
        )),
    }
}

/// What a removal does to a config file.
pub(crate) fn on_removal(original: &str, current: &Observed) -> Option<(Effect, &'static str)> {
    match current {
        Observed::Missing => None,
        Observed::Unreadable => Some((
            Effect::MaybePacsave,
            "the file could not be read, so it may have been edited",
        )),
        Observed::Hash(md5) if md5 == original => None,
        Observed::Hash(_) => Some((Effect::Pacsave, "you edited it, so removal keeps it")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Observed {
        Observed::Hash(s.to_string())
    }

    #[test]
    fn an_untouched_file_is_replaced_without_a_pacnew() {
        assert_eq!(on_upgrade("a", &h("a"), Some("b"), None, false), None);
        assert_eq!(on_upgrade("a", &h("a"), None, None, false), None);
    }

    #[test]
    fn an_edited_file_gets_a_pacnew_only_if_the_package_changes_it() {
        let (effect, _) = on_upgrade("a", &h("mine"), Some("b"), None, false).unwrap();
        assert_eq!(effect, Effect::Pacnew);
        // upstream did not change it: yours stays, nothing beside it
        assert_eq!(on_upgrade("a", &h("mine"), Some("a"), None, false), None);
        // the package already ships exactly what you have
        assert_eq!(on_upgrade("a", &h("mine"), Some("mine"), None, false), None);
    }

    #[test]
    fn without_the_new_package_an_edited_file_may_get_a_pacnew() {
        let (effect, why) = on_upgrade("a", &h("mine"), None, None, false).unwrap();
        assert_eq!(effect, Effect::MaybePacnew);
        assert!(why.contains("not downloaded"));
    }

    #[test]
    fn a_missing_file_is_written_and_an_unreadable_one_is_a_maybe() {
        assert_eq!(
            on_upgrade("a", &Observed::Missing, Some("b"), None, false),
            None
        );
        let (effect, _) = on_upgrade("a", &Observed::Unreadable, Some("b"), None, false).unwrap();
        assert_eq!(effect, Effect::MaybePacnew);
    }

    #[test]
    fn no_upgrade_always_leaves_a_pacnew_for_a_file_that_exists() {
        // Even untouched, even when the new file is identical (seen in pacman).
        for new in [Some("b"), Some("a"), None] {
            let (effect, why) = on_upgrade("a", &h("a"), new, None, true).unwrap();
            assert_eq!(effect, Effect::Pacnew);
            assert!(why.contains("NoUpgrade"));
        }
        // A file that is not there is simply written.
        assert_eq!(
            on_upgrade("a", &Observed::Missing, Some("b"), None, true),
            None
        );
    }

    #[test]
    fn a_file_the_new_package_drops_is_kept_as_pacsave_if_edited() {
        let dropped = Some(false);
        assert_eq!(
            on_upgrade("a", &h("mine"), None, dropped, false).unwrap().0,
            Effect::Pacsave
        );
        assert_eq!(on_upgrade("a", &h("a"), None, dropped, false), None);
        assert_eq!(
            on_upgrade("a", &h("mine"), None, Some(true), false)
                .unwrap()
                .0,
            Effect::MaybePacnew
        );
    }

    #[test]
    fn a_removal_keeps_only_edited_files_as_pacsave() {
        assert_eq!(on_removal("a", &h("a")), None);
        assert_eq!(on_removal("a", &Observed::Missing), None);
        assert_eq!(on_removal("a", &h("mine")).unwrap().0, Effect::Pacsave);
        assert_eq!(
            on_removal("a", &Observed::Unreadable).unwrap().0,
            Effect::MaybePacsave
        );
    }
}
