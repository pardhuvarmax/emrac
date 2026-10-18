//! A dependency graph drawn in the terminal with box-drawing characters
//! (`emrac graph --format boxes`), without Graphviz.
//!
//! Layout, left to right: a package sits in the column of its distance from the
//! root (breadth-first), so every drawn link goes from one column to the next.
//! A link to a package that is already in the same or an earlier column would
//! have to run backwards; those are listed under the drawing instead, so
//! nothing is left out. Each parent has one vertical trunk in the gap to its
//! right, and each child hangs off it with an arrow; an optional dependency
//! has a dashed arrow. See `docs/plan/r3/u11-graph-render.md`.

use std::collections::{HashMap, VecDeque};

use crate::deps::GraphReport;

/// More packages than this is not readable as a picture.
pub const MAX_NODES: usize = 60;

const U: u8 = 1;
const D: u8 = 2;
const L: u8 = 4;
const R: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    Text(char),
    Line { mask: u8, dashed: bool },
}

struct Canvas {
    rows: Vec<Vec<Cell>>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Canvas {
            rows: vec![vec![Cell::Empty; width]; height],
        }
    }

    fn text(&mut self, x: usize, y: usize, c: char) {
        self.rows[y][x] = Cell::Text(c);
    }

    fn line(&mut self, x: usize, y: usize, mask: u8, dashed: bool) {
        match self.rows[y][x] {
            Cell::Line { mask: m, dashed: d } => {
                self.rows[y][x] = Cell::Line {
                    mask: m | mask,
                    dashed: d && dashed,
                }
            }
            Cell::Empty => self.rows[y][x] = Cell::Line { mask, dashed },
            Cell::Text(_) => {}
        }
    }

    /// A horizontal run from `xa` to `xb` on row `y`.
    fn h(&mut self, y: usize, xa: usize, xb: usize, dashed: bool) {
        for x in xa..=xb {
            let mut m = 0;
            if x > xa {
                m |= L;
            }
            if x < xb {
                m |= R;
            }
            if m != 0 {
                self.line(x, y, m, dashed);
            }
        }
    }

    /// A vertical run from `ya` to `yb` in column `x`.
    fn v(&mut self, x: usize, ya: usize, yb: usize) {
        for y in ya..=yb {
            let mut m = 0;
            if y > ya {
                m |= U;
            }
            if y < yb {
                m |= D;
            }
            if m != 0 {
                self.line(x, y, m, false);
            }
        }
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for row in &self.rows {
            let line: String = row
                .iter()
                .map(|c| match *c {
                    Cell::Empty => ' ',
                    Cell::Text(t) => t,
                    Cell::Line { mask, dashed } => glyph(mask, dashed),
                })
                .collect();
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }
}

fn glyph(mask: u8, dashed: bool) -> char {
    match mask {
        m if m == L | R || m == L || m == R => {
            if dashed {
                '\u{254c}'
            } else {
                '\u{2500}'
            }
        }
        m if m == U | D || m == U || m == D => '\u{2502}',
        m if m == U | R => '\u{2514}',
        m if m == U | L => '\u{2518}',
        m if m == D | R => '\u{250c}',
        m if m == D | L => '\u{2510}',
        m if m == U | D | R => '\u{251c}',
        m if m == U | D | L => '\u{2524}',
        m if m == U | L | R => '\u{2534}',
        m if m == D | L | R => '\u{252c}',
        _ => '\u{253c}',
    }
}

