# Slice u16 — `tree`, `graph`, `impact`, `cycles`

The rest of the SPEC's Dependency Intelligence group (Part V; Part VIII flags), after `deps`,
`rdeps`, `why` and `orphans` (r1 cycle, u12). All read-only: libalpm only, no root, no network,
`--json`.

## Commands

- `emrac tree <pkg> [--depth N] [--optional]`: the installed dependencies of an installed package
  as an indented tree. Each package is expanded once; when it comes up again it is marked
  `shown above`; at the depth limit a package that has more below is marked `more below`.
  `--optional` adds installed optional dependencies, marked `optional`.
- `emrac graph <pkg> [--depth N] [--optional] [--format text|dot|json]`: the same graph as nodes
  and edges. `dot` is Graphviz source (`emrac graph firefox --format dot | dot -Tsvg > f.svg`;
  optional edges dashed, the root bold); `json` is what `--json` gives.
- `emrac impact <pkg> [--remove | --install | --upgrade]` (default `--remove`, the three are
  exclusive). Removal: what needs the package, directly or not (pacman refuses to remove it unless
  they go too); who only lists it as optional (they keep working); the dependencies nothing else
  would need afterwards (to a fixpoint, never an explicitly installed one); systemd units the
  removed packages ship and whether each is enabled; desktop entries; the disk space freed in
  all. `--install` and `--upgrade` show the plan `install` / `upgrade` would make (the same
  plans, `--dry-run` style), so there is one source of truth.
- `emrac cycles`: groups of installed packages that require each other in a ring (strongly
  connected components of the required-dependency graph, or a package that requires itself). Said
  not to be a fault: pacman installs such a group together.

Not built: `--make` (makedepends are not recorded for installed packages) and graphs of
packages that are not installed (`deps` covers one level of those).

## Code

- `deps.rs`: `tree_lines`, `graph_of` (+ `to_dot`), `cycles` (Tarjan), `remove_impact`,
  `service_units`, `desktop_entries`; all take the graph as closures, so they are pure.
- `backend/alpm.rs`: `installed_names`, `installed_optional_names`, `package_info`,
  `package_files`. `sources.rs`: `tree`, `graph`, `cycles`, `impact_remove`.
- CLI: `cli.rs` (`Tree`, `Graph`, `Impact`, `Cycles`, `GraphFormat`), `commands/deps.rs`,
  `output.rs` (long lists show 30 and say how many more; `--json` has all).

## Verification

Unit tests: the tree (order, repeats, depth cut), the graph (nodes, edges, depth, dot), cycles
(a ring, a self-loop, a chain), the removal impact (breaks, optional, unneeded to a fixpoint),
units and desktop entries from a file list. Container: all four on a real pacman database. Run
read-only on a real system with 1,400 packages: the `openssl` removal impact lists 814 dependents.
