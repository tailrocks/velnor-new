import re
from .lex import DOC_OPEN, IDENT, LINE_DOC, NEWLINE, attr_open, block_end, literal_end, skip_char, skip_string, skip_trivia, split_lines
from .markdown import carried, value_has_fence


DOC_EQ = re.compile(r"\s*=(?!=)")
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
    hit, item = value_has_fence(text, prev, standalone, item)
    tail = (inner, split_lines(text)[-1], seen or bool(text.strip()), item)
    return (None if hit is None else piece_at(pieces, hit)), tail


class CfgLevels:
    def __init__(self, name, tail):
        self.stack = [[1, False, tail]] if name == "cfg_attr" else []

    def comma(self, paren):
        if self.stack and paren == self.stack[-1][0]:
            self.stack[-1][1] = True

    def close(self, paren):
        while self.stack and paren < self.stack[-1][0]:
            self.stack.pop()

    def meta(self, paren):
        return bool(self.stack) and self.stack[-1][1] and paren == self.stack[-1][0]

    def nests(self, src, word, j, paren):
        if word == "cfg_attr" and self.meta(paren) and src.startswith("(", skip_trivia(src, j)):
            self.stack.append([paren + 1, False, self.stack[-1][2]])


def cfg_has_doc(src, i):
    levels = CfgLevels("cfg_attr", None)
    paren = 0
    while i < len(src):
        c = src[i]
        if (j := skip_trivia(src, i)) != i:
            i = j
        elif c == '"':
            i = skip_string(src, i)
        elif c == "'":
            j = skip_char(src, i)
            i = i + 1 if j is None else j
        elif c in "([{":
            paren += 1
            i += 1
        elif c in ")]}":
            paren -= 1
            if paren < 0:
                return False
            levels.close(paren)
            i += 1
        elif c == ",":
            levels.comma(paren)
            i += 1
        elif m := IDENT.match(src, i):
            word, j = m.group(), m.end()
            if (end := literal_end(src, word, j)) is not None:
                i = end
                continue
            if word == "doc" and levels.meta(paren) and DOC_EQ.match(src, skip_trivia(src, j)):
                return True
            levels.nests(src, word, j, paren)
            i = j
        else:
            i += 1
    return False


def attr_end(src, i):
    depth = 1
    while i < len(src) and depth:
        if src[i] == '"':
            i = skip_string(src, i)
            continue
        if src[i] == "[":
            depth += 1
        elif src[i] == "]":
            depth -= 1
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
