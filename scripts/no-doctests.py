#!/usr/bin/env python3
import re
import sys
from pathlib import Path

IDENT = re.compile(r"[^\W\d]\w*")
FENCE = re.compile(r"```|~~~")
LINE_DOC = re.compile(r"//(?:!|/(?!/))")
QUOTE = r"(?:[ \t]*>)*"
FOOTNOTE = r"\[\^(?!\s*\])[^\]]*\]:"
CONTAINER = r"(?:>[ \t]*|(?:[-*+]|\d{1,9}[.)])(?:[ \t]+(?:\[[ xX]\][ \t]+)?|$)|" + FOOTNOTE + r"[ \t]*)*"
LINE_FENCE = re.compile(r"[ \t]*" + CONTAINER + r"(?:```|~~~)")
LINE_INDENT = re.compile(QUOTE + r" {5,}\S")
ATTR_INDENT = re.compile(r" {4,}\S|(?:[ \t]*>)+ {5,}\S")
LIST_INDENT = re.compile(r"[ \t]*" + CONTAINER + r"(?:[-*+]|\d{1,9}[.)]) {5,}\S")
BLOCK_MARGIN = re.compile(r"[ \t]*\*?")
BLOCK_STAR = re.compile(r"[ \t]*\*")
LIST_ITEM = re.compile(r" *(?:[-*+]|\d{1,9}[.)]) +\S")
QUOTE_START = re.compile(r"[ \t]*>")
HRULE = re.compile(r"(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,}")
SETEXT = re.compile(r"=+[ \t]*|-+[ \t]*")
ATX = re.compile(r"#{1,6}(?:[ \t]|$)")
FOOTNOTE_DEF = re.compile(r"[ \t]*" + FOOTNOTE)
CFG_DOC = re.compile(r",\s*doc\s*=")
HTML_DONE = re.compile(r"<!--.*-->|<\?.*\?>|<![A-Za-z].*>|<!\[CDATA\[.*\]\]>")
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
    return bool(text) and not ATX.match(text) and not HRULE.fullmatch(text) and not HTML_DONE.fullmatch(text)


def split_lines(text):
    return NEWLINE.split(text)


def doc_text_tabs(text):
    if text[:1] in (" ", "\t"):
        return " " + text[1:].expandtabs(4)
    return text.expandtabs(4)


def block_text(lines, normalize=True):
    rest = lines[1:]
    body_lines = rest[:]
    while body_lines and not body_lines[-1].strip():
        body_lines.pop()
    if all(BLOCK_STAR.match(line) for line in body_lines):
        body = [BLOCK_MARGIN.sub("", line, count=1) for line in rest]
    else:
        text = [line for line in rest if line.strip()]
        margin = min((len(line) - len(line.lstrip(" \t")) for line in text), default=0) if normalize else 0
        body = [" " + line[margin:] if line.strip() else "" for line in rest]
    return [BLOCK_MARGIN.sub("", lines[0], count=1)] + body


def unindent_run(bodies):
    texts = [body[1:] if body[:1] == " " else body for body in bodies]
    margin = min((len(text) - len(text.lstrip(" ")) for text in texts if text.strip()), default=0)
    return [" " + text[margin:] if text.strip() else "" for text in texts]


def carried(tail, inner):
    if tail is None or tail[0] != inner:
        return None, False, None
    return tail[1], tail[2], tail[3]


def markdown_hit(lines, indent, prev=None, item=None):
    for k, raw in enumerate(lines):
        line = raw.expandtabs(4)
        blank = not line.strip()
        width = len(line) - len(line.lstrip(" "))
        was = item
        if not blank and item is not None and width < item:
            item = None
        lazy = prose(prev) and not QUOTE_START.match(line)
        if LIST_INDENT.match(line) or (LINE_FENCE.match(line) and not (lazy and indent.match(line))):
            return k, item
        code = indent.match(line) if item is None or blank else width >= item + 4
        if code and not lazy:
            return k, item
        match = LIST_ITEM.match(line)
        if match and not HRULE.fullmatch(line.strip()):
            item = match.end() - 1
        elif FOOTNOTE_DEF.match(line):
            item = width + 4
        heading = prose(prev) and SETEXT.fullmatch(line.strip()) and not indent.match(line) and (was is None or width >= was)
        prev = "" if heading else line
    return None, item


def value_has_fence(value, prev=None, normalize=True, item=None):
    lines = [line.expandtabs(4) for line in split_lines(value)]
    if normalize:
        margin = min((len(line) - len(line.lstrip(" ")) for line in lines if line.strip()), default=0)
        lines = [line[margin:] for line in lines]
    hit, item = markdown_hit(lines, ATTR_INDENT, prev, item)
    return hit is not None, item


