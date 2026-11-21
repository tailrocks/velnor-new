#!/usr/bin/env python3
import re
import sys
from pathlib import Path

IDENT = re.compile(r"[^\W\d]\w*")
FENCE = re.compile(r"```|~~~")
LINE_DOC = re.compile(r"//(?:!|/(?!/))")
CONTAINER = r"(?:>[ \t]*|(?:[-*+]|\d{1,9}[.)])[ \t]+(?:\[[ xX]\][ \t]+)?|\[\^[^\]\s]+\]:[ \t]*)*"
MARKER = r"[ \t]*(?:>|(?:[-*+]|\d{1,9}[.)])(?=[ \t])|\[\^[^\]\s]+\]:)"
FENCE_OPEN = r"(?:`{3,}(?=[^`\n]*$)|~{3,})"
LINE_FENCE = re.compile(r"[ \t]*" + CONTAINER + FENCE_OPEN)
LINE_INDENT = re.compile(r"(?:" + MARKER + r")* {5,}\S")
ATTR_INDENT = re.compile(r" {4,}\S|(?:" + MARKER + r")+ {5,}\S")
HTML6_TAGS = (
    "address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt"
    "|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu"
    "|menuitem|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr"
    "|track|ul"
)
HTML_BLOCK6 = re.compile(r"[ ]{0,3}</?(?:" + HTML6_TAGS + r")(?=[ \t>]|/>|$)", re.I)
TAG_OPEN = r"<[A-Za-z][A-Za-z0-9-]*(?:[ \t]+[A-Za-z_:][A-Za-z0-9_.:-]*(?:[ \t]*=[ \t]*(?:[^ \t\"'=<>`]+|'[^']*'|\"[^\"]*\"))?)*[ \t]*/?>"
TAG_CLOSE = r"</[A-Za-z][A-Za-z0-9-]*[ \t]*>"
HTML_BLOCK7 = re.compile(r"[ ]{0,3}(?:" + TAG_OPEN + "|" + TAG_CLOSE + r")[ \t]*$")
RAW_OPEN = re.compile(r"[ ]{0,3}(?:<((?i:script|pre|style|textarea))(?=[\s>]|$)|<!--|<\?|<!\[CDATA\[|<![A-Za-z])")
RAW_END = {"<!--": "-->", "<?": "?>", "<![cdata[": "]]>"}
QUOTE_PREFIX = re.compile(r"(?:[ \t]*>)*")
NON_PROSE = re.compile(r"[ \t]*(?:#{1,6}(?:[ \t]|$)|(?:[-*+]|\d{1,9}[.)])(?:[ \t]|$)|[-*_\s]*$|\[\^[^\]\s]+\]:[ \t]*$)")
EQ_UNDERLINE = re.compile(r"[ \t]*=+[ \t]*$")
LIST_MARK = re.compile(r"[ \t]*(?:[-*+]|\d{1,9}[.)])(?=[ \t]|$)[ \t]*")
LIST_ITEM = re.compile(r"[ \t]*(?:[-*+]|\d{1,9}[.)])[ \t]+")
LIST_CODE = re.compile(r"[ \t]*(?:[-*+]|\d{1,9}[.)])[ \t]{5,}\S")
BLOCK_MARGIN = re.compile(r"[ \t]*\*?")
NEWLINE = re.compile(r"\r\n|\r|\n")
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
    return text.expandtabs(4)


def doc_text_tabs(text):
    if text[:1] in (" ", "\t"):
        return " " + text[1:].expandtabs(4)
    return text.expandtabs(4)


def line_indent_text(body):
    if body[:1] in (" ", "\t"):
        return " " + expand_tabs(body[1:])
    return expand_tabs(body)


def new_doc_state():
    return {"html": False, "prose": None, "raw": None, "depth": 0, "item": None, "list": None}


def value_has_fence(value, state):
    lines = re.split(r"\r\n|\r|\n", value)
    margin = min((len(line) - len(line.lstrip(" ")) for line in lines if line.strip()), default=0)
    return any(doc_line(" " + line[margin:], state) for line in lines)


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


def doc_content(body):
    text = line_indent_text(body)
    return text[1:] if body[:1] in (" ", "\t") else text


