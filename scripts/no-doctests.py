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
LIST_INDENT = re.compile(r"[ \t]*" + CONTAINER + r"(?:[-*+]|\d{1,9}[.)]) {5,}\S")
BLOCK_MARGIN = re.compile(r"[ \t]*\*?")
QUOTE_START = re.compile(r"[ \t]*>")
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
HTML_ATTR_NAME = r"[A-Za-z_:][A-Za-z0-9_.:-]*"
HTML_ATTR_VALUE = r"[ \t]*=[ \t]*(?:[^ \t\"'=<>`]+|'[^']*'|\"[^\"]*\")"
HTML_ATTRIBUTE = r"[ \t]+" + HTML_ATTR_NAME + r"(?:" + HTML_ATTR_VALUE + r")?"
HTML_OPEN_TAG = r"<[A-Za-z][A-Za-z0-9-]*(?:" + HTML_ATTRIBUTE + r")*[ \t]*/?>"
HTML_CLOSE_TAG = r"</[A-Za-z][A-Za-z0-9-]*[ \t]*>"
FOOTNOTE = re.compile(r"\[\^[^\]\s]+\]:[ \t]*")
HTML_BLOCKS = (
    (re.compile(r"<(?:script|pre|style|textarea)(?:[ ]|>|$)", re.I), re.compile(r"</(?:script|pre|style|textarea)>", re.I)),
    (re.compile(r"<!--"), re.compile(r"-->")),
    (re.compile(r"<\?"), re.compile(r"\?>")),
    (re.compile(r"<![A-Za-z]"), re.compile(r">")),
    (re.compile(r"<!\[CDATA\["), re.compile(r"\]\]>")),
    (re.compile(r"</?(?:" + HTML_TYPE6 + r")(?:[ ]|/?>|$)", re.I), None),
    (re.compile(r"(?:" + HTML_OPEN_TAG + "|" + HTML_CLOSE_TAG + r")[ \t]*$"), None),
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
    return bool(text) and not text.startswith("#") and not HRULE.fullmatch(text)


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
        if LIST_INDENT.match(line) or (LINE_FENCE.match(line) and not (lazy and indent.match(line))):
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
        self.para_quoted = False
        self.item = None
        self.empty_item = False
        self.html = None

    def open_html(self, s, depth):
        matched, end = html_start(s)
        if not matched:
            return False
        self.html = None if end is not None and end.search(s) else (end, depth)
        self.para = False
        return True

    def begin_block(self, rest, quoted, depth):
        if self.open_html(rest, depth):
            return
        self.para = bool(rest) and not (ATX.match(rest) or THEMATIC.match(rest) or FENCE_OPEN.match(rest))
        self.para_quoted = quoted

    def feed(self, text):
        q = QUOTE_PREFIX.match(text)
        depth = q.group().count(">") if q else 0
        quoted = depth > 0
        body = text[q.end() :] if quoted else text
        ind = len(body) - len(body.lstrip(" "))
        s = body[ind:].rstrip()
        if s and not quoted and self.item is not None and ind < self.item:
            self.item = None
        if self.html is not None:
            end, open_depth = self.html
            if depth < open_depth:
                self.html = None
            else:
                content = body if open_depth else text
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
        if not s:
            self.para = False
            if self.empty_item:
                self.item = None
            return False
        self.empty_item = False
        if quoted and self.para and not self.para_quoted:
            self.para = False
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
            self.begin_block(rest, quoted, depth)
            return plain
        f = FOOTNOTE.match(s)
        if f and not quoted:
            self.item = ind + 4
            self.begin_block(s[f.end() :], quoted, depth)
            return plain
        self.begin_block(s, quoted, depth)
        return plain


def line_hit(bodies):
    doc = Doc()
    for k, body in enumerate(bodies):
        suppressed = doc.feed(body[1:] if body.startswith(" ") else body)
        if LIST_INDENT.match(body) or (LINE_FENCE.match(body) and not suppressed) or (LINE_INDENT.match(body) and not suppressed):
            return k
    return None


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
                hit = line_hit([body for _, body in run])
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
                hit = line_hit(stripped)
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
