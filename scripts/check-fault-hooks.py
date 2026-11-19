#!/usr/bin/env python3
"""Guard: no test seam is reachable from a non-test build of hf-host, and the reader policy
constructors that weaken capture backpressure are used only where they are allowed."""
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "crates/hf-host/src"
ROOTS = [SRC / "lib.rs", SRC / "main.rs", SRC / "ctl/main.rs"]
WORDS = re.compile(r"seams|for_test|ThreadWriter|ScriptedSource|ArmedFlush|WriterSlot::Thread")
TEST_ONLY_FILES = ("task_runtime/seams.rs", "task_runtime/tests/")
MOD = re.compile(r"^(\s*)((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*(;|\{)")
MOD_WORD = re.compile(r"(?:^|[\s\]])mod\s+\w+")
PATH_ATTR = re.compile(r'#\[path\s*=\s*"([^"]+)"\]')


def cfg_lines(text):
    return text.splitlines()


def is_cfg_test(line):
    return line.strip() == "#[cfg(test)]"


def top_level_terminators(text):
    """Count `;` and closing `}` that end a statement or item at nesting depth zero."""
    depth, count = 0, 0
    for ch in re.sub(r'"(\\.|[^"\\])*"|//.*', "", text):
        if ch in "({[":
            depth += 1
        elif ch in ")}]":
            depth -= 1
            if depth == 0 and ch == "}":
                count += 1
        elif ch == ";" and depth == 0:
            count += 1
    return count


def same_line_cfg_test(line):
    """`#[cfg(test)] item;` covering exactly one declaration on its line."""
    s = line.strip()
    prefix = "#[cfg(test)] "
    return s.startswith(prefix) and top_level_terminators(s[len(prefix):]) <= 1


def mentions_test_cfg(line):
    s = line.strip()
    if same_line_cfg_test(line):
        return False
    return s.startswith("#[cfg") and not s.startswith("#[cfg_attr") and re.search(r"\btest\b", s)


def covered_spans(lines):
    """Line indexes covered by an item or statement that directly follows #[cfg(test)]."""
    covered = set()
    for i, line in enumerate(lines):
        if same_line_cfg_test(line):
            covered.add(i)
        if not is_cfg_test(line):
            continue
        depth, j, started = 0, i + 1, False
        while j < len(lines):
            covered.add(j)
            for ch in re.sub(r'"(\\.|[^"\\])*"|//.*', "", lines[j]):
                if ch in "({[":
                    depth += 1
                    started = True
                elif ch in ")}]":
                    depth -= 1
            tail = lines[j].rstrip()
            if depth <= 0 and (tail.endswith(";") or tail.endswith("}") or tail.endswith(",")):
                break
            if depth <= 0 and not started and tail.endswith(")"):
                break
            j += 1
        covered.add(i)
    return covered


def check_file(path, text, problems):
    lines = cfg_lines(text)
    covered = covered_spans(lines)
    for i, line in enumerate(lines):
        if mentions_test_cfg(line) and not is_cfg_test(line):
            problems.append(f"{path}:{i + 1}: cfg mentioning test other than #[cfg(test)]")
        code = re.sub(r"//.*", "", line)
        if WORDS.search(code) and i not in covered:
            problems.append(f"{path}:{i + 1}: seam word outside a cfg(test) item")
        if re.search(r"\binclude!|macro_rules!", code) and i not in covered:
            problems.append(f"{path}:{i + 1}: include!/macro_rules! in a reachable file")
        if re.search(r"use\s+[^;]*seams", code) or re.search(r"seams[^;]*\bas\b", code):
            problems.append(f"{path}:{i + 1}: seam import or alias")


def attributes_above(lines, index):
    """The consecutive attribute lines directly above line `index`."""
    found, i = [], index - 1
    while i >= 0 and lines[i].strip().startswith("#["):
        found.append(lines[i].strip())
        i -= 1
    return found


def resolve(path, name, attrs, read):
    """The file a `mod name;` declared in `path` loads, or None."""
    for attr in attrs:
        if m := PATH_ATTR.search(attr):
            return path.parent / m.group(1)
    base = path.parent if path.name in ("lib.rs", "main.rs", "mod.rs") else path.parent / path.stem
    for cand in (base / f"{name}.rs", base / name / "mod.rs"):
        if read(cand) is not None:
            return cand
    return None


def reachable(roots, read, problems=None):
    """Files reachable from the non-test roots. Declarations it cannot resolve, or that sit
    inside an inline module (whose path rules it does not model), are reported."""
    problems = problems if problems is not None else []
    seen, queue = {}, list(roots)
    while queue:
        path = queue.pop()
        if path in seen:
            continue
        text = read(path)
        if text is None:
            problems.append(f"{path.as_posix()}: root or module file is missing")
            continue
        seen[path] = text
        lines = cfg_lines(text)
        for i, line in enumerate(lines):
            m = MOD.match(line)
            if not m:
                if MOD_WORD.search(re.sub(r"//.*", "", line)):
                    problems.append(f"{path.as_posix()}:{i + 1}: unrecognized module declaration")
                continue
            trailing = re.sub(r"//.*", "", line[m.end():]).strip() if m.group(4) == ";" else ""
            if trailing:
                problems.append(f"{path.as_posix()}:{i + 1}: code after a module declaration")
            attrs = attributes_above(lines, i) + re.findall(r"#\[[^\]]*\]", m.group(2))
            if any(is_cfg_test(a) for a in attrs):
                continue
            if m.group(4) == "{":
                problems.append(f"{path.as_posix()}:{i + 1}: inline module is not modelled")
                continue
            if m.group(1):
                problems.append(f"{path.as_posix()}:{i + 1}: nested module declaration")
                continue
            target = resolve(path, m.group(3), attrs, read)
            if target is None:
                problems.append(f"{path.as_posix()}:{i + 1}: module {m.group(3)} does not resolve")
            else:
                queue.append(target)
    return seen


