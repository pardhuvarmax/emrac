//! The look of `config.toml`: a banner, a framed header per section, and for
//! every key what it does, its default and what it accepts, with a flag on
//! the ones that differ from the default. The file is written from the values
//! in a `Config`, so `config set` and `config unset` keep it in this shape;
//! comments are not parsed back, only regenerated.

use crate::config::Config;

const WIDTH: usize = 72;

/// "EMRAC" in plain ASCII, five rows.
const LOGO: [&str; 5] = [
    r" _____   __  __   ____       _       ____ ",
    r"| ____| |  \/  | |  _ \     / \     / ___|",
    r"|  _|   | |\/| | | |_) |   / _ \   | |    ",
    r"| |___  | |  | | |  _ <   / ___ \  | |___ ",
    r"|_____| |_|  |_| |_| \_\ /_/   \_\  \____|",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Number,
    Text,
    /// A number that may be absent (TOML has no null: it is left commented out).
    OptNumber,
}

struct Key {
    name: &'static str,
    kind: Kind,
    /// One line: what it does.
    does: &'static str,
    /// What it accepts.
    accepts: &'static str,
    /// An example for an optional key that is currently unset.
    example: &'static str,
}

struct Section {
    name: &'static str,
    icon: &'static str,
    blurb: &'static str,
    keys: &'static [Key],
}

const fn key(name: &'static str, kind: Kind, does: &'static str, accepts: &'static str) -> Key {
    Key {
        name,
        kind,
        does,
        accepts,
        example: "",
    }
}

const BOOL: &str = "true | false";
const NUM: &str = "a whole number; 0 turns the limit off";

const SECTIONS: &[Section] = &[
    Section {
        name: "flags",
        icon: "⚑",
        blurb: "defaults for the global command-line switches",
        keys: &[
            key("offline", Kind::Bool, "never touch the network; the AUR is skipped", BOOL),
            key("yes", Kind::Bool, "answer yes to every confirmation (pacman gets --noconfirm); silence is never consent, so leave it off unless you script emrac", BOOL),
            key("dry_run", Kind::Bool, "show what would happen and change nothing, on every command", BOOL),
            key("json", Kind::Bool, "print machine-readable JSON instead of text", BOOL),
            key("quiet", Kind::Bool, "print only errors", BOOL),
        ],
    },
    Section {
        name: "review",
        icon: "◉",
        blurb: "what you are shown before an AUR package is built",
        keys: &[
            key("skip_pkgbuild", Kind::Bool, "do not show the PKGBUILD for review (the audit still runs)", BOOL),
            key("skip_diff", Kind::Bool, "do not show the diff of an AUR package that changed since you last built it", BOOL),
            key("inspect_official", Kind::Bool, "download official packages first and show what they would change before installing", BOOL),
        ],
    },
    Section {
        name: "search",
        icon: "⌕",
        blurb: "how `emrac search` behaves",
        keys: &[
            Key {
                name: "limit",
                kind: Kind::OptNumber,
                does: "show at most this many results; leave it out for no limit",
                accepts: "a whole number, or unset",
                example: "20",
            },
            key("official", Kind::Bool, "search only the official repositories", BOOL),
            key("aur", Kind::Bool, "search only the AUR", BOOL),
        ],
    },
    Section {
        name: "snapshot",
        icon: "❄",
        blurb: "restore points of the installed package set",
        keys: &[key(
            "before_upgrade",
            Kind::Bool,
            "save a restore point right before `emrac upgrade` changes anything (see `emrac snapshot list`)",
            BOOL,
        )],
    },
    Section {
        name: "build",
        icon: "⚒",
        blurb: "limits on sandboxed AUR builds; 0 turns a limit off",
        keys: &[
            key("timeout_minutes", Kind::Number, "stop a build after this many minutes", NUM),
            key("memory_percent", Kind::Number, "memory ceiling as a percentage of total RAM; swap is not allowed", "0 to 100; 0 turns the limit off"),
            key("max_tasks", Kind::Number, "most processes and threads a build may have", NUM),
            key("tmpfs_gib", Kind::Number, "size cap of the build's /tmp and $HOME, in GiB", NUM),
        ],
    },
    Section {
        name: "cache",
        icon: "⛁",
        blurb: "what `emrac cache clean` keeps",
        keys: &[
            key("policy", Kind::Text, "standard keeps the installed and the previous version of every package; lean keeps only what undoing recent changes needs and what cannot be downloaded again", "\"standard\" | \"lean\""),
            key("keep_days", Kind::Number, "with lean: everything done in the last this-many days stays undoable", "a whole number of days"),
        ],
    },
    Section {
        name: "safety",
        icon: "⛨",
        blurb: "the checks made before a transaction changes anything",
        keys: &[
            key(
                "preflight",
                Kind::Text,
                "block stops a transaction that would run out of space, leave no way to boot, or run the battery flat; warn shows the problem and goes on; off skips the checks",
                "\"block\" | \"warn\" | \"off\"",
            ),
            key(
                "partial_upgrade",
                Kind::Text,
                "what to do when installing or upgrading only some packages would leave other upgrades waiting, a partial upgrade that Arch does not support; --allow-partial lets one run through",
                "\"refuse\" | \"warn\" | \"off\"",
            ),
            key(
                "arch_news",
                Kind::Text,
                "the Arch news before an upgrade: gate warns about unread news and stops for a critical item (a breaking change, manual intervention, or a change touching the kernel, bootloader, C library, systemd, pacman, drivers); warn never stops; off does not look",
                "\"gate\" | \"warn\" | \"off\"",
            ),
            key(
                "db_backup",
                Kind::Text,
                "on keeps a small backup of pacman's local package database before each transaction (the newest 3, in ~/.local/share/emrac/pacman-db) and checks it before and after with pacman -Dk; off does neither",
                "\"on\" | \"off\"",
            ),
        ],
    },
    Section {
        name: "self",
        icon: "↻",
        blurb: "how emrac learns about its own releases (`emrac self`)",
        keys: &[
            key("source", Kind::Text, "where the release index and news come from", "an https:// address or a file:// path"),
            key("channel", Kind::Text, "which releases `emrac self upgrade` offers; prerelease also sees the stable ones", "\"stable\" | \"prerelease\""),
            key("require_signature", Kind::Bool, "refuse a release index without a valid gpg signature by signing_key", BOOL),
            key("signing_key", Kind::Text, "fingerprint of the gpg key that signs the release index", "a 16 to 64 digit hex fingerprint, or empty"),
        ],
    },
];

