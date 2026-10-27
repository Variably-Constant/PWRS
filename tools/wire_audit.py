"""Reports functions with no caller outside test code.

A framework can pass every test while shipping surface nothing
reaches. This walks every function defined in the crates and the
example modules and checks whether anything in production refers to it.

Production is every `.rs` file under `crates/*/src` and
`examples/*/src`, with the bodies of items compiled only for tests
(`#[cfg(test)]`, or a cfg such as `all(test, target_arch = "x86_64")`)
and `*_tests.rs` removed, plus the C# and PowerShell files outside
`target`, the Pester suites among them: a proxy method or a cmdlet is
called from PowerShell under its PascalCase name and never from Rust,
so a Rust-only search reports it as dead when it is not. A method
marked `#[member]` or `#[base]` is declared under no name of its own,
so its mark counts as its caller.

Test-support modules are listed separately rather than counted as
failures: `pwrs::testing` and `pwrs-host` exist so that a module
author can test a module, so being reachable only from tests is what
they are for.

    python tools/wire_audit.py          report and exit non-zero on a finding
    python tools/wire_audit.py --list   report everything, always exit 0
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Reachable without a Rust caller, with the reason each one is here.
EXEMPT = {
    "main": "the cargo-pwrs entry point",
    "deref": "std::ops::Deref, reached through the deref operator",
    "deref_mut": "std::ops::DerefMut, reached through the deref operator",
    "run_winps_script": "pwrs-build's public capturing runner for Windows PowerShell, beside run_pwsh_script; "
    "cargo pwrs test streams instead, and removing it would break the published crate's API",
    "compile_exe": "pwrs-build's public executable compile, for a program building an executable for Windows PowerShell; "
    "cargo pwrs builds none, and its test compiles and runs one",
    "framework_exe_references": "pwrs-build's public reference set for compile_exe; cargo pwrs builds no executable, "
    "and its test compiles and runs one",
}

# Surface whose callers are tests by design.
TEST_SUPPORT = ("crates/pwrs/src/testing.rs", "crates/pwrs-host/src/lib.rs")

DEF = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)\s*[(<]")

# A function with one of these attributes is called from the code its
# macro generates, so the source has no call to it by name.
WIRED_BY_ATTRIBUTE = re.compile(r"#\[(completer|dynamic_params)\s*\(")

# A #[psmethods] method with one of these marks is declared under no name
# of its own: #[member] backs a member of the class's .NET base or
# interfaces, which the generated class declares and .NET calls, and
# #[base] is called by the generated class's constructor.
WIRED_BY_MARK = re.compile(r"#\[(member\s*=|base\s*\]|base\s*\()")


def own_attributes(text: str, at: int) -> str:
    """The outer attributes of an item that starts on the line offset `at`
    is on: the attribute lines among the attribute and doc-comment lines
    directly above it. A doc comment that names an attribute is text."""
    lines = text[:at].split("\n")
    lines.pop()
    own = []
    while lines and lines[-1].strip().startswith(("#[", "///")):
        line = lines.pop()
        if line.strip().startswith("#["):
            own.append(line)
    return "\n".join(own)


def cfg_predicate(text: str, open_paren: int):
    """The predicate of the `#[cfg(...)]` whose `(` is at `open_paren`,
    and the offset just past its closing `)`, or None when it never
    closes. Parentheses inside string literals do not count."""
    depth, j = 0, open_paren
    while j < len(text):
        c = text[j]
        if c == '"':
            j = text.find('"', j + 1)
            if j < 0:
                return None
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return text[open_paren + 1 : j], j + 1
        j += 1
    return None


def split_terms(s: str) -> list:
    """Splits a cfg argument list at its top-level commas."""
    terms, depth, quoted, cur = [], 0, False, []
    for c in s:
        if c == '"':
            quoted = not quoted
        elif not quoted and c == "(":
            depth += 1
        elif not quoted and c == ")":
            depth -= 1
        elif not quoted and c == "," and depth == 0:
            terms.append("".join(cur).strip())
            cur = []
            continue
        cur.append(c)
    tail = "".join(cur).strip()
    if tail:
        terms.append(tail)
    return terms


def test_only(predicate: str) -> bool:
    """Whether code under `cfg(predicate)` is compiled only for tests:
    `test`, an `all` with a test-only term, or an `any` whose every term
    is test-only. `not(...)` and every other predicate are production."""
    predicate = predicate.strip()
    if predicate == "test":
        return True
    m = re.fullmatch(r"(all|any)\s*\((.*)\)", predicate, re.S)
    if not m:
        return False
    terms = split_terms(m.group(2))
    if m.group(1) == "all":
        return any(test_only(t) for t in terms)
    return bool(terms) and all(test_only(t) for t in terms)


# Where a comment or a literal can begin: `//`, `/*`, a quote, or a raw
# string's `r"`, `r#"` and so on, with or without `b`, at a word's start.
LITERAL_START = re.compile(r'//|/\*|"|\'|(?<![A-Za-z0-9_])b?r(#*)"')
BLOCK_COMMENT_MARK = re.compile(r"/\*|\*/")


def code_mask(text: str) -> list:
    """Whether each character of `text` is code: False inside a comment,
    a string literal or a character literal, so a brace or a `#[cfg` in
    one of them is not read as structure. Block comments nest, as Rust's
    do, and a lifetime is code."""
    n = len(text)
    mask = [True] * n
    i = 0
    while True:
        m = LITERAL_START.search(text, i)
        if not m:
            break
        i, token = m.start(), m.group(0)
        j = None
        if token == "//":
            j = text.find("\n", i)
            if j < 0:
                j = n
        elif token == "/*":
            depth, j = 1, i + 2
            while depth:
                mark = BLOCK_COMMENT_MARK.search(text, j)
                if not mark:
                    j = n
                    break
                depth += 1 if mark.group(0) == "/*" else -1
                j = mark.end()
        elif token.endswith('"') and len(token) > 1:
            close = '"' + m.group(1)
            k = text.find(close, m.end())
            j = n if k < 0 else k + len(close)
        elif token == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            j = min(j + 1, n)
        elif text.startswith("\\'", i + 1):
            j = i + 4
        elif i + 1 < n and text[i + 1] == "\\":
            k = text.find("'", i + 2)
            j = n if k < 0 else k + 1
        elif i + 2 < n and text[i + 2] == "'":
            j = i + 3
        if j is None:
            i += 1
            continue
        j = min(j, n)
        mask[i:j] = [False] * (j - i)
        i = max(j, i + 1)
    return mask


def strip_test_modules(text: str) -> str:
    """Blanks the body of every item whose `#[cfg(...)]` compiles it only
    for tests, leaving every offset where it was. Only code counts: a `#[cfg` or a
    brace in a comment or a literal is text."""
    code = code_mask(text)
    out = list(text)
    for m in re.finditer(r"#\[cfg\s*\(", text):
        if not code[m.start()]:
            continue
        parsed = cfg_predicate(text, m.end() - 1)
        if parsed is None or not test_only(parsed[0]):
            continue
        i = text.find("{", parsed[1])
        while i >= 0 and not code[i]:
            i = text.find("{", i + 1)
        if i < 0:
            continue
        depth, j = 0, i
        while j < len(text):
            if code[j] and text[j] == "{":
                depth += 1
            elif code[j] and text[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        for k in range(m.start(), min(j + 1, len(out))):
            if out[k] != "\n":
                out[k] = " "
    return "".join(out)


def pascal(snake: str) -> str:
    return "".join(p[:1].upper() + p[1:] for p in snake.split("_") if p)


def read(p: Path) -> str:
    return p.read_text(encoding="utf-8", errors="replace")


def rust_sources():
    out = []
    for group in ("crates", "examples"):
        for crate in sorted((ROOT / group).iterdir()):
            src = crate / "src"
            if src.is_dir():
                out += [p for p in sorted(src.rglob("*.rs")) if not p.name.endswith("_tests.rs")]
    return out


def main() -> int:
    list_only = "--list" in sys.argv

    sources = rust_sources()
    cleaned = {p: strip_test_modules(read(p)) for p in sources}
    production = "\n".join(cleaned.values())

    # Anything a script or the managed side can name.
    managed = []
    for pattern in ("**/*.cs", "**/*.ps1", "**/*.psd1", "**/*.psm1"):
        for p in ROOT.rglob(pattern):
            if "target" in p.parts or ".git" in p.parts:
                continue
            managed.append(read(p))
    managed_text = "\n".join(managed)

    defined, attribute_wired = {}, set()
    for p, text in cleaned.items():
        for m in DEF.finditer(text):
            defined.setdefault(m.group(1), set()).add(p)
            # The attributes sit on the lines just above the fn.
            head = text[max(0, m.start() - 200) : m.start()]
            if WIRED_BY_ATTRIBUTE.search(head) or WIRED_BY_MARK.search(own_attributes(text, m.start())):
                attribute_wired.add(m.group(1))

    unreached, support = [], []
    for name in sorted(defined):
        if name in EXEMPT or name in attribute_wired:
            continue
        word = r"\b" + re.escape(name) + r"\b"
        uses = len(re.findall(word, production)) - len(re.findall(r"\bfn\s+" + re.escape(name) + r"\b", production))
        if uses > 0:
            continue
        # A cmdlet, a proxy method or a completer is called by name
        # from PowerShell or from the generated C#, in PascalCase.
        if re.search(r"\b" + re.escape(pascal(name)) + r"\b", managed_text):
            continue
        where = sorted(str(w.relative_to(ROOT)).replace("\\", "/") for w in defined[name])
        row = (name, where)
        (support if any(w in TEST_SUPPORT for w in where) else unreached).append(row)

    print(f"{len(defined)} functions defined in production code")
    if support:
        print(f"\n{len(support)} reachable only from tests, in test-support modules by design:")
        for name, where in support:
            print(f"  {name:<32} {', '.join(where)}")
    if unreached:
        print(f"\n{len(unreached)} WITH NO CALLER IN PRODUCTION, C# OR POWERSHELL:")
        for name, where in unreached:
            print(f"  {name:<32} {', '.join(where)}")
        if not list_only:
            print("\nEither wire it into the module surface or delete it.")
            return 1
    else:
        print("\nEvery function has a caller in production, C# or PowerShell, or an attribute the generated code calls it through.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
