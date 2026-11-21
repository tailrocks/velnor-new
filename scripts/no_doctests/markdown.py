import re
from .html import BLANK, html_block
from .lex import split_lines


FENCE = re.compile(r"```|~~~")
QUOTE = r"(?:[ \t]*>)*"
FOOTNOTE = r"\[\^(?!\s*\])(?:\\.|[^\]\\])*\]:"
CONTAINER = r"(?:>[ \t]*|(?:[-*+]|\d{1,9}[.)])(?:[ \t]+(?:\[[ xX]\][ \t]+)?|$)|" + FOOTNOTE + r"[ \t]*)*"
LINE_FENCE = re.compile(r"[ \t]*" + CONTAINER + r"(?:```|~~~)")
LINE_INDENT = re.compile(QUOTE + r" {5,}\S")
ATTR_INDENT = re.compile(r" {4,}\S|(?:[ \t]*>)+ {5,}\S")
QUOTE_CODE = re.compile(r"(?:[ \t]*>)+ {5,}\S")
QUOTE_DEPTH = re.compile(QUOTE)
LIST_INDENT = re.compile(r"[ \t]*" + CONTAINER + r"(?:[-*+]|\d{1,9}[.)]) {5,}\S")
LIST_ITEM = re.compile(r" *(?:[-*+]|\d{1,9}[.)]) +\S")
EMPTY_ITEM = re.compile(r" *([-*+]|\d{1,9}[.)])[ \t]*$")
HRULE = re.compile(r"(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,}")
SETEXT = re.compile(r"=+[ \t]*|-+[ \t]*")
ATX = re.compile(r"#{1,6}(?:[ \t]|$)")
FOOTNOTE_DEF = re.compile(r"[ \t]*" + FOOTNOTE)
NO_PREV = (False, 0, None)


def line_text(line):
    return re.sub("^" + CONTAINER, "", line.strip(), count=1)


def paragraph(text, indented):
    return bool(text) and (indented or not (ATX.match(text) or HRULE.fullmatch(text)))


def doc_text_tabs(text):
    if text[:1] in (" ", "\t"):
        return " " + text[1:].expandtabs(4)
    return text.expandtabs(4)


def unindent_run(bodies):
    texts = [body[1:] if body[:1] == " " else body for body in bodies]
    margin = min((len(text) - len(text.lstrip(" ")) for text in texts if text.strip()), default=0)
    return [" " + text[margin:] if text.strip() else "" for text in texts]


def carried(tail, inner):
    if tail is None or tail[0] != inner:
        return NO_PREV, False, None
    return tail[1], tail[2], tail[3]


def markdown_hit(lines, indent, prev=NO_PREV, item=None):
    for k, raw in enumerate(lines):
        line = raw.expandtabs(4)
        blank = not line.strip()
        width = len(line) - len(line.lstrip(" "))
        if prev[2] == BLANK and blank:
            prev = NO_PREV
        if prev[2] is not None:
            prev = (False, 0, prev[2] if prev[2] == BLANK or not prev[2].search(line) else None)
            continue
        was = item
        if not blank and item is not None and width < item:
            item = None
        depth = QUOTE_DEPTH.match(line).group().count(">")
        lazy = prev[0] and depth <= prev[1]
        code = indent.match(line) if item is None or blank else width >= item + 4 or bool(QUOTE_CODE.match(line[item:]))
        if LIST_INDENT.match(line) or (LINE_FENCE.match(line) and not (lazy and code)) or (code and not lazy):
            return k, item, prev
        match = LIST_ITEM.match(line)
        empty = EMPTY_ITEM.match(line)
        if match and not HRULE.fullmatch(line.strip()):
            item = match.end() - 1
        elif empty and not prev[0]:
            item = width + len(empty.group(1)) + 1
        elif FOOTNOTE_DEF.match(line):
            item = width + 4
        text = line_text(line)
        heading = prev[0] and SETEXT.fullmatch(line.strip()) and not indent.match(line) and (was is None or width >= was)
        is_html, state = (False, None) if code or heading else html_block(text, prev[0])
        if heading:
            prev = NO_PREV
        elif is_html:
            prev = (False, 0, state)
        else:
            prev = (paragraph(text, bool(code)) or (prev[0] and bool(empty)), depth, None)
    return None, item, prev


def value_has_fence(value, prev=NO_PREV, normalize=True, item=None):
    lines = [line.expandtabs(4) for line in split_lines(value)]
    if normalize:
        margin = min((len(line) - len(line.lstrip(" ")) for line in lines if line.strip()), default=0)
        lines = [line[margin:] for line in lines]
    return markdown_hit(lines, ATTR_INDENT, prev, item)
