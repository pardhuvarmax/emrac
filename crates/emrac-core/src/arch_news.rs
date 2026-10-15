//! The Arch Linux news gate (u20, part 2): the news feed, which items are
//! new to you, and which are *critical*: a breaking change, a request for
//! manual intervention, or a change that touches a critical part of the
//! system such as the kernel. Everything here is a pure function over text
//! except `fetch` and the files. See `docs/plan/r2/u20-trust.md`.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const FEED: &str = "https://archlinux.org/feeds/news/";
const MAX_FEED: usize = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
/// A cached feed older than this is labelled stale.
pub const STALE_HOURS: u64 = 48;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub link: String,
    /// Unix seconds.
    pub published: u64,
    /// The text of the announcement, without markup.
    pub text: String,
}

// ---------------------------------------------------------------- RSS

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => ent
                .strip_prefix('#')
                .and_then(|n| {
                    n.strip_prefix('x')
                        .map_or_else(|| n.parse().ok(), |h| u32::from_str_radix(h, 16).ok())
                })
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text inside the first `<tag>…</tag>` of `xml`, CDATA unwrapped.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = xml.find(&format!("<{name}>"))? + name.len() + 2;
    let close = xml[open..].find(&format!("</{name}>"))? + open;
    let inner = xml[open..close].trim();
    Some(
        inner
            .strip_prefix("<![CDATA[")
            .and_then(|i| i.strip_suffix("]]>"))
            .unwrap_or(inner),
    )
}

/// Removes tags and control characters; keeps the words. The feed carries
/// its HTML entity-escaped (`&lt;p&gt;`), so entities are decoded first, the
/// tags removed, and the entities of the text decoded.
pub fn strip_markup(html: &str) -> String {
    let html = decode_entities(html);
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if in_tag => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    let text = decode_entities(&out);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `Tue, 14 Oct 2026 12:00:00 +0000` as unix seconds.
pub fn parse_rfc2822(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = s.split_once(", ").map_or(s, |(_, rest)| rest);
    let mut f = s.split_whitespace();
    let day: i64 = f.next()?.parse().ok()?;
    let month = match f.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = f.next()?.parse().ok()?;
    let mut t = f.next()?.split(':');
    let (h, m, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next().unwrap_or("0").parse().ok()?,
    );
    let zone = f.next().unwrap_or("+0000");
    let offset = if zone.len() == 5 && (zone.starts_with('+') || zone.starts_with('-')) {
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        sign * (zone[1..3].parse::<i64>().ok()? * 3600 + zone[3..5].parse::<i64>().ok()? * 60)
    } else {
        0
    };
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + (yoe * 365 + yoe / 4 - yoe / 100 + doy) - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + m * 60 + sec - offset).ok()
}

/// The items of an RSS feed. Items that don't parse are left out.
pub fn parse_feed(xml: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<item>") {
        let after = &rest[start + 6..];
        let Some(end) = after.find("</item>") else {
            break;
        };
        let body = &after[..end];
        rest = &after[end + 7..];
        let (Some(title), Some(link), Some(date)) =
            (tag(body, "title"), tag(body, "link"), tag(body, "pubDate"))
        else {
            continue;
        };
        let Some(published) = parse_rfc2822(date) else {
            continue;
        };
        let text = tag(body, "description")
            .map(strip_markup)
            .unwrap_or_default();
        items.push(Item {
            title: strip_markup(title),
            link: link.trim().to_string(),
            published,
            text,
        });
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.published));
    items
}

// -------------------------------------------------------- criticality

/// Why an item counts as critical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Reason {
    Breaking,
    ManualIntervention,
    Kernel,
    Bootloader,
    Toolchain,
    Init,
    Pacman,
    Crypto,
    Drivers,
    Filesystem,
}

impl Reason {
    pub fn phrase(self) -> &'static str {
        match self {
            Reason::Breaking => "announces a breaking change",
            Reason::ManualIntervention => "asks for manual intervention",
            Reason::Kernel => "touches the kernel or initramfs",
            Reason::Bootloader => "touches the bootloader",
            Reason::Toolchain => "touches the C library or toolchain",
            Reason::Init => "touches systemd or boot",
            Reason::Pacman => "touches pacman or its keyring",
            Reason::Crypto => "touches OpenSSL or certificates",
            Reason::Drivers => "touches graphics drivers or firmware",
            Reason::Filesystem => "changes the filesystem layout",
        }
    }
}