fn pad(text: &str, width: usize) -> String {
    let n = text.chars().count();
    format!("{text}{}", " ".repeat(width.saturating_sub(n)))
}

fn rule(left: char, fill: char, right: char) -> String {
    format!("# {left}{}{right}", fill.to_string().repeat(WIDTH - 2))
}

fn line(left: char, inner: &str, right: char) -> String {
    format!("# {left}{}{right}", pad(inner, WIDTH - 2))
}

/// `a` on the left, `b` flush right, inside a frame.
fn spread(left: char, a: &str, b: &str, right: char) -> String {
    let room = WIDTH - 2;
    let gap = room.saturating_sub(a.chars().count() + b.chars().count());
    line(left, &format!("{a}{}{b}", " ".repeat(gap)), right)
}

/// `text` wrapped at `width` columns after `first` on the first line and
/// `rest` on the others.
fn wrap(first: &str, rest: &str, text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut current = first.to_string();
    let mut fresh = true;
    for word in text.split_whitespace() {
        if !fresh && current.chars().count() + 1 + word.chars().count() > width {
            out.push_str(&current);
            out.push('\n');
            current = rest.to_string();
            fresh = true;
        }
        if !fresh {
            current.push(' ');
        }
        current.push_str(word);
        fresh = false;
    }
    out.push_str(&current);
    out.push('\n');
    out
}

fn toml_value(kind: Kind, raw: &str) -> String {
    match kind {
        Kind::Bool | Kind::Number | Kind::OptNumber => raw.to_string(),
        Kind::Text => toml::Value::String(raw.to_string()).to_string(),
    }
}

