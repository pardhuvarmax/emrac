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
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME");
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

    let out = home.run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
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
