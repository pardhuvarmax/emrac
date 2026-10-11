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
}
