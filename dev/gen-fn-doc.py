#!/usr/bin/env python3
"""Writes docs/fn.md: every function in emrac's Rust sources, by crate and file.

    python3 dev/gen-fn-doc.py            # write docs/fn.md
    python3 dev/gen-fn-doc.py --check    # exit 1 if docs/fn.md is out of date

Functions inside `#[cfg(test)]` modules, and the integration tests under tests/,
are listed separately as tests (a `#[test]`) and test helpers. A method is shown as `Type::name`; a function in a
trait impl as `Type::name` with `(Trait)`. Visibility is shown: `pub`, `pub(crate)`
or nothing for private.
"""
import re, sys, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "docs" / "fn.md"


def blank_noncode(src):
    """Same text with comments and the insides of string/char literals blanked,
    so braces and the word `fn` inside them are not seen. Newlines are kept."""
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        two = src[i:i + 2]
        if two == "//":
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i)); i = j
        elif two == "/*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src[j:j + 2] == "/*": depth += 1; j += 2
                elif src[j:j + 2] == "*/": depth -= 1; j += 2
                else: j += 1
            out.append("".join("\n" if ch == "\n" else " " for ch in src[i:j])); i = j
        elif c == "r" and re.match(r'r#*"', src[i:i + 12]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.match(r'r(#*)"', src[i:])
            hashes = m.group(1)
            end = src.find('"' + hashes, i + len(m.group(0)))
            end = n if end < 0 else end + 1 + len(hashes)
            out.append("".join("\n" if ch == "\n" else " " for ch in src[i:end])); i = end
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            j = min(j + 1, n)
            out.append('"' + "".join("\n" if ch == "\n" else " " for ch in src[i + 1:j - 1]) + '"'); i = j
        elif c == "'":
            m = re.match(r"'(\\.[^']*|[^\\'])'", src[i:i + 12])
            if m:
                out.append(" " * len(m.group(0))); i += len(m.group(0))
            else:
                out.append(c); i += 1
        else:
            out.append(c); i += 1
    return "".join(out)


FN = re.compile(r"\b(pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*fn\s+([A-Za-z_][A-Za-z0-9_]*)")
IMPL = re.compile(r"\bimpl\b(?:<[^{]*?>)?\s+(?:([A-Za-z_][\w:<>, &'\[\]]*?)\s+for\s+)?([A-Za-z_][\w:]*)")
TESTMOD = re.compile(r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?mod\s+\w+\s*\{")


def scan(path, whole_file_is_tests=False):
    raw = path.read_text()
    code = blank_noncode(raw)
    lines_start = [0]
    for m in re.finditer("\n", code):
        lines_start.append(m.end())
    def line_of(pos):
        lo, hi = 0, len(lines_start) - 1
        while lo < hi:
            mid = (lo + hi + 1) // 2
            if lines_start[mid] <= pos: lo = mid
            else: hi = mid - 1
        return lo + 1

    # test module ranges
    test_ranges = []
    for m in TESTMOD.finditer(code):
        depth, j = 1, m.end()
        while j < len(code) and depth:
            if code[j] == "{": depth += 1
            elif code[j] == "}": depth -= 1
            j += 1
        test_ranges.append((m.start(), j))
    def in_test(pos):
        return any(a <= pos < b for a, b in test_ranges)

    # impl block ranges: (start, end, type, trait)
    impls = []
    for m in IMPL.finditer(code):
        # find the opening brace of the impl
        k = m.end()
        brace = code.find("{", k)
        semi = code.find(";", k)
        if brace < 0 or (0 <= semi < brace):
            continue
        header = code[m.start():brace]
        mm = IMPL.match(header)
        trait, ty = (mm.group(1), mm.group(2)) if mm else (None, "?")
        depth, j = 1, brace + 1
        while j < len(code) and depth:
            if code[j] == "{": depth += 1
            elif code[j] == "}": depth -= 1
            j += 1
        impls.append((brace, j, ty.split("::")[-1], (trait or "").split("<")[0].split("::")[-1].strip()))
    def owner(pos):
        best = None
        for a, b, ty, trait in impls:
            if a < pos < b and (best is None or a > best[0]):
                best = (a, b, ty, trait)
        return best

    found, tests, helpers = [], [], []
    for m in FN.finditer(code):
        pos = m.start()
        if in_test(pos) or whole_file_is_tests:
            before = code[max(0, pos - 200):pos]
            attrs = re.findall(r"#\[[^\]]*\]", before.split("}")[-1].split(";")[-1])
            (tests if any(a.startswith("#[test") or a.startswith("#[tokio::test") for a in attrs) else helpers).append((line_of(pos), m.group(2)))
            continue
        vis = (m.group(1) or "").strip()
        own = owner(pos)
        name = m.group(2)
        label = f"{own[2]}::{name}" if own else name
        if own and own[3]:
            label += f" ({own[3]})"
        found.append((line_of(pos), vis, label))
    return found, tests, helpers


def main():
    files = sorted(p for c in ("emrac-core", "emrac-cli") for p in (ROOT / "crates" / c / "src").rglob("*.rs"))
    files += sorted((ROOT / "crates" / "emrac-cli" / "tests").rglob("*.rs"))
    per_crate = {}
    total = n_tests = n_helpers = 0
    for p in files:
        is_integration = "tests" in p.relative_to(ROOT / "crates").parts[1:2]
        funcs, tests, helpers = scan(p, whole_file_is_tests=is_integration)
        crate = p.relative_to(ROOT / "crates").parts[0]
        per_crate.setdefault(crate, []).append((p.relative_to(ROOT).as_posix(), funcs, tests, helpers))
        total += len(funcs); n_tests += len(tests); n_helpers += len(helpers)
    out = ["# Every function in emrac", "",
           "Generated by `python3 dev/gen-fn-doc.py` from the Rust sources; do not edit by hand. "
           "Each file lists its **functions** (with visibility, `Type::name` for a method, the trait named for a trait "
           "implementation, and the line it starts on), then its **tests** (every `#[test]`), then the **test helpers** "
           "(the other functions inside test modules). The tests are described, with the end-to-end checks, in "
           "[`docs/tests/emrac-tests.rst`](./tests/emrac-tests.rst).", "",
           f"**{total} functions, {n_tests} tests and {n_helpers} test helpers in {len(files)} files.**", ""]
    for crate, items in per_crate.items():
        nf = sum(len(f) for _, f, _, _ in items); nt = sum(len(t) for _, _, t, _ in items)
        out += [f"## {crate} ({nf} functions, {nt} tests, {len(items)} files)", ""]
        for rel, funcs, tests, helpers in items:
            if not (funcs or tests or helpers):
                continue
            out += [f"### `{rel}`", ""]
            if funcs:
                out += [f"Functions ({len(funcs)})", ""]
                out += [f"- `{(vis + ' ') if vis else ''}{label}()` :{line}" for line, vis, label in funcs]
                out.append("")
            if tests:
                out += [f"Tests ({len(tests)})", ""]
                out += [f"- `{name}()` :{line}" for line, name in tests]
                out.append("")
            if helpers:
                out += [f"Test helpers ({len(helpers)})", ""]
                out += [f"- `{name}()` :{line}" for line, name in helpers]
                out.append("")
    text = "\n".join(out)
    if "--check" in sys.argv:
        if not OUT.exists() or OUT.read_text() != text:
            print("docs/fn.md is out of date: run python3 dev/gen-fn-doc.py", file=sys.stderr)
            sys.exit(1)
        print("docs/fn.md is current")
        return
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}: {total} functions, {n_tests} tests, {n_helpers} test helpers in {len(files)} files")


main()
