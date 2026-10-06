"""Bounded Rust source reader for the freshness gate's literal pin authority."""

SOURCE_CAP = 256 * 1024
_GROUPS = {"(": ")", "[": "]", "{": "}"}
_GROUP_ENDS = frozenset(_GROUPS.values())
_ESCAPES = {
    "n": "\n", "r": "\r", "t": "\t", "0": "\0",
    "\\": "\\", '"': '"', "'": "'",
}


def _skip_block_comment(source, index):
    depth = 1
    index += 2
    while index < len(source) and depth:
        if source.startswith("/*", index):
            depth += 1
            index += 2
        elif source.startswith("*/", index):
            depth -= 1
            index += 2
        else:
            index += 1
    if depth:
        raise ValueError("unterminated block comment")
    return index


def _escape(source, index, family):
    cursor = index + 1
    if cursor >= len(source):
        raise ValueError("unterminated escape")
    escape = source[cursor]
    if escape in _ESCAPES:
        if family == "c" and escape == "0":
            raise ValueError("NUL escape in C string")
        return cursor + 1, _ESCAPES[escape] if family == "str" else ""
    if escape == "x":
        digits = source[cursor + 1:cursor + 3]
        if len(digits) != 2 or any(
                char not in "0123456789abcdefABCDEF" for char in digits):
            raise ValueError("invalid hexadecimal escape")
        value = int(digits, 16)
        if family in ("str", "char") and value > 0x7f:
            raise ValueError("non-ASCII hexadecimal escape in Rust string")
        if family == "c" and value == 0:
            raise ValueError("NUL escape in C string")
        return cursor + 3, chr(value) if family == "str" else ""
    if escape == "u" and family not in ("byte", "bytechar"):
        if cursor + 1 >= len(source) or source[cursor + 1] != "{":
            raise ValueError("invalid Unicode escape")
        end = source.find("}", cursor + 2)
        if end < 0:
            raise ValueError("unterminated Unicode escape")
        digits = source[cursor + 2:end].replace("_", "")
        if not 1 <= len(digits) <= 6 or any(
                char not in "0123456789abcdefABCDEF" for char in digits):
            raise ValueError("invalid Unicode escape")
        scalar = int(digits, 16)
        if scalar > 0x10ffff or 0xd800 <= scalar <= 0xdfff:
            raise ValueError("invalid Unicode scalar")
        if family == "c" and scalar == 0:
            raise ValueError("NUL escape in C string")
        return end + 1, chr(scalar) if family == "str" else ""
    if escape in ("\n", "\r") and family in ("str", "byte", "c"):
        if escape == "\r" and source.startswith("\n", cursor + 1):
            cursor += 1
        cursor += 1
        while cursor < len(source) and source[cursor].isspace():
            cursor += 1
        return cursor, ""
    raise ValueError("unsupported Rust escape")


def _cooked_string(source, quote, family="str"):
    value = []
    index = quote + 1
    while index < len(source):
        char = source[index]
        if char == '"':
            return index + 1, "".join(value) if family == "str" else None
        if char == "\\":
            index, decoded = _escape(source, index, family)
            if family == "str":
                value.append(decoded)
            continue
        if family == "byte" and ord(char) > 0x7f:
            raise ValueError("non-ASCII character in byte string")
        if family == "c" and char == "\0":
            raise ValueError("NUL character in C string")
        if family == "str":
            value.append(char)
        index += 1
    raise ValueError("unterminated string literal")


def _char_end(source, quote, family="char"):
    index = quote + 1
    if index >= len(source) or source[index] in "\n\r'":
        return None
    if source[index] == "\\":
        index, _ = _escape(source, index, family)
    else:
        if family == "bytechar" and ord(source[index]) > 0x7f:
            raise ValueError("non-ASCII character in byte character")
        index += 1
    return index + 1 if index < len(source) and source[index] == "'" else None


def _raw_string(source, index):
    prefix = next((candidate for candidate in ("br", "cr", "r")
                   if source.startswith(candidate, index)), None)
    if prefix is None:
        return None
    cursor = index + len(prefix)
    hash_start = cursor
    while cursor < len(source) and source[cursor] == "#":
        cursor += 1
    if cursor >= len(source) or source[cursor] != '"':
        return None
    hashes = source[hash_start:cursor]
    end = source.find('"' + hashes, cursor + 1)
    if end < 0:
        raise ValueError("unterminated raw string literal")
    value = source[cursor + 1:end] if prefix == "r" else None
    return end + len(hashes) + 1, value, prefix == "r"


def _identifier(source, index):
    char = source[index]
    raw = char == "r" and source.startswith("r#", index) \
            and index + 2 < len(source) \
            and (source[index + 2] == "_" or source[index + 2].isidentifier())
    start = index + 2 if raw else index
    if not raw and char != "_" and not char.isidentifier():
        return None
    end = start + 1
    while end < len(source) and (source[end] == "_" or
                                  ("a" + source[end]).isidentifier()):
        end += 1
    return end, source[start:end]


