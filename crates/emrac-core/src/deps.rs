//! Dependency inspection: report types plus the graph searches behind
//! `emrac why` and `emrac deps --recursive`. The searches are pure — the
//! package graph is passed in as closures — so they need no libalpm.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;

/// One declared dependency and what satisfies it on this machine.
#[derive(Debug, Clone, Serialize)]
pub struct DepEntry {
    /// As declared, e.g. `glibc>=2.40`.
    pub spec: String,
    /// The installed package providing it, as `(name, version)`.
    pub satisfied_by: Option<(String, String)>,
    /// Only set for optional dependencies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepsReport {
    pub name: String,
    pub version: String,
    /// `false`: described from the sync database.
    pub installed: bool,
    pub depends: Vec<DepEntry>,
    pub optional: Vec<DepEntry>,
    /// Transitive closure of required dependencies, `(name, depth)`; only
    /// filled by `--recursive`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recursive: Option<Vec<(String, usize)>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RdepsReport {
    pub name: String,
    pub required_by: Vec<String>,
    pub optional_for: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WhyReport {
    /// Installed on purpose; `required_by` are installed packages that also
    /// need it.
    Explicit {
        name: String,
        required_by: Vec<String>,
    },
    /// Installed as a dependency; `chain` runs from an explicitly installed
    /// package to `name`.
    Dependency { name: String, chain: Vec<String> },
    /// A dependency that nothing explicit (or anything else) needs.
    Orphan { name: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Orphan {
    pub name: String,
    pub version: String,
    pub installed_size: u64,
}

/// Shortest chain `explicit → … → target` along "is required by" edges, or
/// `None` if no explicitly installed package needs `target`. Ties are broken
/// alphabetically so the answer is stable. `target` itself being explicit is
/// the caller's business (a chain of one is never returned).
pub fn why_chain(
    target: &str,
    required_by: &dyn Fn(&str) -> Vec<String>,
    is_explicit: &dyn Fn(&str) -> bool,
) -> Option<Vec<String>> {
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::from([target.to_string()]);
    let mut queue = VecDeque::from([target.to_string()]);

    while let Some(current) = queue.pop_front() {
        let mut next = required_by(&current);
        next.sort();
        for dependent in next {
            if !seen.insert(dependent.clone()) {
                continue;
            }
            parent.insert(dependent.clone(), current.clone());
            if is_explicit(&dependent) {
                // Walking the parent links runs dependent → … → target,
                // which is already the order to print.
                let mut chain = vec![dependent.clone()];
                let mut at = dependent;
                while let Some(p) = parent.get(&at) {
                    chain.push(p.clone());
                    at = p.clone();
                }
                return Some(chain);
            }
            queue.push_back(dependent);
        }
    }
    None
}

/// Every package reachable from `root` through `deps_of`, breadth-first,
/// each once with its shallowest depth (the root itself is not listed).
pub fn closure(root: &str, deps_of: &dyn Fn(&str) -> Vec<String>) -> Vec<(String, usize)> {
    let mut seen: HashSet<String> = HashSet::from([root.to_string()]);
    let mut out = Vec::new();
    let mut queue = VecDeque::from([(root.to_string(), 0usize)]);

    while let Some((current, depth)) = queue.pop_front() {
        let mut next = deps_of(&current);
        next.sort();
        for dep in next {
            if seen.insert(dep.clone()) {
                out.push((dep.clone(), depth + 1));
                queue.push_back((dep, depth + 1));
            }
        }
    }
    out
}

// ------------------------------------------------- tree, graph, cycles, impact

#[derive(Debug, Clone, Serialize)]
pub struct TreeReport {
    pub name: String,
    pub version: String,
    pub lines: Vec<TreeLine>,
}

/// One line of `emrac tree`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeLine {
    pub depth: usize,
    pub name: String,
    /// An optional dependency.
    pub optional: bool,
    /// Already shown above: its own dependencies are not repeated.
    pub repeat: bool,
    /// Has dependencies that the depth limit hid.
    pub cut: bool,
}

/// Depth-first listing of the dependencies of `root` (not listed itself).
/// `children` gives `(name, optional)` for a package. Each package is
/// expanded once; later appearances are marked `repeat`.
pub fn tree_lines(
    root: &str,
    max_depth: Option<usize>,
    children: &dyn Fn(&str) -> Vec<(String, bool)>,
) -> Vec<TreeLine> {
    fn walk(
        name: &str,
        depth: usize,
        max_depth: Option<usize>,
        children: &dyn Fn(&str) -> Vec<(String, bool)>,
        seen: &mut HashSet<String>,
        out: &mut Vec<TreeLine>,
    ) {
        let mut kids = children(name);
        kids.sort();
        for (kid, optional) in kids {
            let first = seen.insert(kid.clone());
            let at_limit = max_depth.is_some_and(|m| depth >= m);
            let has_more = !children(&kid).is_empty();
            out.push(TreeLine {
                depth,
                name: kid.clone(),
                optional,
                repeat: !first && has_more,
                cut: first && at_limit && has_more,
            });
            if first && !at_limit {
                walk(&kid, depth + 1, max_depth, children, seen, out);
            }
        }
    }
    let mut seen = HashSet::from([root.to_string()]);
    let mut out = Vec::new();
    walk(root, 1, max_depth, children, &mut seen, &mut out);
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphReport {
    pub root: String,
    pub nodes: Vec<String>,
    pub edges: Vec<Edge>,
}

/// The dependency graph below `root`, breadth-first, to `max_depth`
/// levels (`None`: all).
pub fn graph_of(
    root: &str,
    max_depth: Option<usize>,
    children: &dyn Fn(&str) -> Vec<(String, bool)>,
) -> GraphReport {
    let mut nodes = vec![root.to_string()];
    let mut edges = Vec::new();
    let mut seen: HashSet<String> = HashSet::from([root.to_string()]);
    let mut queue = VecDeque::from([(root.to_string(), 0usize)]);
    while let Some((name, depth)) = queue.pop_front() {
        if max_depth.is_some_and(|m| depth >= m) {
            continue;
        }
        let mut kids = children(&name);
        kids.sort();
        for (kid, optional) in kids {
            edges.push(Edge {
                from: name.clone(),
                to: kid.clone(),
                optional,
            });
            if seen.insert(kid.clone()) {
                nodes.push(kid.clone());
                queue.push_back((kid, depth + 1));
            }
        }
    }
    nodes.sort();
    GraphReport {
        root: root.to_string(),
        nodes,
        edges,
    }
}

impl GraphReport {
    /// Graphviz source: `emrac graph x --format dot | dot -Tsvg > x.svg`.
    pub fn to_dot(&self) -> String {
        let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let mut out = String::from("digraph dependencies {\n  rankdir=LR;\n  node [shape=box];\n");
        out.push_str(&format!("  {} [style=bold];\n", q(&self.root)));
        for e in &self.edges {
            out.push_str(&format!(
                "  {} -> {}{};\n",
                q(&e.from),
                q(&e.to),
                if e.optional { " [style=dashed]" } else { "" }
            ));
        }
        out.push_str("}\n");
        out
    }
}

/// Groups of packages that require each other in a ring (strongly connected
/// components with more than one member, or one that requires itself). Each
/// group sorted, groups sorted by their first name.
pub fn cycles(nodes: &[String], deps_of: &dyn Fn(&str) -> Vec<String>) -> Vec<Vec<String>> {
    struct Tarjan<'a> {
        deps_of: &'a dyn Fn(&str) -> Vec<String>,
        index: HashMap<String, usize>,
        low: HashMap<String, usize>,
        on_stack: HashSet<String>,
        stack: Vec<String>,
        counter: usize,
        out: Vec<Vec<String>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, v: &str) {
            self.index.insert(v.to_string(), self.counter);
            self.low.insert(v.to_string(), self.counter);
            self.counter += 1;
            self.stack.push(v.to_string());
            self.on_stack.insert(v.to_string());
            let mut next = (self.deps_of)(v);
            next.sort();
            let mut self_loop = false;
            for w in next {
                if w == v {
                    self_loop = true;
                }
                if !self.index.contains_key(&w) {
                    self.visit(&w);
                    let low = self.low[&w].min(self.low[v]);
                    self.low.insert(v.to_string(), low);
                } else if self.on_stack.contains(&w) {
                    let low = self.index[&w].min(self.low[v]);
                    self.low.insert(v.to_string(), low);
                }
            }
            if self.low[v] == self.index[v] {
                let mut group = Vec::new();
                while let Some(w) = self.stack.pop() {
                    self.on_stack.remove(&w);
                    let done = w == v;
                    group.push(w);
                    if done {
                        break;
                    }
                }
                if group.len() > 1 || self_loop {
                    group.sort();
                    self.out.push(group);
                }
            }
        }
    }
    let mut t = Tarjan {
        deps_of,
        index: HashMap::new(),
        low: HashMap::new(),
        on_stack: HashSet::new(),
        stack: Vec::new(),
        counter: 0,
        out: Vec::new(),
    };
    let mut sorted = nodes.to_vec();
    sorted.sort();
    for n in &sorted {
        if !t.index.contains_key(n) {
            t.visit(n);
        }
    }
    t.out.sort();
    t.out
}

/// What removing a package would take with it.
#[derive(Debug, Clone, Serialize)]
pub struct RemoveImpact {
    /// Installed packages that need it, directly or not: removal is refused
    /// unless they go too (`--cascade`).
    pub breaks: Vec<String>,
    /// Packages that only list it as optional: they keep working.
    pub optional_for: Vec<String>,
    /// Dependencies nothing else would need afterwards.
    pub unneeded: Vec<String>,
}

/// Works out the removal impact of `target` from the installed graph.
/// `required_by` / `optional_for` / `deps_of` give installed names;
/// `is_explicit` tells packages installed on purpose from dependencies.
pub fn remove_impact(
    target: &str,
    required_by: &dyn Fn(&str) -> Vec<String>,
    optional_for: &dyn Fn(&str) -> Vec<String>,
    deps_of: &dyn Fn(&str) -> Vec<String>,
    is_explicit: &dyn Fn(&str) -> bool,
) -> RemoveImpact {
    // Everything that needs the target, transitively.
    let breaks = closure(target, required_by);
    let mut gone: HashSet<String> = HashSet::from([target.to_string()]);
    gone.extend(breaks.iter().map(|(n, _)| n.clone()));

    // Dependencies nothing outside the removed set needs, to a fixpoint.
    let mut unneeded: Vec<String> = Vec::new();
    let mut frontier: Vec<String> = gone.iter().cloned().collect();
    while let Some(pkg) = frontier.pop() {
        for dep in deps_of(&pkg) {
            if gone.contains(&dep) || is_explicit(&dep) {
                continue;
            }
            let still_needed = required_by(&dep).iter().any(|r| !gone.contains(r))
                || optional_for(&dep).iter().any(|r| !gone.contains(r));
            if !still_needed {
                gone.insert(dep.clone());
                unneeded.push(dep.clone());
                frontier.push(dep);
            }
        }
    }
    let mut breaks: Vec<String> = breaks.into_iter().map(|(n, _)| n).collect();
    breaks.sort();
    let mut optional: Vec<String> = optional_for(target)
        .into_iter()
        .filter(|n| !gone.contains(n))
        .collect();
    optional.sort();
    unneeded.sort();
    RemoveImpact {
        breaks,
        optional_for: optional,
        unneeded,
    }
}

/// A package named in an impact report.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactItem {
    pub name: String,
    pub version: String,
    pub installed_size: u64,
}