const RULES: &[(Reason, &[&str])] = &[
    (
        Reason::ManualIntervention,
        &[
            "manual intervention",
            "action required",
            "requires you to",
            "you must",
            "needs to be done manually",
            "before upgrading",
            "before you upgrade",
            "prior to upgrading",
        ],
    ),
    (
        Reason::Breaking,
        &[
            "breaking change",
            "breaking changes",
            "will fail",
            "will break",
            "breaks ",
            "incompatible",
            "no longer work",
            "will no longer",
            "has been removed",
            "dropped support",
        ],
    ),
    (
        Reason::Kernel,
        &[
            "kernel",
            "initramfs",
            "mkinitcpio",
            "dkms",
            "linux-lts",
            "linux-zen",
            "linux-hardened",
            "kernel module",
        ],
    ),
    (
        Reason::Bootloader,
        &[
            "bootloader",
            "grub",
            "systemd-boot",
            "limine",
            "refind",
            "secure boot",
            "efi system partition",
            "/boot",
        ],
    ),
    (
        Reason::Toolchain,
        &["glibc", "libc", "gcc", "libstdc++", "binutils", "toolchain"],
    ),
    (Reason::Init, &["systemd", "init system", "udev"]),
    (
        Reason::Pacman,
        &[
            "pacman",
            "archlinux-keyring",
            "keyring",
            "signature",
            "libalpm",
        ],
    ),
    (
        Reason::Crypto,
        &["openssl", "ca-certificates", "gnutls", "nss"],
    ),
    (
        Reason::Drivers,
        &[
            "nvidia",
            "mesa",
            "amdgpu",
            "linux-firmware",
            "graphics driver",
            "xorg-server",
            "wayland",
        ],
    ),
    (
        Reason::Filesystem,
        &[
            "usrmerge",
            "/usr/lib",
            "/lib ",
            "filesystem package",
            "merged /usr",
            "moved to /usr",
        ],
    ),
];

/// Whole-word-ish match so `nss` doesn't hit `business` and `libc` not `libcap`.
fn mentions(haystack: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(at) = haystack[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        let before = haystack[..start].chars().next_back();
        let after = haystack[end..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
        let needs_start = needle
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        let needs_end = needle
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        if (!needs_start || !word(before)) && (!needs_end || !word(after)) {
            return true;
        }
        from = end;
    }
    false
}

/// What makes an item critical (empty: it is only news). A breaking change
/// or a request for manual intervention is looked for in the whole text; a
/// critical part of the system only in the title, since the body of a notice
/// often just uses `pacman` or a path in its instructions.
pub fn reasons(item: &Item) -> Vec<Reason> {
    let title = item.title.to_lowercase();
    let all = format!("{} {}", item.title, item.text).to_lowercase();
    let mut out: Vec<Reason> = RULES
        .iter()
        .filter(|(reason, words)| {
            let haystack = if matches!(reason, Reason::Breaking | Reason::ManualIntervention) {
                &all
            } else {
                &title
            };
            words.iter().any(|w| mentions(haystack, w))
        })
        .map(|(reason, _)| *reason)
        .collect();
    out.sort();
    out
}

pub fn is_critical(item: &Item) -> bool {
    !reasons(item).is_empty()
}

// ----------------------------------------------------- seen and cached

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// News published up to this time has been seen (unix seconds).
    #[serde(default)]
    pub seen_until: u64,
    /// When the feed was last fetched.
    #[serde(default)]
    pub fetched_at: u64,
    #[serde(default)]
    pub items: Vec<Item>,
}

pub fn path() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| Error::NoDataDir)?;
    Ok(PathBuf::from(home).join(".local/share/emrac/news.json"))
}

