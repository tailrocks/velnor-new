#!/usr/bin/env python3
import re
import sys
from pathlib import Path

IDENT = re.compile(r"[^\W\d]\w*")
FENCE = re.compile(r"```|~~~")
INDENTED = re.compile(r"^[ \t]*\*? {5,}\S", re.M)
DOC_OPEN = re.compile(r"/\*(?:!|\*(?![*/]))")
RAW_PREFIX = {"r", "br", "cr"}
BYTE_PREFIX = {"b", "c"}


def block_end(src, i):
    depth = 0
    while i < len(src):
        if src.startswith("/*", i):
            depth += 1
            i += 2
        elif src.startswith("*/", i):
            depth -= 1
            i += 2
            if depth == 0:
                return i
        else:
            i += 1
    return len(src)


def skip_string(src, i):
    i += 1
    while i < len(src):
        if src[i] == "\\":
            i += 2
        elif src[i] == '"':
            return i + 1
        else:
            i += 1
    return len(src)


def skip_raw(src, i):
    j = i + 1
    while j < len(src) and src[j] == "#":
        j += 1
    if j >= len(src) or src[j] != '"':
        return None
    close = '"' + "#" * (j - i - 1)
    k = src.find(close, j + 1)
    return len(src) if k < 0 else k + len(close)


def skip_char(src, i):
    if i + 1 < len(src) and src[i + 1] == "\\":
        k = src.find("'", i + 3)
        return k + 1 if k >= 0 else None
    if i + 2 < len(src) and src[i + 2] == "'":
        return i + 3
    return None


def doc_blocks(src):
    i = 0
    while i < len(src):
        c = src[i]
        if src.startswith("//", i):
            k = src.find("\n", i)
            i = len(src) if k < 0 else k
        elif src.startswith("/*", i):
            end = block_end(src, i)
            text = src[i:end]
            if DOC_OPEN.match(text):
                yield src.count("\n", 0, i) + 1, text[3:]
            i = end
        elif c == '"':
            i = skip_string(src, i)
        elif c == "'":
            j = skip_char(src, i)
            i = i + 1 if j is None else j
        elif (m := IDENT.match(src, i)):
            word = m.group()
            j = i + len(word)
            if word in RAW_PREFIX and j < len(src) and src[j] in '#"':
                end = skip_raw(src, j - 1)
                i = j if end is None else end
            elif word in BYTE_PREFIX and j < len(src) and src[j] == '"':
                i = skip_string(src, j)
            else:
                i = j
        else:
            i += 1


def rust_sources():
    return sorted(p for p in Path("crates").rglob("*.rs") if "src" in p.parts)


def is_shebang(src):
    if not src.startswith("#!"):
        return False
    i = 2
    while i < len(src):
        if src[i].isspace():
            i += 1
        elif src.startswith("/*", i):
            i = block_end(src, i)
        elif src.startswith("//", i):
            end = src.find("\n", i)
            if end < 0:
                return True
            i = end
        else:
            return src[i] != "["
    return True


def strip_shebang(src):
    src = src.removeprefix("\ufeff")
    if is_shebang(src):
        first, newline, rest = src.partition("\n")
        return newline + rest
    return src


def main(args):
    paths = [Path(a) for a in args] or rust_sources()
    bad = 0
    for path in paths:
        src = path.read_text(encoding="utf-8")
        if "/**" not in src and "/*!" not in src:
            continue
        src = strip_shebang(src)
        for line, body in doc_blocks(src):
            if FENCE.search(body) or INDENTED.search(body):
                print(f"{path}:{line}: doctest form inside a block doc comment")
                bad += 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
