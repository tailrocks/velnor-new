#!/usr/bin/env python3
import re
import sys
from pathlib import Path

IDENT = re.compile(r"[^\W\d]\w*")
FENCE = re.compile(r"```|~~~")
LINE_DOC = re.compile(r"//(?:!|/(?!/))")
QUOTE = r"(?:[ \t]*>)*"
CONTAINER = r"(?:>[ \t]*|(?:[-*+]|\d{1,9}[.)])(?:[ \t]+(?:\[[ xX]\][ \t]+)?|$)|\[\^[^\]\s]+\]:[ \t]*)*"
LINE_FENCE = re.compile(r"[ \t]*" + CONTAINER + r"(?:```|~~~)")
LINE_INDENT = re.compile(QUOTE + r" {5,}\S")
ATTR_INDENT = re.compile(r" {4,}\S|(?:[ \t]*>)+ {5,}\S")
LIST_INDENT = re.compile(r"[ \t]*" + CONTAINER + r"((?:[-*+]|\d{1,9}[.)])) {5,}\S")
ORDERED_NO_INTERRUPT = re.compile(r"(?!0*1[.)])\d{1,9}[.)]")
BLOCK_MARGIN = re.compile(r"^[ \t]*\*")
QUOTE_START = re.compile(r"[ \t]*>")
SETEXT_UNDERLINE = re.compile(r"(?:=+|-+)[ \t]*")
HTML_CLOSED = re.compile(
    r"<!--.*-->|<\?.*\?>|<!\[CDATA\[.*\]\]>|<![A-Za-z].*>"
    r"|<(script|pre|style|textarea)(?:\s|>|$).*</\1\s*>",
    re.I,
)
HRULE = re.compile(r"(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,}")
DOC_OPEN = re.compile(r"/\*(?:!|\*(?![*/]))")
NEWLINE = re.compile(r"\r\n|\r|\n")
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


def attr_open(src, i):
    j = skip_trivia(src, i + 1)
    if j < len(src) and src[j] == "!":
        j = skip_trivia(src, j + 1)
    return j + 1 if j < len(src) and src[j] == "[" else None


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
        and not SETEXT_UNDERLINE.fullmatch(text)
        and not HTML_CLOSED.match(text)
    )


def split_lines(text):
    return NEWLINE.split(text)


def doc_text_tabs(text):
    if text[:1] in (" ", "\t"):
        return " " + text[1:].expandtabs(4)
    return text.expandtabs(4)


def markdown_hit(lines, indent):
    prev = None
    for k, raw in enumerate(lines):
        line = raw.expandtabs(4)
        lazy = prose(prev) and not QUOTE_START.match(line)
        item = LIST_INDENT.match(line)
        if (item and not (lazy and ORDERED_NO_INTERRUPT.fullmatch(item.group(1)))) or (
            LINE_FENCE.match(line) and not (lazy and indent.match(line))
        ):
            return k
        if indent.match(line) and not lazy:
            return k
        prev = line
    return None


def value_has_fence(value):
    lines = [line.expandtabs(4) for line in split_lines(value)]
    margin = min((len(line) - len(line.lstrip(" ")) for line in lines if line.strip()), default=0)
    lines = [line[margin:] for line in lines]
    return markdown_hit(lines, ATTR_INDENT) is not None


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
        run.append((pos, doc_text_tabs(src[pos + 3 : end])))
        j = end + 1
        while j < len(src) and src[j] in " \t":
            j += 1
        if not joinable or end >= len(src) or not LINE_DOC.match(src, j):
            return run, end
        pos = j


def doc_findings(src):
    i = 0
    attr_depth = 0
    attr_name = ""
    paren = 0
    cfg_comma = False
    doc_paren = 0
    doc_value = False
    while i < len(src):
        c = src[i]
        if src.startswith("//", i):
            if LINE_DOC.match(src, i):
                run, end = line_run(src, i)
                hit = markdown_hit([body for _, body in run], LINE_INDENT)
                if hit is not None:
                    yield run[hit][0]
            else:
                k = src.find("\n", i)
                end = len(src) if k < 0 else k
            i = end
        elif src.startswith("/*", i):
            end = block_end(src, i)
            if DOC_OPEN.match(src, i):
                start = i + 3
                lines = split_lines(src[start:end])
                stripped = [doc_text_tabs(BLOCK_MARGIN.sub("", line, count=1)) for line in lines]
                hit = markdown_hit(stripped, LINE_INDENT)
                if hit is not None:
                    yield start + sum(len(line) + 1 for line in lines[:hit])
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
            paren = 0
            cfg_comma = False
            k = skip_trivia(src, j)
            k += 2 if src.startswith("r#", k) else 0
            name = IDENT.match(src, k)
            attr_name = name.group() if name else ""
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
        elif c in "(),":
            if attr_depth and c == "(":
                paren += 1
            elif attr_depth and c == ")":
                paren -= 1
                doc_value = doc_value and paren >= doc_paren
            elif attr_depth and c == ",":
                doc_value = doc_value and paren > doc_paren
                if attr_name == "cfg_attr" and paren == 1:
                    cfg_comma = True
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
            elif doc_value and src.startswith("!", skip_trivia(src, j)) and word != "concat":
                yield i
                i = j
            else:
                if word == "doc":
                    k = skip_trivia(src, j)
                    if DOC_INCLUDE.match(src, k):
                        yield i
                    elif DOC_EQ.match(src, k) and (
                        (attr_name == "doc" and paren == 0) or (attr_name == "cfg_attr" and paren == 1 and cfg_comma)
                    ):
                        doc_value = True
                        doc_paren = paren
                i = j
        else:
            i += 1


def rust_sources():
    return sorted(p for p in Path("crates").rglob("*.rs") if "src" in p.parts)


def strip_shebang(src):
    src = src.removeprefix("\ufeff")
    if src.startswith("#!") and not src.startswith("[", skip_trivia(src, 2)):
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
