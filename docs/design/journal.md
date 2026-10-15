# The transaction journal

What emrac does when a transaction is cut off, for people who use emrac.

**The problem.** emrac records a transaction after it ends. If the power goes, the terminal is
closed or the process is killed in the middle of `pacman -Syu`, nothing is recorded, and nobody
knows what state the system is in.

**What emrac does.** Before it hands a plan to pacman (or builds an AUR package) emrac writes
`~/.local/share/emrac/journal.json` (what was planned, when, and where pacman's log ended) and
keeps a lock on it. When the transaction ends and has been recorded, the file is removed.

**Next time.** If the file is there and nobody holds the lock, the run that wrote it is gone. The
next change you ask for (or `emrac history recover`) reads pacman's log from where it ended then and
compares the installed versions with the plan. Each planned package is *done*, *not reached*, or
*changed otherwise*. That goes into your history as a failed transaction whose error starts with
`interrupted:`, so it is in the hash chain and the ledger like any other. `emrac doctor` shows
"Interrupted transaction" until it is recorded. `emrac history ack tx-N` marks it as dealt with.

**Two emrac at once.** A second emrac that finds the lock held refuses to start a change: it says
another one is changing packages.

**What it does not do.**
- It does not resume or retry. The report says what is left; you decide.
- It cannot tell a half-written package from a good one. If pacman's log has a transaction with no
  end, the report says so and suggests `emrac verify` on the packages involved.
- If the disk itself was lost in the crash, the journal is lost with it.
- pacman run by hand during the same time shows up in the same log range.