def _tokens(source):
    tokens = []
    index = 0
    while index < len(source):
        char = source[index]
        if char.isspace():
            index += 1
            continue
        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            index = len(source) if end < 0 else end + 1
            continue
        if source.startswith("/*", index):
            index = _skip_block_comment(source, index)
            continue
        raw = _raw_string(source, index)
        if raw is not None:
            index, value, is_str = raw
            tokens.append(("string", value) if is_str else ("literal", None))
            continue
        if source.startswith(('b"', 'c"'), index):
            family = "byte" if char == "b" else "c"
            index, _ = _cooked_string(source, index + 1, family)
            tokens.append(("literal", None))
            continue
        if char == '"':
            index, value = _cooked_string(source, index)
            tokens.append(("string", value))
            continue
        if source.startswith("b'", index):
            end = _char_end(source, index + 1, "bytechar")
            if end is None:
                raise ValueError("invalid byte character literal")
            index = end
            tokens.append(("literal", None))
            continue
        if char == "'":
            end = _char_end(source, index)
            if end is not None:
                index = end
                tokens.append(("literal", None))
                continue
            if source.startswith("\\", index + 1):
                raise ValueError("unterminated character literal")
        ident = _identifier(source, index)
        if ident is not None:
            index, value = ident
            tokens.append(("ident", value))
            continue
        if char.isascii() and char.isdigit():
            index += 1
            while index < len(source) and (
                    source[index].isalnum() or source[index] in "_'"):
                index += 1
            tokens.append(("literal", None))
            continue
        tokens.append(("punct", char))
        index += 1
    return _without_macro_bodies(tokens)


def _skip_group(tokens, index):
    opening = tokens[index][1]
    if opening not in _GROUPS:
        raise ValueError("invalid Rust token group")
    stack = [_GROUPS[opening]]
    for cursor in range(index + 1, len(tokens)):
        kind, value = tokens[cursor]
        if kind != "punct":
            continue
        if value in _GROUPS:
            stack.append(_GROUPS[value])
        elif value in _GROUP_ENDS:
            if not stack or stack.pop() != value:
                raise ValueError("mismatched Rust token group")
            if not stack:
                return cursor + 1
    raise ValueError("unterminated Rust token group")


def _without_macro_bodies(tokens):
    filtered = []
    index = 0
    while index < len(tokens):
        if tokens[index:index + 2] == [("ident", "macro_rules"), ("punct", "!")] \
                and index + 3 < len(tokens) \
                and tokens[index + 2][0] == "ident" \
                and tokens[index + 3][0] == "punct" \
                and tokens[index + 3][1] in _GROUPS:
            index = _skip_group(tokens, index + 3)
            continue
        if tokens[index] == ("punct", "!") \
                and (index == 0 or tokens[index - 1] != ("punct", "#")) \
                and index + 1 < len(tokens) \
                and tokens[index + 1][0] == "punct" \
                and tokens[index + 1][1] in _GROUPS:
            index = _skip_group(tokens, index + 1)
            continue
        filtered.append(tokens[index])
        index += 1
    return filtered


def _top_level_consts(tokens, name):
    matches = []
    stack = []
    declaration = [("ident", "const"), ("ident", name)]
    for index, (kind, value) in enumerate(tokens):
        if kind == "punct" and value in _GROUPS:
            stack.append(_GROUPS[value])
        elif kind == "punct" and value in _GROUP_ENDS:
            if not stack or stack.pop() != value:
                raise ValueError("mismatched Rust source delimiter")
        elif not stack and tokens[index:index + 2] == declaration:
            matches.append(index)
    if stack:
        raise ValueError("unterminated Rust source delimiter")
    return matches


def _has_conditional_crate_attribute(tokens):
    index = 0
    while index < len(tokens):
        if tokens[index:index + 3] == [
                ("punct", "#"), ("punct", "!"), ("punct", "[")]:
            if index + 3 < len(tokens) and tokens[index + 3] in (
                    ("ident", "cfg"), ("ident", "cfg_attr")):
                return True
            index = _skip_group(tokens, index + 2)
            continue
        if tokens[index][0] == "punct" and tokens[index][1] in _GROUPS:
            index = _skip_group(tokens, index)
            continue
        index += 1
    return False


def extract_rust_const(source: bytes, name: str) -> str:
    """Return one unconditional, public top-level `&str` literal's value."""
    if len(source) > SOURCE_CAP:
        raise ValueError(f"source exceeds {SOURCE_CAP} bytes")
    try:
        tokens = _tokens(source.decode("utf-8"))
        if _has_conditional_crate_attribute(tokens):
            raise ValueError("conditional crate attribute")
        matches = _top_level_consts(tokens, name)
    except (UnicodeError, ValueError) as error:
        raise ValueError(f"unsupported Rust source ({error})") from error
    if len(matches) != 1:
        detail = f"missing const {name}" if not matches else f"duplicate const {name}"
        raise ValueError(detail)
    index = matches[0]
    if index == 0 or tokens[index - 1] != ("ident", "pub"):
        raise ValueError(f"unsupported const {name} visibility")
    if index >= 2 and tokens[index - 2] == ("punct", "]"):
        raise ValueError("unsupported authority attribute")
    expected = [("punct", ":"), ("punct", "&"), ("ident", "str"),
                ("punct", "="), None, ("punct", ";")]
    actual = tokens[index + 2:index + 8]
    if len(actual) != len(expected) or any(
            wanted is not None and found != wanted
            for found, wanted in zip(actual, expected, strict=True)):
        raise ValueError(f"unsupported const {name} declaration")
    if actual[4][0] != "string":
        raise ValueError(f"const {name} must use one string literal")
    return actual[4][1]
