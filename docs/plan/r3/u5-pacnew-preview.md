# Slice u5 (after r3) — `.pacnew` and `.pacsave` preview

Today emrac tells you about `.pacnew` files after an upgrade (`emrac pacnew`, the post-upgrade report).
The upgrade plan can say which of **your edited** config files will get one, and removals which will
become `.pacsave`, so the choice to proceed is made knowing it.

Idea source: no package manager does this before the fact; dpkg asks about a conffile while it runs
(`Configuration file '/etc/x' ==> Modified (by you) or by a script ...`), and `etckeeper` records it
afterwards. The rule is pacman's own three-way comparison, which was written down only after running
every case in a container (`research/pacman-commit-order.md`), with two corrections found that way:
`NoUpgrade` always leaves a `.pacnew` for an existing file (even an identical one), and a file the new
package stops shipping is kept as `.pacsave` when edited.

## Design

- `config_preview.rs` (pure): `on_upgrade(original, current, new, still_shipped, no_upgrade)` and
  `on_removal(original, current)` return an `Effect` (`Pacnew`, `MaybePacnew`, `Pacsave`,
  `MaybePacsave`) and a one-line reason. The table is in `docs/design/config-preview.md`.
- Inputs: `AlpmBackend::config_inputs(&Plan)` reads every `backup=()` entry of the plan's installed
  packages (name, recorded md5) and hashes the live file with libalpm's own `compute_md5sum`; for the new
  version it reads the repositories' file list (`.files` databases) to know whether the file is still
  shipped. `Sources::config_preview` finds the new package in the cache (`rollback::find_package_file`),
  extracts only the edited files (`Extracted`) and hashes them. No download, no root (a file only root can
  read is a maybe).
- `NoUpgrade` comes from `pacman-conf NoUpgrade`; patterns are matched with the same glob as hooks.
- `PlanPreview { hooks, config }` replaces the hook-only parameter of the printer, so later previews
  need no new plumbing. Neither part changes the plan id.
- Output under the plan, `--json` gets a `config` list (omitted when empty).

## Not covered

Files whose `backup=` status changes between versions, `.pacnew` for files that are not `backup=()`
(pacman does not create those), AUR packages (built later; they show nothing until then), and what
`emrac pacnew` does with the result (unchanged).

## Verification

Unit tests for each row of the table; `dev/e2e/preview.sh`: edited, unchanged-upstream, dropped, removal
and `NoUpgrade` against a local repository, before and after the package is cached, each compared with
what pacman actually leaves in `/etc`.