/// A systemd unit a removed package ships.
#[derive(Debug, Clone, Serialize)]
pub struct UnitFact {
    pub unit: String,
    pub package: String,
    /// Enabled on this machine (linked from a `*.wants` directory).
    pub enabled: bool,
}

/// The full picture for `emrac impact <pkg> --remove`.
#[derive(Debug, Clone, Serialize)]
pub struct RemoveImpactReport {
    pub name: String,
    pub version: String,
    pub explicit: bool,
    pub installed_size: u64,
    pub breaks: Vec<ImpactItem>,
    pub optional_for: Vec<String>,
    pub unneeded: Vec<ImpactItem>,
    /// Everything that would go: the package, what needs it, what only it needed.
    pub reclaimed_bytes: u64,
    pub units: Vec<UnitFact>,
    pub desktop_entries: Vec<String>,
}

/// The systemd units among a package's files (`usr/lib/systemd/system/x.service`).
pub fn service_units(files: &[String]) -> Vec<String> {
    let mut units: Vec<String> = files
        .iter()
        .filter_map(|f| f.strip_prefix("usr/lib/systemd/system/"))
        .filter(|f| !f.contains('/'))
        .filter(|f| {
            [".service", ".socket", ".timer", ".path"]
                .iter()
                .any(|ext| f.ends_with(ext))
        })
        .map(str::to_string)
        .collect();
    units.sort();
    units
}

