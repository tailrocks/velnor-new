#!/usr/bin/env python3
"""Fail when a Rust file puts a doc fence inside a block doc comment.

Scans Rust tokens, not raw text: line comments, string and char literals are
skipped as units, and a block comment nests like rustc's, so a `/**` inside a
string literal is not a doc comment and a nested `/* */` does not end the
outer one.
"""
import re
import sys

IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
FENCE = re.compile(r"```|~~~")
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
        elif c.isalpha() or c == "_":
            word = IDENT.match(src, i).group()
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


def main(paths):
    bad = 0
    for path in paths:
        with open(path, encoding="utf-8") as f:
            src = f.read()
        for line, body in doc_blocks(src):
            if FENCE.search(body):
                print(f"{path}:{line}: doc fence inside a block doc comment")
                bad += 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
