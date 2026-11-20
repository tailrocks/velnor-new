#!/usr/bin/env python3
import re
import sys
from pathlib import Path

IDENT = re.compile(r"[^\W\d]\w*")
FENCE = re.compile(r"```|~~~")
LINE_DOC = re.compile(r"//(?:!|/(?!/))")
LINE_FENCE = re.compile(r"[ \t]*(?:```|~~~)")
LINE_INDENT = re.compile(r" {5,}\S")
BLOCK_FENCE = re.compile(r"^[ \t]*\*?[ \t]*(?:```|~~~)", re.M)
BLOCK_INDENT = re.compile(r"^[ \t]*\*? {5,}\S", re.M)
DOC_OPEN = re.compile(r"/\*(?:!|\*(?![*/]))")
ATTR_DOC = re.compile(r"#!?\[doc\s*=")
DOC_INCLUDE = re.compile(r"\s*=\s*include_str!")
RAW_PREFIX = {"r", "br", "cr"}
BYTE_PREFIX = {"b", "c"}
SHEBANG_ATTR = re.compile(r"#!\s*\[")
DOC_HINT = ("///", "//!", "/**", "/*!", "doc")


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


def doc_findings(src):
    i = 0
    in_attr = False
    while i < len(src):
        c = src[i]
        if src.startswith("//", i):
            k = src.find("\n", i)
            end = len(src) if k < 0 else k
            if LINE_DOC.match(src, i):
                body = src[i + 3 : end]
                if LINE_FENCE.match(body) or LINE_INDENT.match(body):
                    yield i
            i = end
        elif src.startswith("/*", i):
            end = block_end(src, i)
            if DOC_OPEN.match(src, i):
                body = src[i + 3 : end]
                if BLOCK_FENCE.search(body) or BLOCK_INDENT.search(body):
                    yield i
            i = end
        elif c == '"':
            end = skip_string(src, i)
            if in_attr and FENCE.search(src, i, end):
                yield i
            i = end
        elif c == "'":
            j = skip_char(src, i)
            i = i + 1 if j is None else j
        elif c == "#" and ATTR_DOC.match(src, i):
            in_attr = True
            i += 1
        elif c == "]":
            in_attr = False
            i += 1
        elif m := IDENT.match(src, i):
            word = m.group()
            j = i + len(word)
            if word in RAW_PREFIX and j < len(src) and src[j] in '#"':
                end = skip_raw(src, j - 1)
                i = j if end is None else end
            elif word in BYTE_PREFIX and j < len(src) and src[j] == '"':
                i = skip_string(src, j)
            else:
                if word == "doc" and DOC_INCLUDE.match(src, j):
                    yield i
                i = j
        else:
            i += 1


def rust_sources():
    return sorted(p for p in Path("crates").rglob("*.rs") if "src" in p.parts)


def strip_shebang(src):
    src = src.removeprefix("﻿")
    if src.startswith("#!") and not SHEBANG_ATTR.match(src):
        first, newline, rest = src.partition("\n")
        return newline + rest
    return src


def main(args):
    paths = [Path(a) for a in args] or rust_sources()
    bad = 0
    for path in paths:
        src = path.read_text(encoding="utf-8")
        if not any(hint in src for hint in DOC_HINT):
            continue
        src = strip_shebang(src)
        for pos in doc_findings(src):
            line = src.count("\n", 0, pos) + 1
            print(f"{path}:{line}: doctest form in a doc comment or doc attribute")
            bad += 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