/// The desktop entries among a package's files.
pub fn desktop_entries(files: &[String]) -> Vec<String> {
    let mut entries: Vec<String> = files
        .iter()
        .filter_map(|f| f.strip_prefix("usr/share/applications/"))
        .filter(|f| f.ends_with(".desktop") && !f.contains('/'))
        .map(str::to_string)
        .collect();
    entries.sort();
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `a`, `b` explicit. a → x → y, b → y, b → z (arrows: "requires").
    fn graph() -> (HashMap<&'static str, Vec<String>>, HashSet<&'static str>) {
        let required_by = HashMap::from([
            ("y", vec!["x".to_string(), "b".to_string()]),
            ("x", vec!["a".to_string()]),
            ("z", vec!["b".to_string()]),
            ("loop1", vec!["loop2".to_string()]),
            ("loop2", vec!["loop1".to_string()]),
        ]);
        (required_by, HashSet::from(["a", "b"]))
    }

    fn chain(target: &str) -> Option<Vec<String>> {
        let (rb, explicit) = graph();
        why_chain(target, &|n| rb.get(n).cloned().unwrap_or_default(), &|n| {
            explicit.contains(n)
        })
    }

    #[test]
    fn why_prefers_the_shortest_chain() {
        // y is needed by b directly (2 hops) and by a via x (3 hops).
        assert_eq!(chain("y").unwrap(), ["b", "y"]);
        assert_eq!(chain("z").unwrap(), ["b", "z"]);
        assert_eq!(chain("x").unwrap(), ["a", "x"]);
    }

    #[test]
    fn why_is_none_without_an_explicit_dependent() {
        assert_eq!(chain("nobody"), None);
    }

    #[test]
    fn why_survives_cycles() {
        assert_eq!(chain("loop1"), None);
    }

    #[test]
    fn why_breaks_ties_alphabetically() {
        let rb = HashMap::from([("t", vec!["m".to_string(), "k".to_string()])]);
        let c = why_chain("t", &|n| rb.get(n).cloned().unwrap_or_default(), &|n| {
            n == "m" || n == "k"
        });
        assert_eq!(c.unwrap(), ["k", "t"]);
    }

    #[test]
    fn closure_lists_each_package_once_with_its_shallowest_depth() {
        let g: HashMap<&str, Vec<String>> = HashMap::from([
            ("root", vec!["a".into(), "b".into()]),
            ("a", vec!["c".into(), "b".into()]),
            ("b", vec!["c".into(), "root".into()]),
        ]);
        let out = closure("root", &|n| g.get(n).cloned().unwrap_or_default());
        assert_eq!(
            out,
            [
                ("a".to_string(), 1),
                ("b".to_string(), 1),
                ("c".to_string(), 2)
            ]
        );
    }

    fn kids<'a>(
        g: &'a HashMap<&'static str, Vec<(&'static str, bool)>>,
    ) -> impl Fn(&str) -> Vec<(String, bool)> + 'a {
        move |n| {
            g.get(n)
                .map(|v| v.iter().map(|(k, o)| (k.to_string(), *o)).collect())
                .unwrap_or_default()
        }
    }

    fn sample() -> HashMap<&'static str, Vec<(&'static str, bool)>> {
        HashMap::from([
            ("app", vec![("lib", false), ("util", false), ("docs", true)]),
            ("lib", vec![("core", false)]),
            ("util", vec![("core", false), ("lib", false)]),
            ("core", vec![]),
            ("docs", vec![("core", false)]),
        ])
    }

    #[test]
    fn the_tree_expands_each_package_once_and_marks_repeats_and_cuts() {
        let g = sample();
        let t = tree_lines("app", None, &kids(&g));
        let line = |i: usize| (t[i].depth, t[i].name.as_str(), t[i].optional, t[i].repeat);
        assert_eq!(line(0), (1, "docs", true, false));
        assert_eq!(line(1), (2, "core", false, false));
        assert_eq!(line(2), (1, "lib", false, false));
        assert_eq!(
            line(3),
            (2, "core", false, false),
            "a leaf is listed wherever it is needed"
        );
        assert_eq!(line(4), (1, "util", false, false));
        // lib and core appear again under util, not expanded again.
        assert!(t
            .iter()
            .any(|l| l.name == "lib" && l.depth == 2 && l.repeat));
        let limited = tree_lines("app", Some(1), &kids(&g));
        assert!(limited.iter().all(|l| l.depth == 1));
        assert!(limited.iter().find(|l| l.name == "lib").unwrap().cut);
        assert!(
            limited.iter().find(|l| l.name == "docs").unwrap().cut,
            "docs has dependencies of its own"
        );
    }

    #[test]
    fn the_graph_lists_nodes_and_edges_to_a_depth_and_renders_dot() {
        let g = sample();
        let full = graph_of("app", None, &kids(&g));
        assert_eq!(full.nodes, ["app", "core", "docs", "lib", "util"]);
        assert_eq!(full.edges.len(), 7);
        let one = graph_of("app", Some(1), &kids(&g));
        assert_eq!(one.edges.len(), 3);
        assert_eq!(one.nodes.len(), 4);
        let dot = full.to_dot();
        assert!(dot.starts_with("digraph dependencies {"));
        assert!(dot.contains("\"app\" -> \"docs\" [style=dashed];"), "{dot}");
        assert!(dot.contains("\"lib\" -> \"core\";"));
        assert!(dot.trim_end().ends_with('}'));
    }

    #[test]
    fn cycles_are_found_as_groups_and_a_plain_chain_has_none() {
        let g: HashMap<&str, Vec<String>> = HashMap::from([
            ("a", vec!["b".into()]),
            ("b", vec!["c".into()]),
            ("c", vec!["a".into(), "d".into()]),
            ("d", vec![]),
            ("e", vec!["e".into()]),
            ("f", vec!["d".into()]),
        ]);
        let nodes: Vec<String> = g.keys().map(|k| k.to_string()).collect();
        let found = cycles(&nodes, &|n| g.get(n).cloned().unwrap_or_default());
        assert_eq!(found, vec![vec!["a", "b", "c"], vec!["e"]]);
        let chain: HashMap<&str, Vec<String>> =
            HashMap::from([("x", vec!["y".into()]), ("y", vec![])]);
        let nodes = vec!["x".to_string(), "y".to_string()];
        assert!(cycles(&nodes, &|n| chain.get(n).cloned().unwrap_or_default()).is_empty());
    }

    #[test]
    fn removal_impact_lists_who_breaks_what_becomes_unneeded_and_optional_users() {
        // app (explicit) -> lib -> core ; tool (explicit) -> core ; extra (dep) <- lib ; opt lists lib as optional
        let required_by: HashMap<&str, Vec<String>> = HashMap::from([
            ("lib", vec!["app".into()]),
            ("core", vec!["lib".into(), "tool".into()]),
            ("extra", vec!["lib".into()]),
            ("leaf", vec!["extra".into()]),
        ]);
        let deps: HashMap<&str, Vec<String>> = HashMap::from([
            ("app", vec!["lib".into()]),
            ("lib", vec!["core".into(), "extra".into()]),
            ("tool", vec!["core".into()]),
            ("extra", vec!["leaf".into()]),
        ]);
        let optional: HashMap<&str, Vec<String>> = HashMap::from([("lib", vec!["viewer".into()])]);
        let explicit = |n: &str| matches!(n, "app" | "tool" | "viewer");
        let impact = remove_impact(
            "lib",
            &|n| required_by.get(n).cloned().unwrap_or_default(),
            &|n| optional.get(n).cloned().unwrap_or_default(),
            &|n| deps.get(n).cloned().unwrap_or_default(),
            &explicit,
        );
        assert_eq!(impact.breaks, ["app"]);
        assert_eq!(impact.optional_for, ["viewer"]);
        // core stays (tool needs it); extra and then leaf become unneeded.
        assert_eq!(impact.unneeded, ["extra", "leaf"]);
    }

    #[test]
    fn units_and_desktop_entries_are_picked_from_a_file_list() {
        let files: Vec<String> = [
            "usr/bin/app",
            "usr/lib/systemd/system/app.service",
            "usr/lib/systemd/system/app.socket",
            "usr/lib/systemd/system/app.service.d/override.conf",
            "usr/lib/systemd/system/multi-user.target.wants/",
            "usr/share/applications/app.desktop",
            "usr/share/applications/sub/other.desktop",
            "usr/share/doc/app/README",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(service_units(&files), ["app.service", "app.socket"]);
        assert_eq!(desktop_entries(&files), ["app.desktop"]);
    }
}
