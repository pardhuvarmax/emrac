# docs/

Design and planning documents that aren't shipped product code, and don't
fit in a single crate's own README.

- [`plan/`](./plan) (one folder per release cycle) — per-slice implementation plans, written before the
  work starts.
- [`design/`](./design) — design documents that settle how a part of emrac works.
- [`releases/`](./releases) — one release document per release (each is
  also the text of its tag).
- [`tests/`](./tests) — the list of every passing test, generated and kept
  current by `dev/gen-tests-doc.sh`.
- [`feats/`](./feats) — [`feats.md`](./feats/feats.md) lists every feature and its status; the folder holds the collected feature ideas and the cross-distribution
  design; nothing here is a commitment.