def has_following_doc(src, j, inner):
    while j < len(src):
        c = src[j]
        if c.isspace() or c in "])":
            j += 1
        elif src.startswith("//", j):
            if LINE_DOC.match(src, j):
                return src.startswith("//!", j) == inner
            k = src.find("\n", j)
            j = len(src) if k < 0 else k
        elif src.startswith("/*", j):
            if DOC_OPEN.match(src, j):
                return src.startswith("/*!", j) == inner
            j = block_end(src, j)
        elif c == "#" and (k := attr_open(src, j)) is not None:
            n = skip_trivia(src, k)
            n += 2 if src.startswith("r#", n) else 0
            name = IDENT.match(src, n)
            if name and name.group() == "doc":
                return src.startswith("!", skip_trivia(src, j + 1)) == inner
            m, depth = k, 1
            while m < len(src) and depth:
                if src[m] == '"':
                    m = skip_string(src, m)
                    continue
                if src[m] == "[":
                    depth += 1
                elif src[m] == "]":
                    depth -= 1
                m += 1
            if name and name.group() == "cfg_attr" and CFG_DOC.search(src, k, m):
                return src.startswith("!", skip_trivia(src, j + 1)) == inner
            j = m
        elif m := IDENT.match(src, j):
            j = m.end()
        else:
            return False
    return False


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
        text = src[pos + 3 : end]
        run.append((pos, doc_text_tabs(text), "\t" in text))
        j = end + 1
        while j < len(src) and src[j] in " \t":
            j += 1
        if not joinable or end >= len(src) or not LINE_DOC.match(src, j) or src.startswith("//!", j) != src.startswith("//!", i):
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
    attr_inner = False
    tail = None
    while i < len(src):
        c = src[i]
        if src.startswith("//", i):
            if LINE_DOC.match(src, i):
                run, end = line_run(src, i)
                bodies = [body for _, body, _ in run]
                inner = src.startswith("//!", i)
                prev, seen, item = carried(tail, inner)
                tabbed = any(tab for _, _, tab in run)
                standalone = not seen and not tabbed and not has_following_doc(src, end, inner)
                hit, item = markdown_hit(unindent_run(bodies) if standalone else bodies, LINE_INDENT, prev, item)
                if hit is not None:
                    yield run[hit][0]
                tail = (inner, bodies[-1], seen or any(body.strip() for body in bodies), item)
            else:
                k = src.find("\n", i)
                end = len(src) if k < 0 else k
            i = end
        elif src.startswith("/*", i):
            end = block_end(src, i)
            if DOC_OPEN.match(src, i):
                start = i + 3
                body = src[start:end]
                lines = split_lines(body[:-2] if body.endswith("*/") else body)
                inner = src.startswith("/*!", i)
                prev, seen, item = carried(tail, inner)
                standalone = not seen and not has_following_doc(src, end, inner)
                stripped = [doc_text_tabs(line) for line in block_text(lines, standalone)]
                hit, item = markdown_hit(stripped, LINE_INDENT, prev, item)
                if hit is not None:
                    yield start + sum(len(line) + 1 for line in lines[:hit])
                tail = (inner, stripped[-1], seen or any(line.strip() for line in stripped), item)
            i = end
        elif c == '"':
            end = skip_string(src, i)
            if doc_value:
                value = plain_value(src[i:end])
                prev, seen, item = carried(tail, attr_inner)
                standalone = not seen and paren == doc_paren and not has_following_doc(src, end, attr_inner)
                found, item = value_has_fence(value, prev, standalone, item)
                if found:
                    yield i
                if attr_name == "doc":
                    tail = (attr_inner, split_lines(value)[-1], seen or bool(value.strip()), item)
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
            attr_inner = src.startswith("!", skip_trivia(src, i + 1))
            i = j
        elif c in "([{":
            if attr_depth:
                paren += 1
                if c == "[":
                    attr_depth += 1
            elif c in "({":
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
                if c == "," and attr_name == "cfg_attr" and paren == 1:
                    cfg_comma = True
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
                        prev, seen, item = carried(tail, attr_inner)
                        standalone = not seen and paren == doc_paren and not has_following_doc(src, end, attr_inner)
                        found, item = value_has_fence(value, prev, standalone, item)
                        if found:
                            yield i
                        if attr_name == "doc":
                            tail = (attr_inner, split_lines(value)[-1], seen or bool(value.strip()), item)
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