/// The whole file for `config`.
pub fn render(config: &Config) -> String {
    let defaults = Config::default();
    let mut out = String::new();

    out.push_str(&rule('╔', '═', '╗'));
    out.push('\n');
    out.push_str(&line('║', "", '║'));
    out.push('\n');
    for row in LOGO {
        out.push_str(&line('║', &format!("   {row}"), '║'));
        out.push('\n');
    }
    out.push_str(&line('║', "", '║'));
    out.push('\n');
    out.push_str(&line(
        '║',
        "   configuration  ·  a hybrid package manager for Arch Linux",
        '║',
    ));
    out.push('\n');
    out.push_str(&line('║', "", '║'));
    out.push('\n');
    out.push_str(&spread('║', "", "« Sic Mundus Creatus Est »   ", '║'));
    out.push('\n');
    out.push_str(&rule('╚', '═', '╝'));
    out.push_str(
        "\n#\n\
#   Change a value     emrac config set <key> <value>   e.g. build.memory_percent 50\n\
#   Back to default    emrac config unset <key>\n\
#   Read this file     emrac config show     Check it   emrac config validate\n\
#   Edit by hand       emrac config edit\n\
#\n\
#   ✦ Each value says what it does, its default, and what it accepts.\n\
#   ✦ A value that differs from its default is flagged at the end of its line.\n\
#   ✦ emrac rewrites this file when you use `config set` or `config unset`:\n\
#     your values are kept, the comments are regenerated. Keep notes elsewhere.\n\n",
    );

    for section in SECTIONS {
        out.push_str(&rule('┏', '━', '┓'));
        out.push('\n');
        out.push_str(&spread(
            '┃',
            &format!(" {}  {}", section.icon, section.name),
            &format!("{} ", section.blurb),
            '┃',
        ));
        out.push('\n');
        out.push_str(&rule('┗', '━', '┛'));
        out.push_str(&format!("\n[{}]\n", section.name));

        for key in section.keys {
            let full = format!("{}.{}", section.name, key.name);
            let now = config.get(&full).unwrap_or_default();
            let default = defaults.get(&full).unwrap_or_default();
            let shown_default = if key.kind == Kind::Text && default.is_empty() {
                "(empty)".to_string()
            } else {
                default.clone()
            };
            out.push('\n');
            out.push_str(&wrap(
                &format!("# ▸ {} — ", key.name),
                "#     ",
                key.does,
                WIDTH + 2,
            ));
            let facts = format!("default: {shown_default}     accepts: {}", key.accepts);
            if facts.chars().count() <= 76 {
                out.push_str(&format!("#     {facts}\n"));
            } else {
                out.push_str(&format!(
                    "#     default: {shown_default}\n#     accepts: {}\n",
                    key.accepts
                ));
            }
            if key.kind == Kind::OptNumber && now == "unset" {
                out.push_str(&format!("# {} = {}\n", key.name, key.example));
                continue;
            }
            let value = toml_value(key.kind, &now);
            let assignment = format!("{} = {value}", key.name);
            if now == default {
                out.push_str(&assignment);
            } else {
                let width = (assignment.chars().count() + 2).max(36);
                out.push_str(&format!(
                    "{}# ◂ changed (default: {shown_default})",
                    pad(&assignment, width)
                ));
            }
            out.push('\n');
        }
        out.push('\n');
    }

    out.push_str(
        "# ─────────────────────────── end of configuration ───────────────────────────\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reparsed(c: &Config) -> String {
        let back: Config = toml::from_str(&render(c)).expect("the styled file is valid TOML");
        toml::to_string_pretty(&back).unwrap()
    }

    fn plain(c: &Config) -> String {
        toml::to_string_pretty(c).unwrap()
    }

    #[test]
    fn the_default_file_reads_back_to_the_same_settings() {
        let c = Config::default();
        assert_eq!(reparsed(&c), plain(&c));
    }

    #[test]
    fn every_key_survives_a_change_including_awkward_text() {
        let mut c = Config::default();
        for (k, v) in [
            ("flags.offline", "true"),
            ("flags.yes", "true"),
            ("flags.dry_run", "true"),
            ("flags.json", "true"),
            ("flags.quiet", "true"),
            ("review.skip_pkgbuild", "true"),
            ("review.skip_diff", "true"),
            ("review.inspect_official", "true"),
            ("search.limit", "20"),
            ("search.official", "true"),
            ("search.aur", "true"),
            ("snapshot.before_upgrade", "true"),
            ("build.timeout_minutes", "30"),
            ("build.memory_percent", "50"),
            ("build.max_tasks", "100"),
            ("build.tmpfs_gib", "2"),
            ("cache.policy", "lean"),
            ("cache.keep_days", "3"),
            ("safety.preflight", "warn"),
            ("safety.partial_upgrade", "off"),
            ("safety.arch_news", "warn"),
            ("safety.db_backup", "off"),
            ("self.source", "file:///tmp/a \"b\"\\c/releases.json"),
            ("self.channel", "prerelease"),
            ("self.require_signature", "true"),
            ("self.signing_key", "ABCDEF0123456789ABCDEF"),
        ] {
            c.set(k, v).unwrap();
        }
        assert_eq!(reparsed(&c), plain(&c));
        let text = render(&c);
        assert_eq!(text.matches("◂ changed").count(), 26, "{text}");
    }

    #[test]
    fn the_banner_spells_emrac_in_plain_ascii_inside_the_box() {
        let text = render(&Config::default());
        for row in LOGO {
            assert!(row.is_ascii(), "{row:?}");
            assert!(
                row.chars().count() <= WIDTH - 2 - 3,
                "fits the box: {row:?}"
            );
            assert!(
                text.lines()
                    .any(|l| l.starts_with("# ║   ") && l.contains(row.trim_end())),
                "{row:?} is in the box"
            );
        }
        let widths: std::collections::HashSet<usize> =
            LOGO.iter().map(|r| r.chars().count()).collect();
        assert_eq!(widths.len(), 1, "every row of the logo is the same width");
    }

    #[test]
    fn rewriting_a_rendered_file_changes_nothing() {
        let mut c = Config::default();
        c.set("cache.policy", "lean").unwrap();
        let first = render(&c);
        let again: Config = toml::from_str(&first).unwrap();
        assert_eq!(render(&again), first);
    }

    #[test]
    fn an_unset_limit_is_shown_commented_and_a_set_one_is_a_value() {
        let mut c = Config::default();
        let text = render(&c);
        assert!(text.contains("\n# limit = 20\n"), "{text}");
        assert!(!text.contains("\nlimit ="));
        c.set("search.limit", "5").unwrap();
        let text = render(&c);
        assert!(text.contains("\nlimit = 5 "), "{text}");
        assert!(text.contains("◂ changed"));
    }

    #[test]
    fn the_file_is_framed_documented_and_not_wider_than_it_should_be() {
        let text = render(&Config::default());
        assert!(text.starts_with("# ╔"));
        assert!(text.contains("configuration  ·  a hybrid package manager"));
        assert!(text.contains("Sic Mundus Creatus Est"));
        for s in [
            "[flags]",
            "[review]",
            "[search]",
            "[snapshot]",
            "[build]",
            "[cache]",
            "[safety]",
            "[self]",
        ] {
            assert!(text.contains(&format!("\n{s}\n")), "{s}");
        }
        assert!(text.contains("# ▸ memory_percent — memory ceiling"));
        assert!(
            text.lines()
                .filter(|l| l.starts_with('#'))
                .all(|l| l.chars().count() <= 90),
            "no comment line runs on"
        );
        assert!(text.contains("# ▸ yes — answer yes to every confirmation"));
        assert!(text.contains("default: 75"));
        assert!(
            text.contains("default: (empty)"),
            "an empty default is said"
        );
        for l in text.lines().filter(|l| {
            l.starts_with("# ╔")
                || l.starts_with("# ┏")
                || l.starts_with("# ║")
                || l.starts_with("# ┃")
        }) {
            assert_eq!(l.chars().count(), WIDTH + 2, "{l:?}");
        }
        assert!(
            !text.contains("◂ changed"),
            "a default config flags nothing"
        );
    }
}