pub fn load() -> State {
    path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(state: &State) -> Result<()> {
    let p = path()?;
    let io = |e: std::io::Error| Error::Io {
        context: format!("couldn't write {}", p.display()),
        source: e,
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = p.with_extension("json.part");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(state).expect("serializable"),
    )
    .and_then(|()| std::fs::rename(&tmp, &p))
    .map_err(io)
}

/// Where "seen" starts when you have never acknowledged: the time of your
/// last upgrade (news since then is news to you), else two weeks ago.
pub fn default_seen(last_upgrade: Option<u64>, now: u64) -> u64 {
    last_upgrade.unwrap_or_else(|| now.saturating_sub(14 * 86_400))
}

/// `seen_until` as stored, or the default when nothing was ever acknowledged.
pub fn effective_seen(state: &State, last_upgrade: Option<u64>, now: u64) -> u64 {
    if state.seen_until > 0 {
        state.seen_until
    } else {
        default_seen(last_upgrade, now)
    }
}

/// Items you have not seen: published after `seen_until`.
pub fn unread(items: &[Item], seen_until: u64) -> Vec<&Item> {
    items.iter().filter(|i| i.published > seen_until).collect()
}

/// How old the cache is, and whether that is stale.
pub fn age_label(fetched_at: u64, now: u64) -> (String, bool) {
    if fetched_at == 0 {
        return ("never fetched".to_string(), true);
    }
    let hours = now.saturating_sub(fetched_at) / 3600;
    let label = match hours {
        0 => "fetched just now".to_string(),
        h if h < 48 => format!("fetched {h} hour{} ago", if h == 1 { "" } else { "s" }),
        h => format!("fetched {} days ago", h / 24),
    };
    if hours > STALE_HOURS {
        (format!("{label}, stale"), true)
    } else {
        (label, false)
    }
}

// --------------------------------------------------------------- fetch

/// Fetches and parses the feed at `url` (`https://` or `file://`).
pub fn fetch(url: &str) -> Result<Vec<Item>> {
    let body = if let Some(path) = url.strip_prefix("file://") {
        std::fs::read_to_string(path)
            .map_err(|e| Error::News(format!("couldn't read {path}: {e}")))?
    } else {
        let mut response = ureq::get(url)
            .header("User-Agent", concat!("emrac/", env!("CARGO_PKG_VERSION")))
            .config()
            .timeout_global(Some(TIMEOUT))
            .build()
            .call()
            .map_err(|e| Error::News(format!("couldn't fetch {url}: {e}")))?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_FEED as u64)
            .read_to_vec()
            .map_err(|e| Error::News(format!("couldn't read {url}: {e}")))?;
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let items = parse_feed(&body);
    if items.is_empty() {
        return Err(Error::News(format!("{url} has no news items in it")));
    }
    Ok(items)
}

/// The feed address: `EMRAC_NEWS_FEED` (for tests) or the real one.
pub fn feed_url() -> String {
    std::env::var("EMRAC_NEWS_FEED").unwrap_or_else(|_| FEED.to_string())
}

/// Fetches the feed and stores it. Falls back to the cache, saying why.
pub struct Loaded {
    pub items: Vec<Item>,
    pub state: State,
    /// Why the feed could not be fetched (the items are then the cached ones).
    pub problem: Option<String>,
}

pub fn load_fresh(offline: bool, now: u64) -> Loaded {
    let mut state = load();
    if offline {
        return Loaded {
            items: state.items.clone(),
            state,
            problem: Some("--offline is set".into()),
        };
    }
    match fetch(&feed_url()) {
        Ok(items) => {
            state.items = items.clone();
            state.fetched_at = now;
            let _ = save(&state);
            Loaded {
                items,
                state,
                problem: None,
            }
        }
        Err(e) => Loaded {
            items: state.items.clone(),
            state,
            problem: Some(e.to_string()),
        },
    }
}

/// Marks everything published so far as seen.
pub fn acknowledge(items: &[Item]) -> Result<u64> {
    let mut state = load();
    let newest = items
        .iter()
        .map(|i| i.published)
        .max()
        .unwrap_or(state.seen_until);
    state.seen_until = state.seen_until.max(newest);
    if state.items.is_empty() {
        state.items = items.to_vec();
    }
    save(&state)?;
    Ok(state.seen_until)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED_XML: &str = r#"<?xml version="1.0"?><rss><channel>
<item><title>Kernel 7.0 and initramfs changes</title><link>https://archlinux.org/news/kernel-7/</link>
<pubDate>Tue, 14 Oct 2026 12:00:00 +0000</pubDate>
<description><![CDATA[<p>With &lt;b&gt;linux 7.0&lt;/b&gt; the initramfs must be regenerated.</p>]]></description></item>
<item><title>Manual intervention required for foo &amp; bar</title><link>https://archlinux.org/news/foo/</link>
<pubDate>Mon, 13 Oct 2026 08:30:00 +0530</pubDate>
<description>Before upgrading, remove foo-compat by hand.</description></item>
<item><title>New mirror in Iceland</title><link>https://archlinux.org/news/mirror/</link>
<pubDate>Sun, 12 Oct 2026 00:00:00 +0000</pubDate>
<description>A new mirror is available.</description></item>
<item><title>Broken item</title><link>x</link></item>
</channel></rss>"#;

    fn item(title: &str, text: &str, published: u64) -> Item {
        Item {
            title: title.into(),
            link: "l".into(),
            published,
            text: text.into(),
        }
    }

    #[test]
    fn the_feed_is_parsed_newest_first_with_markup_and_entities_removed() {
        let items = parse_feed(FEED_XML);
        assert_eq!(items.len(), 3, "the broken item is left out");
        assert_eq!(items[0].title, "Kernel 7.0 and initramfs changes");
        assert!(
            items[0].text.contains("linux 7.0") && !items[0].text.contains('<'),
            "{}",
            items[0].text
        );
        assert_eq!(items[1].title, "Manual intervention required for foo & bar");
        assert!(items[0].published > items[1].published && items[1].published > items[2].published);
        assert_eq!(parse_rfc2822("Thu, 01 Jan 1970 00:00:00 +0000"), Some(0));
        assert_eq!(parse_rfc2822("Thu, 01 Jan 1970 05:30:00 +0530"), Some(0));
        assert_eq!(
            parse_rfc2822("Thu, 15 Oct 2026 00:00:00 +0000"),
            Some(1_792_022_400)
        );
        assert!(parse_rfc2822("someday").is_none());
        assert_eq!(strip_markup("a&nbsp;<b>b</b>&amp;c&#65;"), "a b &cA");
    }

    #[test]
    fn what_makes_an_item_critical_is_risk_not_a_package_name() {
        let items = parse_feed(FEED_XML);
        let r0 = reasons(&items[0]);
        assert!(r0.contains(&Reason::Kernel), "{r0:?}");
        let r1 = reasons(&items[1]);
        assert!(r1.contains(&Reason::ManualIntervention), "{r1:?}");
        assert!(reasons(&items[2]).is_empty(), "a new mirror is only news");
        assert!(is_critical(&items[0]) && !is_critical(&items[2]));
        for (title, text, want) in [
            (
                "GRUB 3 moves its config",
                "update grub.cfg",
                Reason::Bootloader,
            ),
            ("glibc 2.50 requires a rebuild", "", Reason::Toolchain),
            ("Nvidia 600 drops older cards", "", Reason::Drivers),
            ("pacman 8 changes the keyring", "", Reason::Pacman),
            ("systemd 270 renames units", "", Reason::Init),
            ("OpenSSL 4 is incompatible", "", Reason::Crypto),
            ("usrmerge finishes", "", Reason::Filesystem),
            (
                "Plugin API change",
                "this is a breaking change for plugins",
                Reason::Breaking,
            ),
        ] {
            assert!(reasons(&item(title, text, 1)).contains(&want), "{title}");
        }
        assert!(
            reasons(&item(
                "Business meeting notes",
                "libcap is fine, a nssbox too",
                1
            ))
            .is_empty(),
            "no partial-word hits"
        );
        assert_eq!(Reason::Kernel.phrase(), "touches the kernel or initramfs");
    }

    #[test]
    fn a_critical_part_of_the_system_counts_from_the_title_only() {
        let body_only = item(
            "Update for foo",
            "run pacman -Syu and then edit /usr/lib/foo/conf",
            1,
        );
        assert!(reasons(&body_only).is_empty(), "{:?}", reasons(&body_only));
        let titled = item("pacman 8 ships a new keyring", "details", 1);
        assert!(reasons(&titled).contains(&Reason::Pacman));
        let plural = item("Breaking changes for all users of varnish", "", 1);
        assert!(reasons(&plural).contains(&Reason::Breaking));
    }

    #[test]
    fn nothing_acknowledged_yet_means_news_since_your_last_upgrade() {
        let fresh = State::default();
        assert_eq!(effective_seen(&fresh, Some(5_000), 9_000_000), 5_000);
        assert_eq!(
            effective_seen(&fresh, None, 20 * 86_400),
            6 * 86_400,
            "two weeks back"
        );
        let acked = State {
            seen_until: 77,
            ..State::default()
        };
        assert_eq!(effective_seen(&acked, Some(5_000), 9_000_000), 77);
    }

    #[test]
    fn unread_items_are_the_ones_after_the_last_acknowledgement() {
        let items = parse_feed(FEED_XML);
        assert_eq!(unread(&items, 0).len(), 3);
        let second = items[1].published;
        let u = unread(&items, second);
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].title, items[0].title);
        assert!(unread(&items, items[0].published).is_empty());
    }

    #[test]
    fn the_cache_age_is_worded_and_stale_after_two_days() {
        assert_eq!(age_label(0, 100), ("never fetched".to_string(), true));
        assert_eq!(age_label(100, 100).0, "fetched just now");
        assert_eq!(age_label(1, 3 * 3600).0, "fetched 2 hours ago");
        let (text, stale) = age_label(1, 5 * 86_400 + 1);
        assert!(
            stale && text.contains("5 days") && text.contains("stale"),
            "{text}"
        );
        assert!(!age_label(1, 47 * 3600).1);
    }

    #[test]
    fn a_feed_file_is_fetched_and_an_empty_or_missing_one_is_an_error() {
        let d = std::env::temp_dir().join(format!("emrac-news-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("feed.xml");
        std::fs::write(&f, FEED_XML).unwrap();
        assert_eq!(fetch(&format!("file://{}", f.display())).unwrap().len(), 3);
        std::fs::write(&f, "<rss></rss>").unwrap();
        assert!(fetch(&format!("file://{}", f.display()))
            .unwrap_err()
            .to_string()
            .contains("no news items"));
        assert!(fetch("file:///nonexistent/feed.xml").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
