//! Package holds with an optional reason and expiry. Storage takes explicit
//! paths so tests never touch `$HOME`; the active/expired split is pure.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::history::{format_utc, now};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pin {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Unix seconds (UTC midnight of the chosen date): the hold is in force
    /// until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
    /// Unix seconds, UTC.
    pub created: u64,
}

impl Pin {
    /// Still holding the package back at `at`?
    pub fn is_active(&self, at: u64) -> bool {
        self.until.is_none_or(|until| at < until)
    }

    /// `YYYY-MM-DD` of the expiry, if any.
    pub fn until_date(&self) -> Option<String> {
        self.until.map(|t| {
            format_utc(t)
                .split(' ')
                .next()
                .unwrap_or_default()
                .to_string()
        })
    }

    /// `reason, until DATE` / `until DATE` / `reason` / empty.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(reason) = &self.reason {
            parts.push(reason.clone());
        }
        if let Some(date) = self.until_date() {
            parts.push(format!("until {date}"));
        }
        parts.join(", ")
    }
}

/// Splits pins into those in force and those that have lapsed.
pub fn partition(pins: Vec<Pin>, at: u64) -> (Vec<Pin>, Vec<Pin>) {
    pins.into_iter().partition(|p| p.is_active(at))
}

/// `~/.local/share/emrac/pins.json`.
pub struct PinStore;

impl PinStore {
    pub fn path() -> Result<PathBuf> {
        let home = env::var("HOME").map_err(|_| Error::NoDataDir)?;
        Ok(PathBuf::from(home).join(".local/share/emrac/pins.json"))
    }

    /// Sorted by name. A missing file is no pins.
    pub fn list() -> Result<Vec<Pin>> {
        Self::list_from(&Self::path()?)
    }

    /// Adds or replaces pins. Returns them as stored.
    pub fn pin(names: &[String], reason: Option<&str>, until: Option<u64>) -> Result<Vec<Pin>> {
        Self::pin_in(&Self::path()?, names, reason, until, now())
    }

    /// Removes pins; if any name isn't pinned nothing is removed.
    pub fn unpin(names: &[String]) -> Result<()> {
        Self::unpin_in(&Self::path()?, names)
    }

    fn list_from(path: &Path) -> Result<Vec<Pin>> {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(Error::Io {
                    context: format!("couldn't read your pins at {}", path.display()),
                    source,
                });
            }
        };
        let mut pins: Vec<Pin> = serde_json::from_str(&raw).map_err(|err| Error::PinsCorrupt {
            path: path.display().to_string(),
            why: err.to_string(),
        })?;
        pins.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(pins)
    }

    fn pin_in(
        path: &Path,
        names: &[String],
        reason: Option<&str>,
        until: Option<u64>,
        created: u64,
    ) -> Result<Vec<Pin>> {
        let mut pins = Self::list_from(path)?;
        let mut stored = Vec::new();
        for name in names {
            let pin = Pin {
                name: name.clone(),
                reason: reason.map(str::to_string),
                until,
                created,
            };
            pins.retain(|p| p.name != *name);
            pins.push(pin.clone());
            stored.push(pin);
        }
        Self::write(path, &pins)?;
        Ok(stored)
    }

    fn unpin_in(path: &Path, names: &[String]) -> Result<()> {
        let mut pins = Self::list_from(path)?;
        let missing: Vec<&str> = names
            .iter()
            .filter(|n| !pins.iter().any(|p| p.name == **n))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            return Err(Error::NotPinned(missing.join(", ")));
        }
        pins.retain(|p| !names.contains(&p.name));
        Self::write(path, &pins)
    }

    fn write(path: &Path, pins: &[Pin]) -> Result<()> {
        let write_err = |source| Error::PinWrite {
            path: path.display().to_string(),
            source,
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(write_err)?;
        }
        let json = serde_json::to_string_pretty(pins).expect("pins are serializable");
        // Write beside, then rename: a crash can't leave half a file.
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(write_err)?;
        fs::rename(&tmp, path).map_err(write_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("emrac-pins-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("pins.json")
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn activity_follows_the_expiry() {
        let mut pin = Pin {
            name: "x".into(),
            reason: None,
            until: None,
            created: 0,
        };
        assert!(pin.is_active(u64::MAX), "no expiry never lapses");
        pin.until = Some(1000);
        assert!(pin.is_active(999));
        assert!(
            !pin.is_active(1000),
            "in force until, not including, the date"
        );
    }

    #[test]
    fn describe_and_date_format() {
        let pin = Pin {
            name: "x".into(),
            reason: Some("breaks my driver".into()),
            until: Some(1_798_761_600), // 2027-01-01
            created: 0,
        };
        assert_eq!(pin.until_date().as_deref(), Some("2027-01-01"));
        assert_eq!(pin.describe(), "breaks my driver, until 2027-01-01");
        let bare = Pin {
            reason: None,
            until: None,
            ..pin
        };
        assert_eq!(bare.describe(), "");
    }

    #[test]
    fn partition_splits_active_from_lapsed() {
        let mk = |n: &str, until| Pin {
            name: n.into(),
            reason: None,
            until,
            created: 0,
        };
        let (active, lapsed) =
            partition(vec![mk("a", None), mk("b", Some(5)), mk("c", Some(50))], 10);
        let n = |v: &[Pin]| v.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        assert_eq!(n(&active), ["a", "c"]);
        assert_eq!(n(&lapsed), ["b"]);
    }

    #[test]
    fn pin_list_unpin_round_trip_and_replace() {
        let path = scratch("roundtrip");
        assert!(PinStore::list_from(&path).unwrap().is_empty());
        PinStore::pin_in(&path, &names(&["zed", "alpha"]), Some("why"), None, 7).unwrap();
        PinStore::pin_in(&path, &names(&["alpha"]), None, Some(99), 8).unwrap();

        let pins = PinStore::list_from(&path).unwrap();
        assert_eq!(pins.len(), 2);
        assert_eq!(pins[0].name, "alpha");
        assert_eq!(pins[0].reason, None, "re-pinning replaces the note");
        assert_eq!(pins[0].until, Some(99));
        assert_eq!(pins[1].reason.as_deref(), Some("why"));

        PinStore::unpin_in(&path, &names(&["zed"])).unwrap();
        assert_eq!(PinStore::list_from(&path).unwrap().len(), 1);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unpinning_an_unpinned_name_changes_nothing() {
        let path = scratch("missing");
        PinStore::pin_in(&path, &names(&["a"]), None, None, 1).unwrap();
        let err = PinStore::unpin_in(&path, &names(&["a", "b", "c"])).unwrap_err();
        assert!(matches!(&err, Error::NotPinned(n) if n == "b, c"), "{err}");
        assert_eq!(PinStore::list_from(&path).unwrap().len(), 1);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_silently_empty() {
        let path = scratch("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{nope").unwrap();
        assert!(matches!(
            PinStore::list_from(&path),
            Err(Error::PinsCorrupt { .. })
        ));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
