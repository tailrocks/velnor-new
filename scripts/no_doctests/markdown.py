import re
from .lex import split_lines


FENCE = re.compile(r"```|~~~")
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
HTML_DONE = re.compile(r"<!--.*-->|<\?.*\?>|<![A-Za-z].*>|<!\[CDATA\[.*\]\]>")


def prose(line):
    if line is None:
        return False
    text = re.sub("^" + CONTAINER, "", line.strip(), count=1)
    return bool(text) and not ATX.match(text) and not HRULE.fullmatch(text) and not HTML_DONE.fullmatch(text)


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
    return markdown_hit(lines, ATTR_INDENT, prev, item)
