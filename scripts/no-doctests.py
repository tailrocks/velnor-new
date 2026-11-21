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
WHITESPACE = " \t\r\n"
QUOTE_PREFIX = re.compile(r"(?:[ ]{0,3}>[ ]?)+")
LIST_ITEM = re.compile(r"([-*+]|\d{1,9}[.)])([ ]+|$)")
ATX = re.compile(r"#{1,6}(?:[ ]|$)")
THEMATIC = re.compile(r"(?:(?:\*[ ]*){3,}|(?:-[ ]*){3,}|(?:_[ ]*){3,})$")
SETEXT = re.compile(r"(?:=+|-+)[ ]*$")
FENCE_OPEN = re.compile(r"(?:```|~~~)")
HTML_TYPE6 = (
    "address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|"
    "fieldset|figcaption|figure|footer|form|frame|frameset|h1|h2|h3|h4|h5|h6|head|header|hr|html|iframe|legend|li|"
    "link|main|menu|menuitem|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|"
    "thead|title|tr|track|ul"
)
HTML_BLOCKS = (
    (re.compile(r"<(?:script|pre|style|textarea)(?:[ ]|>|$)", re.I), re.compile(r"</(?:script|pre|style|textarea)>", re.I)),
    (re.compile(r"<!--"), re.compile(r"-->")),
    (re.compile(r"<\?"), re.compile(r"\?>")),
    (re.compile(r"<![A-Za-z]"), re.compile(r">")),
    (re.compile(r"<!\[CDATA\["), re.compile(r"\]\]>")),
    (re.compile(r"</?(?:" + HTML_TYPE6 + r")(?:[ ]|/?>|$)", re.I), None),
    (re.compile(r"(?:<[A-Za-z][A-Za-z0-9-]*(?:[ ]+[^<>]*)?/?>|</[A-Za-z][A-Za-z0-9-]*[ ]*>)[ ]*$"), None),
)


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
    if body[:1] in (" ", "\t"):
        return " " + expand_tabs(body[1:])
    return expand_tabs(body)


def value_has_fence(value):
    doc = Doc()
    for line in re.split(r"\r\n|\r|\n", value):
        indent_hit = ATTR_INDENT.match(line.replace("\t", "    "))
        suppressed = doc.feed(line.replace("\t", "    "))
        if LINE_FENCE.match(line) or (indent_hit and not suppressed):
            return True
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


HTML_INTERRUPTING = HTML_BLOCKS[:-1]


def html_start(s, blocks=HTML_BLOCKS):
    for start, end in blocks:
        if start.match(s):
            return True, end
    return False, None


def block_start(s):
    return bool(
        THEMATIC.match(s)
        or SETEXT.match(s)
        or ATX.match(s)
        or FENCE_OPEN.match(s)
        or LIST_ITEM.match(s)
        or html_start(s, HTML_INTERRUPTING)[0]
    )


class Doc:
    def __init__(self):
        self.para = False
        self.item = None
        self.empty_item = False
        self.html = None

    def open_html(self, s, quoted):
        matched, end = html_start(s)
        if not matched:
            return False
        self.html = None if end is not None and end.search(s) else (end, quoted)
        self.para = False
        return True

    def feed(self, text):
        q = QUOTE_PREFIX.match(text)
        quoted = q is not None
        body = text[q.end() :] if quoted else text
        if self.html is not None:
            end, in_quote = self.html
            if in_quote and not quoted:
                self.html = None
            else:
                content = body if in_quote else text
                if end is None:
                    if content.strip():
                        return True
                    self.html = None
                    self.para = False
                    return False
                if end.search(content):
                    self.html = None
                return True
        if quoted:
            self.item = None
        ind = len(body) - len(body.lstrip(" "))
        s = body[ind:].rstrip()
        if not s:
            self.para = False
            if self.empty_item:
                self.item = None
            return False
        self.empty_item = False
        if not quoted and self.item is not None and ind < self.item:
            self.item = None
        base = 0 if quoted or self.item is None else self.item
        rel = ind - base
        plain = not quoted and ind >= 4
        if self.para and not (rel < 4 and block_start(s)):
            return plain
        if rel >= 4:
            self.para = False
            return False
        if THEMATIC.match(s) or ATX.match(s) or FENCE_OPEN.match(s):
            self.para = False
            return plain
        if self.para and SETEXT.match(s):
            self.para = False
            return plain
        m = LIST_ITEM.match(s)
        if m:
            rest = s[m.end() :]
            if not quoted:
                spaces = len(m.group(2))
                self.item = ind + len(m.group(1)) + (spaces if 0 < spaces <= 4 else 1)
                self.empty_item = not rest
            if self.open_html(rest, quoted):
                return plain
            self.para = bool(rest) and not (ATX.match(rest) or THEMATIC.match(rest) or FENCE_OPEN.match(rest))
            return plain
        if self.open_html(s, quoted):
            return plain
        self.para = True
        return plain


def block_text(line):
    expanded = line.replace("\t", "    ")
    star = re.match(r"[ ]*\*", expanded)
    if star:
        rest = expanded[star.end() :]
        return rest[1:] if rest.startswith(" ") else rest
    return expanded[1:] if expanded.startswith(" ") else expanded


def block_hit(body):
    doc = Doc()
    for line in body.split("\n"):
        indent_hit = BLOCK_INDENT.match(line.replace("\t", "    "))
        suppressed = doc.feed(block_text(line))
        if BLOCK_FENCE.match(line) or (indent_hit and not suppressed):
            return True
    return False


def doc_findings(src):
    i = 0
    attr_depth = 0
    doc_value = False
    line_doc = Doc()
    while i < len(src):
        c = src[i]
        if c not in WHITESPACE and not LINE_DOC.match(src, i):
            line_doc = Doc()
        if src.startswith("//", i):
            k = src.find("\n", i)
            end = len(src) if k < 0 else k
            if LINE_DOC.match(src, i):
                body = src[i + 3 : end]
                text = line_indent_text(body)
                suppressed = line_doc.feed(text[1:] if text.startswith(" ") else text)
                if LINE_FENCE.match(body) or (LINE_INDENT.match(text) and not suppressed):
                    yield i
            i = end
        elif src.startswith("/*", i):
            end = block_end(src, i)
            if DOC_OPEN.match(src, i) and block_hit(src[i + 3 : end]):
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
