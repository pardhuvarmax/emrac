# Slice u9 — the ledger watches itself

u8 puts the history where only root can write it. Root can still change it, and the
usual way to do that leaves no trace. A file cannot log what is done to it, but the
kernel can: the audit system records who touched a path, with which program, when, and
the journal keeps it. This slice makes emrac set that watch up, read it back, and say so
when something other than emrac wrote to the ledger. It builds on u7 (the chain) and u8
(the ledger).

## What this does and does not give

It **records and reports**: any write, truncate, rename, delete or attribute change of the
ledger, and every use of `chattr`, with the program, the user and login user, and the time.
It does not prevent anything. A root attacker can stop the audit daemon, clear the rules or
delete the audit log; the section "Raising the bar" says what limits that, and the
off-machine anchor below is what catches a rewrite those cannot. The documentation says all
of this plainly.

## 1. The watch

`/usr/share/emrac/audit/50-emrac-ledger.rules` ships with the package:

```
-w /var/lib/emrac/ -p wa -k emrac-ledger
-w /usr/bin/chattr -p x  -k emrac-ledger-attr
```

The first logs any write, truncate, rename, delete, chmod or chown of the ledger or its
folder. The second logs every run of `chattr`, so clearing the append-only flag shows up
even though the flag change itself is an ioctl a path watch does not see.

`sudo emrac ledger watch install` copies the file into `/etc/audit/rules.d/`, runs
`augenrules --load`, and asks first (default no, `--yes` does not answer). The package does
not write into `/etc/audit/` itself: that folder belongs to the `audit` package, which is an
optional dependency and may not be installed. `doctor` says which step is missing.

Needs: kernel audit on (`audit=1` on the kernel command line, so processes started before
the rules load are covered) and the `audit` package for `auditctl` / `augenrules`.

## 2. emrac's own appends are witnessed too

The helper that appends to the ledger (u8) also writes one line to the journal as it does:
`SYSLOG_IDENTIFIER=emrac-ledger`, with the record's `seq`, id and hash. These are the
legitimate writes; the audit events are the full picture; the two should agree.

## 3. Reading it back

`emrac history verify` and `doctor` add a "Ledger watch" check. They read the audit events
(`journalctl -o json _TRANSPORT=audit`, readable by the `systemd-journal`, `adm` and `wheel`
groups; `sudo ausearch -k emrac-ledger` as the fallback) and sort each event by its program:

- **`/usr/bin/emrac`** with a matching journal line: fine.
- **Anything else** (`rm`, an editor, `sh`, `chattr`, a script): a warning, naming the
  program, the user, the login user and the time.
- **No events at all after an emrac append:** "the ledger watch is not active", with the
  command that installs it.

`emrac ledger events` lists them for a person to read.

## 4. Anchors (the part that catches a rewrite)

The watch and the journal live on the same machine as the attacker. An anchor is a copy of
the chain's head that does not.

- `emrac history anchor` prints the head as one short code (the sequence number and the
  first 32 hex characters of its hash) to keep somewhere else: a note, a message to yourself,
  a repository.
- `emrac history verify --against <code>` recomputes the chain at that sequence number and
  says whether it still gives the same hash: a rewrite of anything up to that point does not.
- **Optional, second step:** `history.anchor_tsa = "<url>"` in the config makes emrac
  submit the hash (never the records) to an RFC 3161 timestamp authority after a
  transaction and keep the signed answer; `verify --anchors` checks the answers and
  the chain. Off unless a URL is set, so nothing leaves the machine by default.

## Raising the bar (documented, not installed)

- **`-e 2`** as the last audit rule makes the rules unchangeable until reboot. It is a
  system-wide decision, so emrac only explains it.
- **Journal sealing** (Forward Secure Sealing, `journalctl --setup-keys`) makes entries from
  before a compromise unforgeable without a verification key kept off the machine.
- **Forwarding** audit events or the journal to another host.

## Not covered

Signed checkpoints with a hardware key (a later, optional slice); a SELinux or AppArmor
policy; reading the audit log without the journal or `ausearch`; tuning the rules for
other paths.

## Verification

Unit tests: parsing audit lines from captured fixtures, sorting events as emrac or foreign,
the three "Ledger watch" statuses, the shipped rules file's content, the anchor code and the
chain check against it (a rewritten line, a deleted tail, a legacy log). Containers cannot
load audit rules or run a journal, so the real behaviour is checked by hand on a machine
with the `audit` package: install the watch, then as root append with a shell, run
`chattr -a` and `+a`, rename and delete a copy, and confirm `emrac history verify` and
`doctor` name each one with its program and user, that emrac's own appends are not flagged,
and that deleting the watch's rules turns the check into "not active". The procedure and
its expected output are written into the slice's changelog entry when it is run.
