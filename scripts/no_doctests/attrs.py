import re
from .lex import DOC_OPEN, IDENT, LINE_DOC, NEWLINE, attr_open, block_end, literal_end, skip_char, skip_string, skip_trivia
from .cfg import cfg_has_doc
from .markdown import carried, value_has_fence


DOC_INCLUDE = re.compile(r"\s*=\s*include_str!")
STRING_ESCAPE = re.compile(r"\\(?:x([0-9a-fA-F]{2})|u\{([0-9a-fA-F_]{1,8})\}|(\r?\n[ \t\r\n]*)|(.))", re.S)
SIMPLE_ESCAPE = {"n": "\n", "r": "\r", "t": "\t", "0": "\0", "\\": "\\", "'": "'", '"': '"'}


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


def piece_at(pieces, line):
    text = "".join(piece for _, piece in pieces)
    starts = [0] + [m.end() for m in NEWLINE.finditer(text)]
    offset, seen = starts[line], 0
    for pos, piece in pieces:
        seen += len(piece)
        if offset < seen:
            return pos
    return pieces[-1][0]


def flush_value(src, end, pieces, inner, tail):
    text = "".join(piece for _, piece in pieces)
    prev, seen, item = carried(tail, inner)
    standalone = not seen and not has_following_doc(src, end, inner)
    hit, item, prev = value_has_fence(text, prev, standalone, item)
    tail = (inner, prev, seen or bool(text.strip()), item)
    return (None if hit is None else piece_at(pieces, hit)), tail


def attr_end(src, i):
    depth = 1
    while i < len(src) and depth:
        c = src[i]
        if (j := skip_trivia(src, i)) != i:
            i = j
        elif c == '"':
            i = skip_string(src, i)
        elif c == "'":
            j = skip_char(src, i)
            i = i + 1 if j is None else j
        elif m := IDENT.match(src, i):
            end = literal_end(src, m.group(), m.end())
            i = m.end() if end is None else end
        else:
            depth += (c == "[") - (c == "]")
            i += 1
    return i


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
            if name and name.group() == "cfg_attr" and cfg_has_doc(src, k):
                return src.startswith("!", skip_trivia(src, j + 1)) == inner
            j = attr_end(src, k)
        elif m := IDENT.match(src, j):
            j = m.end()
        else:
            return False
    return False
