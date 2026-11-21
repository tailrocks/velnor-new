#!/usr/bin/env python3
import re
import sys
from pathlib import Path

IDENT = re.compile(r"[^\W\d]\w*")
FENCE = re.compile(r"```|~~~")
LINE_DOC = re.compile(r"//(?:!|/(?!/))")
CONTAINER = r"(?:>[ \t]*|(?:[-*+]|\d{1,9}[.)])[ \t]+(?:\[[ xX]\][ \t]+)?|\[\^[^\]\s]+\]:[ \t]*)*"
MARKER = r"[ \t]*(?:>|(?:[-*+]|\d{1,9}[.)])(?=[ \t])|\[\^[^\]\s]+\]:)"
LINE_FENCE = re.compile(r"[ \t]*" + CONTAINER + r"(?:```|~~~)")
LINE_INDENT = re.compile(r"(?:" + MARKER + r")* {5,}\S")
ATTR_INDENT = re.compile(r" {4,}\S|(?:" + MARKER + r")+ {5,}\S")
BLOCK_FENCE = re.compile(r"^[ \t]*\*?[ \t]*" + CONTAINER + r"(?:```|~~~)", re.M)
BLOCK_INDENT = re.compile(r"^[ \t]*\*?(?:" + MARKER + r")* {5,}\S", re.M)
DOC_OPEN = re.compile(r"/\*(?:!|\*(?![*/]))")
DOC_EQ = re.compile(r"\s*=(?!=)")
DOC_INCLUDE = re.compile(r"\s*=\s*include_str!")
STRING_ESCAPE = re.compile(r"\\(?:x([0-9a-fA-F]{2})|u\{([0-9a-fA-F_]{1,8})\}|(\r?\n[ \t\r\n]*)|(.))", re.S)
SIMPLE_ESCAPE = {"n": "\n", "r": "\r", "t": "\t", "0": "\0", "\\": "\\", "'": "'", '"': '"'}
RAW_PREFIX = {"r", "br", "cr"}
BYTE_PREFIX = {"b", "c"}
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


def skip_trivia(src, i):
    while i < len(src):
        if src[i].isspace():
            i += 1
        elif src.startswith("/*", i):
            i = block_end(src, i)
        elif src.startswith("//", i):
            k = src.find("\n", i)
            i = len(src) if k < 0 else k
        else:
            break
    return i


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


def attr_open(src, i):
    k = skip_trivia(src, i + 1)
    if src.startswith("!", k):
        k = skip_trivia(src, k + 1)
    return k + 1 if src.startswith("[", k) else None


def expand_tabs(text):
    out = []
    col = 0
    for ch in text:
        if ch == "\t":
            width = 4 - col % 4
            out.append(" " * width)
            col += width
        else:
            out.append(ch)
            col += 1
    return "".join(out)


def line_indent_text(body):
    # rustdoc drops one leading space or tab after `///`; the tabs left after that
    # expand to the next 4-column stop, not to a fixed four spaces each.
    if body[:1] in (" ", "\t"):
        return " " + expand_tabs(body[1:])
    return expand_tabs(body)


def value_has_fence(value):
    return any(
        LINE_FENCE.match(line) or ATTR_INDENT.match(line.replace("\t", "    "))
        for line in re.split(r"\r\n|\r|\n", value)
    )


def decode_escape(match):
    hex_digits, unicode_digits, continuation, simple = match.group(1, 2, 3, 4)
    if hex_digits:
        return chr(int(hex_digits, 16))
    if unicode_digits:
        return chr(int(unicode_digits.replace("_", ""), 16))
    if continuation is not None:
        return ""
    return SIMPLE_ESCAPE.get(simple, match.group(0))


def plain_value(literal):
    return STRING_ESCAPE.sub(decode_escape, literal[1:-1])


def raw_value(literal, prefix):
    hashes = literal.index('"') - prefix
    return literal[hashes + prefix + 1 : len(literal) - hashes - 1]


def doc_findings(src):
    i = 0
    attr_depth = 0
    doc_value = False
    while i < len(src):
        c = src[i]
        if src.startswith("//", i):
            k = src.find("\n", i)
            end = len(src) if k < 0 else k
            if LINE_DOC.match(src, i):
                body = src[i + 3 : end]
                if LINE_FENCE.match(body) or LINE_INDENT.match(line_indent_text(body)):
                    yield i
            i = end
        elif src.startswith("/*", i):
            end = block_end(src, i)
            if DOC_OPEN.match(src, i):
                body = src[i + 3 : end]
                if BLOCK_FENCE.search(body) or BLOCK_INDENT.search(body.replace("\t", "    ")):
                    yield i
            i = end
        elif c == '"':
            end = skip_string(src, i)
            if doc_value and value_has_fence(plain_value(src[i:end])):
                yield i
            i = end
        elif c == "'":
            j = skip_char(src, i)
            i = i + 1 if j is None else j
        elif c == "#" and (j := attr_open(src, i)) is not None:
            attr_depth = 1
            i = j
        elif c == "[":
            if attr_depth:
                attr_depth += 1
            i += 1
        elif c == "]":
            if attr_depth:
                attr_depth -= 1
            doc_value = doc_value and attr_depth > 0
            i += 1
        elif m := IDENT.match(src, i):
            word = m.group()
            j = i + len(word)
            if word in RAW_PREFIX and j < len(src) and src[j] in '#"':
                end = skip_raw(src, j - 1)
                if end is None:
                    i = j
                else:
                    if doc_value and value_has_fence(raw_value(src[i:end], len(word))):
                        yield i
                    i = end
            elif word in BYTE_PREFIX and j < len(src) and src[j] == '"':
                i = skip_string(src, j)
            elif word == "include_str" and doc_value:
                yield i
                i = j
            else:
                if word == "doc":
                    k = skip_trivia(src, j)
                    if DOC_INCLUDE.match(src, k):
                        yield i
                    elif attr_depth and DOC_EQ.match(src, k):
                        doc_value = True
                i = j
        else:
            i += 1


def rust_sources():
    return sorted(p for p in Path("crates").rglob("*.rs") if "src" in p.parts)


def strip_shebang(src):
    src = src.removeprefix("\ufeff")
    if src.startswith("#!") and attr_open(src, 0) is None:
        first, newline, rest = src.partition("\n")
        return newline + rest
    return src


def main(args):
    paths = [Path(a) for a in args] or rust_sources()
    bad = 0
    for path in paths:
        try:
            src = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            print(f"{path}: not valid UTF-8; cannot scan for doctests")
            bad += 1
            continue
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
