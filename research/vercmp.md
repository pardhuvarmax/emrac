# How pacman compares versions

Subject: `/usr/bin/vercmp` (pacman 7.1.0.r9.g54d9411-3), which is a thin wrapper.
The logic is in `libalpm.so.16.0.1` (`alpm_pkg_vercmp` plus two unexported helpers).
Method: Ghidra decompilation through REA, then every row below re-run on the real `vercmp`.
Behavior is described in prose. No decompiled code is reproduced here.

## Overall shape

1. If both strings are null they are equal; a null sorts below a non-null string.
2. If the two strings are byte-identical the answer is 0 straight away.
3. Otherwise each string is split into `epoch : version - release`.
4. Compare epoch, then version, then release. The first non-zero result wins.
5. Release is only compared when **both** sides have one. `1.0` equals `1.0-1`.

## Splitting

- Epoch: scan the leading run of digits. If it is followed by `:`, that run is the
  epoch and the rest is the remainder. If no `:` follows, the epoch is `0`.
  An empty epoch (`:1.0`) is also `0`.
- Release: split at the **last** `-`. Everything after it is the release.
  With no `-`, the release is absent.

## Segment comparison (used for epoch, version and release)

Equal strings short-circuit to 0. Otherwise walk both strings together:

1. Skip any characters that are not letters or digits on each side. All
   separators are equivalent and carry no weight (`.`, `_`, `+`, `~`, `^`, ...).
2. If one side runs out, go to the tail rules below.
3. Take a maximal run from each side, of the same kind as the first side's current
   character: all digits, or all letters.
4. If the run on the first side is one kind and the other side's run is the other
   kind, the numeric run is newer. Digits beat letters.
5. Numeric runs: drop leading zeros, then the longer run is larger; equal lengths
   compare bytewise.
   Alpha runs: bytewise comparison (so `a` < `b`, and uppercase < lowercase).
6. If the runs are equal, continue after them.

Tail rules, when one side is exhausted:
- Both exhausted: equal.
- Remaining side starts with a letter: that side is **older** (`1.0a` < `1.0`).
  This is why `alpha`, `rc` and `beta` suffixes sort before the plain release.
- Remaining side starts with a digit: that side is **newer** (`1.0.1` > `1.0`).

## Things that matter for the Rust port

- `~` and `^` have no special meaning. They are separators like any other.
  `1.0~rc1` is **newer** than `1.0` here (the opposite of dpkg), and
  `1.0^git1` equals `1.0.git1`. Do not port dpkg or newer-rpm rules into the
  pacman path.
- Separator count and kind are ignored: `1.0_1`, `1.0+1` and `1.0.1` are equal.
- A trailing separator makes the string newer: `1.0.` > `1.0`. The trailing
  separator leaves an empty digit/alpha tail on one side, and the
  "remaining side" rule only looks at a letter-or-digit start, so this is a
  quirk worth keeping, not tidying.
- Leading zeros are insignificant: `1.01` equals `1.1`, `1.0` equals `1.00`.
- Missing release equals any release. Comparison is not transitive across that
  rule (`1.0-1` = `1.0` = `1.0-2`, but `1.0-1` < `1.0-2`), so sorting with it
  can misbehave. Treat it as a pairwise predicate.
- Epoch `0` and no epoch are the same.

## Truth table (observed from the installed `vercmp`, 1 means left is newer)

| left | right | result |
|---|---|---|
| 1.0 | 1.0 | 0 |
| 1.0 | 1.0-1 | 0 |
| 1.0-1 | 1.0-2 | -1 |
| 1.0-1 | 1.0 | 0 |
| 1:1.0 | 2.0 | 1 |
| 1:1.0 | 0:2.0 | 1 |
| 0:1.0 | 1.0 | 0 |
| :1.0 | 1.0 | 0 |
| 1.0a | 1.0 | -1 |
| 1.0a | 1.0b | -1 |
| 1.0a | 1.0.1 | -1 |
| 1.0 | 1.0.1 | -1 |
| 1.0alpha | 1.0 | -1 |
| 1.0rc1 | 1.0 | -1 |
| 1.0~rc1 | 1.0 | 1 |
| 1.0~rc1 | 1.0~rc2 | -1 |
| 1.0^git1 | 1.0 | 1 |
| 1.0^git1 | 1.0.git1 | 0 |
| 1.0_1 | 1.0.1 | 0 |
| 1.0+1 | 1.0.1 | 0 |
| 1.01 | 1.1 | 0 |
| 1.0 | 1.00 | 0 |
| 1.10 | 1.9 | 1 |
| a | 1 | -1 |
| 1 | a | 1 |
| 1.0. | 1.0 | 1 |

## Status

The table is confirmed against the real tool. The prose rules are my reading of the
decompilation and are only as good as the table that backs them.

Rust cross-check: emrac does not implement the comparison itself. `downgrade::vercmp`
calls `alpm::vercmp`, which is libalpm's own routine through the `alpm` crate. So there is
nothing to diff for the pacman rows; instead the table is pinned as a regression test,
`downgrade::tests::vercmp_matches_the_pacman_truth_table` (26 rows, passes). dpkg and rpm
rules are not used anywhere in emrac.
