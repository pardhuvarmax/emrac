# Slice u20 — resource limits for sandboxed builds

The sandbox (u19) controls what a build can *see*. It doesn't stop a runaway or
hostile build from using all the RAM, forking without end, filling a tmpfs, or
running forever. §3.7 of `docs/design/source-build.md`.

## What can be enforced where (checked on this machine)

| Limit | Mechanism | Needs |
|---|---|---|
| memory (and no swap) | cgroup `MemoryMax`/`MemorySwapMax` | `systemd-run --user --scope`, cgroup v2 with `memory` delegated |
| process count (fork bombs) | cgroup `TasksMax` | same, `pids` |
| `/tmp` and `$HOME` size | `bwrap --size N --tmpfs …` | bubblewrap only |
| wall-clock time | emrac kills the sandbox after N minutes | nothing |

`bwrap` has no cgroup support, so memory and tasks go through
`systemd-run --user --scope` wrapped around it. Proven by hand: a 600 MB
allocation inside a 300 MB scope is killed (exit 137); a task cap stops the
forks. CPU is deliberately not capped — a build should be fast.

## Design

- **Defaults**: 120 minutes, memory = 75 % of total RAM (swap 0), 4096 tasks,
  8 GiB for each tmpfs. A value of `0` turns that limit off.
- **Config** (`[build]`, all numeric, `emrac config get/set/unset`):
  `build.timeout_minutes`, `build.memory_percent` (0–100), `build.max_tasks`,
  `build.tmpfs_gib`. Unlike `--no-sandbox` these are safe to keep in config: they
  only restrict a build.
- **Degrade honestly**: if the user manager can't give a scope (no systemd user
  session, cgroup v1, a container), emrac says once that memory and process
  limits are unavailable and continues with the timeout and tmpfs caps, which
  need nothing. It never silently pretends to enforce them.
- **Applies to** the two sandboxed steps. Step 1 (dependencies, sources) needs
  `sudo` and the network and stays outside; `--no-sandbox` builds have no
  limits (that is what the flag means).
- **When a limit fires** the error says which and how to change it: a timeout
  names `build.timeout_minutes`; a kill (SIGKILL / 137) names the memory limit
  and `build.memory_percent`.

## Verification

Pure unit tests for the `systemd-run` argument builder, the `--size` placement
and the timeout runner (a real `sleep` killed on time); config tests for the
new keys; a real build that allocates past its memory limit and one that is cut
off by the timeout; the degrade path with the probe forced off.

## Files

`sandbox.rs` (limits, wrapper, timeout runner), `backend/aur_build.rs`,
`config.rs`, CLI `aur_install.rs`/`main.rs`, `error.rs` (none new expected).