def source_check(read=lambda p: p.read_text() if p.exists() else None, roots=ROOTS):
    problems = []
    seen = reachable(roots, read, problems)
    for path, text in seen.items():
        rel = path.as_posix()
        if any(t in rel for t in TEST_ONLY_FILES):
            problems.append(f"{rel}: test-only file is reachable from a non-test root")
        check_file(rel, text, problems)
    return problems


POLICY_DIRS = ("crates/hf-host/src", "crates/hf-host/tests", "crates/hf-platform/src", "crates/hf-platform/tests")
READER_MODULE = (
    "crates/hf-platform/src/process/reader.rs",
    "crates/hf-platform/src/process/reader_loop.rs",
    "crates/hf-platform/src/process/reader_queue.rs",
)
READER_LOOP = "crates/hf-platform/src/process/reader_loop.rs"
READER_QUEUE = "crates/hf-platform/src/process/reader_queue.rs"
HOST_SEAMS = "crates/hf-host/src/task_runtime/seams.rs"
LITERAL = re.compile(r"ReaderPolicy\s*\{\s*\w+\s*:(?!:)")
CONSTRUCTOR = re.compile(r"ReaderPolicy\s*::\s*(\w+)")
ALIAS = re.compile(r"\bReaderPolicy\s+as\s+(\w+)")


def policy_files(root=ROOT):
    return [p for d in POLICY_DIRS for p in sorted((root / d).rglob("*.rs"))]


def strip_comments(text):
    return re.sub(r"//.*", "", text)


def enclosing_fn(text, offset):
    names = re.findall(r"\bfn\s+(\w+)", text[:offset])
    return names[-1] if names else None


SHORT_WRITE = re.compile(r"\bwrite_retained_cause_capped\b")
SHORT_WRITE_HOME = "crates/hf-platform/src/task_runtime/raw/cause.rs"
SHORT_WRITE_HOST = "crates/hf-host/src/task_runtime/owner_retain.rs"


def policy_check(read, files, root=ROOT):
    """Token rules for `ReaderPolicy`: `with_stall` and the fixture policy only where tests
    and the reader module need them, a literal only in the reader module, and nothing but
    `production()` in non-test host code. The check is textual and conservative."""
    problems = []
    for path in files:
        rel = path.relative_to(root).as_posix() if path.is_absolute() and root in path.parents else path.as_posix()
        text = read(path)
        if text is None:
            continue
        code = strip_comments(text)
        host = rel.startswith("crates/hf-host/")
        in_tests = "/tests/" in rel
        names = {"ReaderPolicy", *ALIAS.findall(code)}
        mentions = any(re.search(rf"\b{re.escape(n)}\b", code) for n in names)
        stall = re.search(r"\bwith_stall\b", code)
        if stall and not in_tests:
            allowed = rel == HOST_SEAMS if host else rel in READER_MODULE
            if not allowed:
                problems.append(f"{rel}: with_stall outside the reader module, seams and tests")
        if LITERAL.search(code) and rel != READER_QUEUE:
            problems.append(f"{rel}: ReaderPolicy literal outside the reader module")
        if mentions and re.search(r"\bfixture\s*\(", code) and not in_tests and rel not in (READER_LOOP, READER_QUEUE):
            problems.append(f"{rel}: fixture reader policy outside the fixture reader entry")
        if host and not in_tests and rel != HOST_SEAMS:
            for m in re.finditer(r"\b(\w+)\s*::\s*(\w+)", code):
                if m.group(1) in names and m.group(2) != "production":
                    problems.append(f"{rel}: host code may only build ReaderPolicy::production()")
        if SHORT_WRITE.search(code) and not in_tests and rel != SHORT_WRITE_HOME:
            if rel != SHORT_WRITE_HOST:
                problems.append(f"{rel}: write_retained_cause_capped outside its module, the host seam and tests")
            else:
                lines = text.splitlines()
                covered = covered_spans(lines)
                for i, line in enumerate(lines):
                    if SHORT_WRITE.search(re.sub(r"//.*", "", line)) and i not in covered:
                        problems.append(f"{rel}:{i + 1}: write_retained_cause_capped outside a cfg(test) item")
        if rel == READER_LOOP:
            for m in CONSTRUCTOR.finditer(code):
                if not (m.group(1) == "fixture" and enclosing_fn(code, m.start()) == "start"):
                    problems.append(f"{rel}: only the fixture entry may build a policy here ({m.group(0)})")
    return problems


def scan_symbols(listing):
    return [l for l in listing.splitlines() if re.search(r"5seams|\bseams\b|__ZN[0-9a-zA-Z_$.]*seams|[A-Za-z_]seams", l)]


def nm(path):
    return subprocess.run(["nm", "-a", str(path)], capture_output=True, text=True, check=True).stdout


def binary_check(ctl, scaffold, control):
    problems = []
    for binary, pinned in ((ctl, "run_writer"), (scaffold, "owner_runtime")):
        listing = nm(binary)
        hits = scan_symbols(listing)
        if hits:
            problems.append(f"{binary}: seam symbols: {hits[:3]}")
        if pinned not in listing:
            problems.append(f"{binary}: production control symbol {pinned} missing")
    if not scan_symbols(nm(control)):
        problems.append("control binary: seam symbol not detected")
    return problems


if __name__ == "__main__":
    found = source_check() + policy_check(lambda p: p.read_text() if p.exists() else None, policy_files())
    for line in found:
        print(line)
    sys.exit(1 if found else 0)
