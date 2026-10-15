# Slice u13 — `emrac pin` / `unpin` (holds with reasons and expiry)

## Context

`SPEC.md` lists `pin <pkg>` / `unpin <pkg>`; `ideas.md` (section 3, "Holds with
reasons and expiry") adds that a hold should carry a note and an optional
expiry so it doesn't silently outlive its purpose. This slice builds both on
top of `emrac upgrade`. (`lock`/`unlock` — a stronger "never touch" — is
separate and not built.)

## Design

- `emrac pin <pkg>... [--reason TEXT] [--until YYYY-MM-DD]`: holds installed
  packages at their current version. Not installed → error. Re-pinning
  replaces the old note/expiry. `--until` is the day the hold ends: it is in
  force until 00:00 UTC of that date.
- `emrac pin` with no packages lists pins (state: active, expired); `--json`.
- `emrac unpin <pkg>...`: removes pins; a package that isn't pinned is an
  error naming it (nothing is removed in that case).
- Storage: `~/.local/share/emrac/pins.json` (array of `{name, reason?, until?,
  created}`), path-taking helpers for tests.
- Effect — `emrac upgrade` only:
  - full upgrade: active pins are passed to pacman as `--ignore a,b` (plan and
    execution), and AUR upgrades for them are dropped. The output says
    `emrac notes: holding back N pinned package(s): name (reason, until …)`
    for pins that actually had an update waiting.
  - `emrac upgrade <pinned-pkg>` is refused, naming the pin and `emrac unpin`.
  - an expired pin is not enforced; upgrade says so once so the user can
    re-pin or clean it up with `unpin`.
  Installing, downgrading, reinstalling, removing and restoring a snapshot are
  deliberate user actions and are not blocked.
- A pin that would break a dependency (a newer package needs the newer
  version) surfaces as pacman's own dependency error; emrac doesn't try to
  resolve it.

Out of scope: `lock`, pin-to-version-range, automatic reminders in a TUI,
`config` default expiry.

## Files

- `pins.rs` (new, core): `Pin`, `PinStore`, `active`, `expired`.
- `pacman.rs` — `--ignore` in `resolve_full_upgrade` / `execute_full_upgrade`.
- `sources.rs` — pins in `plan_full_upgrade`, `upgrade_official`, refusal in
  targeted upgrades; `pin`, `unpin`, `list_pins`.
- `error.rs` — `PinWrite`, `PackagePinned`, `NotPinned`.
- CLI: `commands/pin.rs`, output, `upgrade.rs` notes.
