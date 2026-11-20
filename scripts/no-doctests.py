#!/usr/bin/env python3
import re
import sys
from pathlib import Path

IDENT = re.compile(r"[^\W\d]\w*")
FENCE = re.compile(r"```|~~~")
LINE_DOC = re.compile(r"//(?:!|/(?!/))")
QUOTE = r"(?:[ \t]*>)*"
CONTAINER = r"(?:>[ \t]*|(?:[-*+]|\d{1,9}[.)])[ \t]+|\[\^[^\]]+\]:[ \t]*)*"
LINE_FENCE = re.compile(r"[ \t]*" + CONTAINER + r"(?:```|~~~)")
LINE_INDENT = re.compile(QUOTE + r" {5,}\S")
ATTR_INDENT = re.compile(r" {4,}\S|(?:[ \t]*>)+ {5,}\S")
LIST_INDENT = re.compile(r"[ \t]*" + CONTAINER + r"(?:[-*+]|\d{1,9}[.)]) {5,}\S")
BLOCK_MARGIN = re.compile(r"[ \t]*\*?")
BLOCK_STAR = re.compile(r"[ \t]*\*")
QUOTE_START = re.compile(r"[ \t]*>")
HRULE = re.compile(r"(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,}")
SETEXT = re.compile(r"=+[ \t]*|(?:-[ \t]*){2,}")
DOC_OPEN = re.compile(r"/\*(?:!|\*(?![*/]))")
ATTR_START = re.compile(r"#\s*!?\s*\[")
DOC_EQ = re.compile(r"\s*=(?!=)")
DOC_INCLUDE = re.compile(r"\s*=\s*include_str!")
SHEBANG_ATTR = re.compile(r"#!\s*\[")
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


def prose(line):
    if line is None:
        return False
    text = re.sub("^" + CONTAINER, "", line.strip(), count=1)
    return (
        bool(text)
        and not text.startswith("#")
        and not HRULE.fullmatch(text)
        and not SETEXT.fullmatch(text)
    )


def markdown_hit(lines, indent, prev=None):
    for k, line in enumerate(lines):
        if LINE_FENCE.match(line) or LIST_INDENT.match(line):
            return k
        if indent.match(line) and not (prose(prev) and not QUOTE_START.match(line)):
            return k
        prev = line
    return None


def value_has_fence(value, prev=None):
    return markdown_hit(value.split("\n"), ATTR_INDENT, prev) is not None


def block_text(lines):
    rest = lines[1:]
    if all(not line.strip() or BLOCK_STAR.match(line) for line in rest):
        body = [BLOCK_MARGIN.sub("", line, count=1) for line in rest]
    else:
        text = [line for line in rest if line.strip()]
        margin = min((len(line) - len(line.lstrip(" \t")) for line in text), default=0)
        body = [" " + line[margin:] if line.strip() else "" for line in rest]
    return [BLOCK_MARGIN.sub("", lines[0], count=1)] + body


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


def line_run(src, i):
    run = []
    pos = i
    joinable = not src[src.rfind("\n", 0, i) + 1 : i].strip()
    while True:
        k = src.find("\n", pos)
        end = len(src) if k < 0 else k
        run.append((pos, src[pos + 3 : end]))
        j = end + 1
        while j < len(src) and src[j] in " \t":
            j += 1
        if not joinable or end >= len(src) or not LINE_DOC.match(src, j):
            return run, end
        pos = j


def doc_findings(src):
    i = 0
    attr_depth = 0
    paren = 0
    doc_paren = 0
    doc_value = False
    tail = None
    while i < len(src):
        c = src[i]
        if src.startswith("//", i):
            if LINE_DOC.match(src, i):
                run, end = line_run(src, i)
                bodies = [body for _, body in run]
                hit = markdown_hit(bodies, LINE_INDENT, tail)
                if hit is not None:
                    yield run[hit][0]
                tail = bodies[-1]
            else:
                k = src.find("\n", i)
                end = len(src) if k < 0 else k
            i = end
        elif src.startswith("/*", i):
            end = block_end(src, i)
            if DOC_OPEN.match(src, i):
                start = i + 3
                body = src[start:end]
                lines = (body[:-2] if body.endswith("*/") else body).split("\n")
                stripped = block_text(lines)
                hit = markdown_hit(stripped, LINE_INDENT, tail)
                if hit is not None:
                    yield start + sum(len(line) + 1 for line in lines[:hit])
                tail = stripped[-1]
            i = end
        elif c == '"':
            end = skip_string(src, i)
            if doc_value:
                value = plain_value(src[i:end])
                if value_has_fence(value, tail):
                    yield i
                tail = value.split("\n")[-1]
            i = end
        elif c == "'":
            j = skip_char(src, i)
            i = i + 1 if j is None else j
        elif c == "#" and (m := ATTR_START.match(src, i)):
            attr_depth = 1
            paren = 0
            i = m.end()
        elif c in "([{":
            if attr_depth:
                paren += 1
                if c == "[":
                    attr_depth += 1
            elif c == "{":
                tail = None
            i += 1
        elif c in ")]}":
            if attr_depth:
                paren -= 1
                if c == "]":
                    attr_depth -= 1
                doc_value = doc_value and attr_depth > 0 and paren >= doc_paren
            elif c == "}":
                tail = None
            i += 1
        elif c in ",;":
            if attr_depth:
                doc_value = doc_value and paren > doc_paren
            else:
                tail = None
            i += 1
        elif m := IDENT.match(src, i):
            word = m.group()
            j = i + len(word)
            if word in RAW_PREFIX and j < len(src) and src[j] in '#"':
                end = skip_raw(src, j - 1)
                if end is None:
                    i = j
                else:
                    if doc_value:
                        value = raw_value(src[i:end], len(word))
                        if value_has_fence(value, tail):
                            yield i
                        tail = value.split("\n")[-1]
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
                        doc_paren = paren
                i = j
        else:
            i += 1


def rust_sources():
    return sorted(p for p in Path("crates").rglob("*.rs") if "src" in p.parts)


def strip_shebang(src):
    src = src.removeprefix("\ufeff")
    if src.startswith("#!") and not SHEBANG_ATTR.match(src):
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
