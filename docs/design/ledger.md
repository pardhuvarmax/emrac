# The history, the ledger and the watch

How emrac keeps a record of what it did, and how far that record can be trusted. Written
for people who use emrac; the build notes are in `docs/plan/u7-history-integrity.md`,
`u8-ledger.md` and `u9-ledger-watch.md`.

## Three layers, each with a limit

| Layer | What it gives you | What defeats it |
|---|---|---|
| **Hash chain** in `~/.local/share/emrac/history.jsonl` (`history verify`) | An edited, removed, added or reordered line breaks the chain at that line. | Anyone who can rewrite the whole file recomputes the chain. A line cut off the *end* is invisible to the chain. |
| **Ledger**, a root-owned append-only copy at `/var/lib/emrac/ledger.jsonl` | You, a script or malware without root cannot edit or delete it; `history verify` compares the two copies and `--repair` rebuilds yours. | Root can remove the append-only flag and rewrite the file. |
| **Watch**, kernel audit rules plus emrac's journal line | A write, rename, delete or permission change of the ledger, or a run of `chattr`, by anything but emrac is recorded with program, user, login user and time, and `doctor` and `ledger events` report it. | Root can stop `auditd`, clear the rules or delete the log afterwards. Reporting, not prevention. |

Beyond those, an **anchor** is a copy of the chain's head that does not live on the machine.
It is the one thing a determined attacker on the machine cannot fix afterwards.

## Anchors

    $ emrac history anchor
    1042:3f9a1c0e5b7d24c8a6e1f0b93d5a7c42
    $ emrac history verify --against 1042:3f9a1c0e5b7d24c8a6e1f0b93d5a7c42

Keep the code somewhere else: a note, a message to yourself, a repository. Later,
`--against` recomputes the chain up to line 1042 and says whether that line still hashes to
the same value. It says no if anything up to there was rewritten (even by someone who
rebuilt the chain), or if the history now ends before line 1042 (a tail was cut off). Take
one now and then; the older the anchor, the less it covers, and an archive does not
invalidate it.

## The watch

Needs the `audit` package, `auditd` running, and ideally `audit=1` on the kernel command line
(so programs started before the rules load are covered).

    sudo emrac ledger watch install      # writes /etc/audit/rules.d/50-emrac-ledger.rules and loads it
    emrac ledger events                  # what was seen, 30 days back

The rules (`-w /var/lib/emrac/ -p wa -k emrac-ledger` and `-w /usr/bin/chattr -p x -k emrac-ledger-attr`)
log; they do not block. Each time emrac's helper appends it writes one line to the journal
(`SYSLOG_IDENTIFIER=emrac-ledger`); an event by emrac's own program with such a line close
in time is its own. Another program, or emrac's name without the line, is reported.
Reading the events needs the `systemd-journal` (or `adm`) group, or `sudo emrac ledger events`.

`doctor` says which step is missing: no rules, events unreadable, or "not active" (emrac
appended but the audit system saw nothing).

### Raising the bar (yours to decide, emrac installs none of it)

- `-e 2` as the last audit rule makes the rules unchangeable until reboot.
- Journal sealing (`journalctl --setup-keys`), with the verification key kept off the machine.
- Forwarding the journal or the audit events to another host.
- Keeping anchors off the machine (above).

## Trying the watch by hand

The audit rules cannot be loaded in a container, so this is the procedure for a real
machine with the `audit` package. Expected: each step below shows up in `emrac ledger events`
and `emrac doctor` as a foreign event naming the program and users; emrac's own appends do
not.

1. `sudo emrac ledger watch install` and `sudo systemctl enable --now auditd`.
2. Run any transaction (`emrac install <something small>`); `emrac ledger events` lists the
   append as `emrac`. `emrac doctor`: "Ledger watch" ok.
3. As root: `echo x >> /var/lib/emrac/ledger.jsonl`, `chattr -a /var/lib/emrac/ledger.jsonl`,
   `chattr +a …`, `cp /var/lib/emrac/ledger.jsonl /tmp/l && mv /tmp/l /tmp/m`. Each (the
   append is refused while the flag is set; the chattr runs are logged) should appear as
   `FOREIGN`.
4. `sudo augenrules --load` after deleting the rules file: `doctor` should turn to
   "not active" after the next emrac append.
