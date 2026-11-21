import re


IDENT = re.compile(r"[^\W\d]\w*")
LINE_DOC = re.compile(r"//(?:!|/(?!/))")
DOC_OPEN = re.compile(r"/\*(?:!|\*(?![*/]))")
DOC_EQ = re.compile(r"\s*=(?!=)")
NEWLINE = re.compile(r"\r\n|\r|\n")
NUMBER = re.compile(r"0[xX][0-9a-fA-F_]+|0[oO][0-7_]+|0[bB][01_]+|\d[\d_]*(?:\.(?![.\w])|\.\d[\d_]*)?(?:[eE][+-]?[\d_]+)?")
BOOLS = {"true", "false"}
BASES = {"x": 16, "o": 8, "b": 2}
RAW_PREFIX = {"r", "br", "cr"}
BYTE_PREFIX = {"b", "c"}


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


def literal_end(src, word, j):
    if word in RAW_PREFIX and j < len(src) and src[j] in '#"':
        return skip_raw(src, j - 1)
    if word in BYTE_PREFIX and j < len(src) and src[j] == '"':
        return skip_string(src, j)
    return None


def split_lines(text):
    return NEWLINE.split(text)


def number_piece(src, i):
    m = NUMBER.match(src, i)
    text = m.group().replace("_", "")
    base = BASES.get(text[1:2].lower()) if text[:1] == "0" else None
    suffix = IDENT.match(src, m.end())
    return (str(int(text, base)) if base else text), (suffix.end() if suffix else m.end())