def is_prose(content):
    content = LIST_MARK.sub("", content, count=1)
    if NON_PROSE.match(content):
        return False
    return re.match(r"[ \t]*" + FENCE_OPEN, content) is None


def raw_end(opener):
    if opener.group(1):
        return f"</{opener.group(1).lower()}>"
    return RAW_END.get(opener.group().lstrip(" ").lower(), ">")


def doc_line(body, state):
    rel = doc_content(body)
    quote = QUOTE_PREFIX.match(rel)
    depth = quote.group().count(">")
    text = rel[quote.end() :]
    indent = len(text) - len(text.lstrip(" "))
    if depth < state["depth"] or (state["item"] is not None and text.strip() and indent < state["item"]):
        state["html"] = False
        state["raw"] = None
        state["item"] = None
    if state["html"]:
        state["html"] = bool(text.strip())
        if not state["html"]:
            state["item"] = None
        return False
    if state["raw"] is not None:
        if state["raw"] in text.lower():
            state["raw"] = None
            state["item"] = None
        return False
    if not text.strip():
        state["prose"] = None
        return False
    item = LIST_ITEM.match(text)
    opener = text[item.end() :] if item else text
    continued = state["prose"] is not None and depth <= state["prose"]
    if item:
        state["list"] = (depth, item.end())
    elif state["list"] is not None and (state["list"][0] != depth or (indent < state["list"][1] and not continued)):
        state["list"] = None
    item_col = item.end() if item else None
    if item is None and state["list"] is not None and indent >= state["list"][1]:
        item_col = state["list"][1]
    if match := RAW_OPEN.match(opener):
        end = raw_end(match)
        state["prose"] = None
        if end not in opener[match.end() :].lower():
            state["raw"] = end
            state["depth"] = depth
            state["item"] = item_col
        return False
    if HTML_BLOCK6.match(opener) or ((item is not None or state["prose"] is None) and HTML_BLOCK7.match(opener)):
        state["html"] = True
        state["prose"] = None
        state["depth"] = depth
        state["item"] = item_col
        return False
    indented = LINE_INDENT.match(line_indent_text(body)) is not None and (item is not None or item_col is None or indent - item_col >= 4)
    if LIST_CODE.match(text) or (indented and not continued) or (LINE_FENCE.match(body) and not (continued and indented)):
        state["prose"] = None
        return True
    setext = state["prose"] is not None and EQ_UNDERLINE.match(text) is not None
    state["prose"] = depth if not setext and ((continued and indented) or is_prose(text)) else None
    return False


def doc_findings(src):
    i = 0
    attr_depth = 0
    attr_name = ""
    paren = 0
    cfg_comma = False
    doc_paren = 0
    doc_value = False
    doc_end = -2
    doc_state = new_doc_state()
    attr_doc = None
    while i < len(src):
        c = src[i]
        if src.startswith("//", i):
            k = src.find("\n", i)
            end = len(src) if k < 0 else k
            if LINE_DOC.match(src, i):
                start = src.rfind("\n", 0, i) + 1
                if start != doc_end + 1:
                    doc_state = new_doc_state()
                doc_end = end
                if doc_line(src[i + 3 : end], doc_state):
                    yield i
            i = end
        elif src.startswith("/*", i):
            end = block_end(src, i)
            if DOC_OPEN.match(src, i):
                state = new_doc_state()
                if any(doc_line(doc_text_tabs(BLOCK_MARGIN.sub("", line, count=1)), state) for line in NEWLINE.split(src[i + 3 : end])):
                    yield i
            i = end
        elif c == '"':
            end = skip_string(src, i)
            if doc_value:
                attr_doc = attr_doc or new_doc_state()
                if value_has_fence(plain_value(src[i:end]), attr_doc):
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
            if not attr_depth:
                attr_doc = None
            word = m.group()
            j = i + len(word)
            if word in RAW_PREFIX and j < len(src) and src[j] in '#"':
                end = skip_raw(src, j - 1)
                if end is None:
                    i = j
                else:
                    if doc_value:
                        attr_doc = attr_doc or new_doc_state()
                        if value_has_fence(raw_value(src[i:end], len(word)), attr_doc):
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
            if c in ";{}" and not attr_depth:
                attr_doc = None
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
