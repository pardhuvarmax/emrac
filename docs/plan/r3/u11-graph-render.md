# Slice u11 (after r3) — draw dependency graphs in the terminal

`emrac graph` printed an adjacency list, or Graphviz `dot` text for another program to draw. Two new formats draw it
in the terminal:

```
emrac graph pacman --depth 2 --format boxes     # emrac draws it itself
emrac graph pacman --depth 2 --format easy      # graph-easy draws it
emrac graph pacman --depth 2 --format dot | dot -Tpng | kitten icat    # an image, in a terminal that shows them
```

## `--format boxes` (native)

Box-drawing characters, no other program. Left to right:

- a package sits in the column of its distance from the root (breadth-first), so every drawn link goes to the next column;
- the root has a double border;
- each parent has one vertical trunk in the gap to its right and its children hang off it with an arrow; an optional
  dependency has a dashed line and a hollow arrow;
- a package that two parents need appears **once**. A link that would have to run backwards (to a package in the same or an
  earlier column, or a cycle) is not drawn but **listed under the drawing**, so nothing is lost;
- boxes take the width of their own name, and the rows of a column sit near the middle of their parents;
- more than 60 packages is not a readable picture: the command refuses and says to lower `--depth` (or use `dot`).

Pure code in `emrac-core/src/graph_render.rs`; tested against fixed drawings (a pair, a chain, an optional link with
back links, a shared dependency, a single package, the size limit).

## `--format easy`

Runs `graph-easy --from=dot --as=boxart` on the dot form. `graph-easy` is not shipped (AUR: `perl-graph-easy`; its
program lives in `/usr/bin/vendor_perl`, which is on `PATH` only in a login shell, so emrac tries that path too). When it
is missing the command says how to get it and that `boxes` needs nothing.

## Not covered

Crossing-minimising layouts, drawing links that go backwards, colour, wrapping to the terminal width (a wide graph is
printed as wide as it is; pipe it to `less -S`), and a built-in image renderer.

## Verification

Seven unit tests; `dev/e2e/graph.sh` in the container: the drawing for a real package, every direct dependency present,
the size refusal, JSON and dot unchanged, `easy` refused cleanly without `graph-easy`, then installing `perl-graph-easy`
through emrac and drawing with it.