/// Draws the report, or says why it cannot be drawn.
pub fn render(report: &GraphReport) -> Result<String, String> {
    if report.nodes.len() > MAX_NODES {
        return Err(format!(
            "{} packages is too many to draw (the limit is {MAX_NODES}); lower --depth, or use --format dot",
            report.nodes.len()
        ));
    }
    // Adjacency without repeated links, in the order the report gives them.
    let mut children: HashMap<&str, Vec<(&str, bool)>> = HashMap::new();
    for e in &report.edges {
        let list = children.entry(e.from.as_str()).or_default();
        if !list.iter().any(|(t, _)| *t == e.to) {
            list.push((e.to.as_str(), e.optional));
        }
    }
    // Distance from the root.
    let mut depth: HashMap<&str, usize> = HashMap::from([(report.root.as_str(), 0)]);
    let mut queue = VecDeque::from([report.root.as_str()]);
    while let Some(n) = queue.pop_front() {
        for (c, _) in children.get(n).into_iter().flatten() {
            if !depth.contains_key(c) {
                depth.insert(c, depth[n] + 1);
                queue.push_back(c);
            }
        }
    }
    let layer_count = depth.values().max().map_or(1, |m| m + 1);
    let mut layers: Vec<Vec<&str>> = vec![Vec::new(); layer_count];
    for (n, d) in &depth {
        layers[*d].push(n);
    }
    // Links that go forward by exactly one column are drawn; the rest are listed.
    let mut drawn: Vec<(&str, &str, bool)> = Vec::new();
    let mut aside: Vec<(&str, &str)> = Vec::new();
    for (from, list) in &children {
        if !depth.contains_key(from) {
            continue;
        }
        for (to, optional) in list {
            match depth.get(to) {
                Some(d) if *d == depth[from] + 1 => drawn.push((from, to, *optional)),
                Some(_) => aside.push((from, to)),
                None => {}
            }
        }
    }
    drawn.sort();
    aside.sort();

    // Vertical placement: a package sits near the middle of its parents.
    let mut y: HashMap<&str, usize> = HashMap::new();
    for (l, layer) in layers.iter_mut().enumerate() {
        if l == 0 {
            y.insert(report.root.as_str(), 0);
            continue;
        }
        let mut wants: Vec<(usize, &str)> = layer
            .iter()
            .map(|n| {
                let ys: Vec<usize> = drawn
                    .iter()
                    .filter(|(_, to, _)| to == n)
                    .filter_map(|(from, _, _)| y.get(from).copied())
                    .collect();
                let want = if ys.is_empty() {
                    0
                } else {
                    ys.iter().sum::<usize>() / ys.len()
                };
                (want, *n)
            })
            .collect();
        wants.sort();
        let mut next = 0;
        for (want, n) in &wants {
            let at = (*want).max(next);
            y.insert(n, at);
            next = at + 4;
        }
        *layer = wants.into_iter().map(|(_, n)| n).collect();
    }
    let height = y.values().max().map_or(0, |m| m + 3);

    // Columns.
    let width_of = |layer: &[&str]| layer.iter().map(|n| n.chars().count()).max().unwrap_or(0) + 4;
    let mut x0 = Vec::new();
    let mut parents_of_layer: Vec<Vec<&str>> = Vec::new();
    let mut at = 0;
    for (l, layer) in layers.iter().enumerate() {
        x0.push(at);
        let mut parents: Vec<&str> = layer
            .iter()
            .copied()
            .filter(|n| drawn.iter().any(|(from, _, _)| from == n))
            .collect();
        parents.sort_by_key(|n| y[n]);
        let gap = if l + 1 < layer_count {
            parents.len() + 2
        } else {
            0
        };
        at += width_of(layer) + gap;
        parents_of_layer.push(parents);
    }
    let mut canvas = Canvas::new(at, height);

    // Boxes.
    for (l, layer) in layers.iter().enumerate() {
        for n in layer {
            let w = n.chars().count() + 4;
            let (top, bottom) = (y[n], y[n] + 2);
            let (tl, tr, bl, br, hz, vt) = if l == 0 {
                (
                    '\u{2554}', '\u{2557}', '\u{255a}', '\u{255d}', '\u{2550}', '\u{2551}',
                )
            } else {
                (
                    '\u{250c}', '\u{2510}', '\u{2514}', '\u{2518}', '\u{2500}', '\u{2502}',
                )
            };
            let x = x0[l];
            for i in 0..w {
                let (t, b) = match i {
                    0 => (tl, bl),
                    i if i == w - 1 => (tr, br),
                    _ => (hz, hz),
                };
                canvas.text(x + i, top, t);
                canvas.text(x + i, bottom, b);
            }
            canvas.text(x, top + 1, vt);
            canvas.text(x + w - 1, top + 1, vt);
            for (i, c) in n.chars().enumerate() {
                canvas.text(x + 2 + i, top + 1, c);
            }
        }
    }

    // Links, one trunk per parent in the gap to its right.
    for l in 0..layer_count.saturating_sub(1) {
        let chan = x0[l] + width_of(&layers[l]);
        let arrow = x0[l + 1] - 1;
        for (i, p) in parents_of_layer[l].iter().enumerate() {
            let trunk = chan + 1 + i;
            let rp = y[p] + 1;
            let right = x0[l] + p.chars().count() + 3;
            canvas.h(rp, right, trunk, false);
            canvas.text(right, rp, if l == 0 { '\u{255f}' } else { '\u{251c}' });
            let kids: Vec<(&str, bool)> = drawn
                .iter()
                .filter(|(from, _, _)| from == p)
                .map(|(_, to, o)| (*to, *o))
                .collect();
            let rows: Vec<usize> = kids.iter().map(|(c, _)| y[c] + 1).collect();
            let top = rows.iter().copied().chain([rp]).min().unwrap_or(rp);
            let bottom = rows.iter().copied().chain([rp]).max().unwrap_or(rp);
            canvas.v(trunk, top, bottom);
            for (c, optional) in kids {
                let rc = y[c] + 1;
                canvas.h(rc, trunk, arrow, optional);
                canvas.text(arrow, rc, if optional { '\u{25b7}' } else { '\u{25b6}' });
            }
        }
    }

    let mut out = canvas.render();
    if drawn.iter().any(|(_, _, o)| *o) {
        out.push_str("\n(dashed arrows are optional dependencies)\n");
    }
    if !aside.is_empty() {
        out.push_str("\nAlso, links back to a package drawn in the same or an earlier column:\n");
        let mut by_from: Vec<(&str, Vec<&str>)> = Vec::new();
        for (from, to) in aside {
            match by_from.last_mut() {
                Some((f, list)) if *f == from => list.push(to),
                _ => by_from.push((from, vec![to])),
            }
        }
        for (from, list) in by_from {
            out.push_str(&format!("  {from} -> {}\n", list.join(", ")));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::Edge;

    fn graph(root: &str, edges: &[(&str, &str, bool)]) -> GraphReport {
        let mut nodes = vec![root.to_string()];
        for (f, t, _) in edges {
            for n in [f, t] {
                if !nodes.iter().any(|x| x == n) {
                    nodes.push((*n).to_string());
                }
            }
        }
        GraphReport {
            root: root.into(),
            nodes,
            edges: edges
                .iter()
                .map(|(f, t, o)| Edge {
                    from: (*f).into(),
                    to: (*t).into(),
                    optional: *o,
                })
                .collect(),
        }
    }

    #[test]
    fn a_root_with_two_dependencies_sorted_by_name() {
        let g = graph("app", &[("app", "libb", false), ("app", "liba", false)]);
        assert_eq!(
            render(&g).unwrap(),
            "\
╔═════╗   ┌──────┐
║ app ╟─┬▶│ liba │
╚═════╝ │ └──────┘
        │
        │ ┌──────┐
        └▶│ libb │
          └──────┘
"
        );
    }

    #[test]
    fn a_chain_runs_left_to_right() {
        let g = graph(
            "a",
            &[("a", "b", false), ("b", "c", false), ("c", "d", false)],
        );
        assert_eq!(
            render(&g).unwrap(),
            "\
╔═══╗   ┌───┐   ┌───┐   ┌───┐
║ a ╟──▶│ b ├──▶│ c ├──▶│ d │
╚═══╝   └───┘   └───┘   └───┘
"
        );
    }

    #[test]
    fn an_optional_link_is_dashed_and_links_back_are_listed_not_lost() {
        let g = graph(
            "app",
            &[
                ("app", "left", false),
                ("app", "right", true),
                ("left", "shared", false),
                ("right", "shared", false),
                ("left", "app", false),
                ("shared", "left", false),
            ],
        );
        let out = render(&g).unwrap();
        assert_eq!(
            out,
            "╔═════╗   ┌──────┐\n║ app ╟─┬▶│ left ├──┐\n╚═════╝ │ └──────┘  │  ┌────────┐\n        │           └┬▶│ shared │\n        │ ┌───────┐  │ └────────┘\n        └▷│ right ├──┘\n          └───────┘\n\n(dashed arrows are optional dependencies)\n\nAlso, links back to a package drawn in the same or an earlier column:\n  left -> app\n  shared -> left\n"
        );
        assert!(out.contains('\u{25b7}'), "the optional arrow is hollow");
    }

    #[test]
    fn a_shared_dependency_sits_once_with_the_extra_links_listed() {
        let g = graph(
            "pacman",
            &[
                ("pacman", "bash", false),
                ("pacman", "glibc", false),
                ("pacman", "curl", false),
                ("bash", "glibc", false),
                ("bash", "readline", false),
                ("curl", "openssl", false),
                ("curl", "zlib", false),
                ("openssl", "zlib", false),
                ("readline", "ncurses", false),
            ],
        );
        assert_eq!(
            render(&g).unwrap(),
            "╔════════╗   ┌──────┐     ┌──────────┐   ┌─────────┐\n║ pacman ╟─┬▶│ bash ├────▶│ readline ├──▶│ ncurses │\n╚════════╝ │ └──────┘     └──────────┘   └─────────┘\n           │\n           │ ┌──────┐     ┌─────────┐\n           ├▶│ curl ├───┬▶│ openssl │\n           │ └──────┘   │ └─────────┘\n           │            │\n           │ ┌───────┐  │ ┌──────┐\n           └▶│ glibc │  └▶│ zlib │\n             └───────┘    └──────┘\n\nAlso, links back to a package drawn in the same or an earlier column:\n  bash -> glibc\n  openssl -> zlib\n"
        );
    }

    #[test]
    fn every_package_and_every_link_is_in_the_output() {
        let g = graph(
            "r",
            &[
                ("r", "a", false),
                ("r", "b", false),
                ("a", "c", false),
                ("b", "c", false),
                ("c", "r", false),
            ],
        );
        let out = render(&g).unwrap();
        for n in ["r", "a", "b", "c"] {
            assert!(out.contains(&format!(" {n} ")), "{n} is drawn");
        }
        assert!(out.contains("c -> r"), "the back link is listed");
    }

    #[test]
    fn a_lone_package_is_one_box() {
        let g = graph("solo", &[]);
        assert_eq!(render(&g).unwrap(), "╔══════╗\n║ solo ║\n╚══════╝\n");
    }

    #[test]
    fn too_big_a_graph_is_refused_with_advice() {
        let edges: Vec<(String, String)> = (0..70)
            .map(|i| ("r".to_string(), format!("p{i}")))
            .collect();
        let refs: Vec<(&str, &str, bool)> = edges
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str(), false))
            .collect();
        let e = render(&graph("r", &refs)).unwrap_err();
        assert!(e.contains("lower --depth") && e.contains("71"), "{e}");
    }
}
