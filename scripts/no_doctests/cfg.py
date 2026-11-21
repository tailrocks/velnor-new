from .lex import DOC_EQ, IDENT, literal_end, skip_char, skip_string, skip_trivia


def cfg_value(src, i):
    i = skip_trivia(src, i)
    m = IDENT.match(src, i)
    if m is None:
        return None, i + 1
    word, j = m.group(), skip_trivia(src, m.end())
    if src.startswith("=", j):
        j = skip_trivia(src, j + 1)
        return None, skip_string(src, j) if src.startswith('"', j) else j
    if not src.startswith("(", j):
        return {"true": True, "false": False}.get(word), m.end()
    args = []
    j += 1
    while (j := skip_trivia(src, j)) < len(src) and src[j] != ")":
        value, end = cfg_value(src, j)
        args.append(value)
        j = skip_trivia(src, end)
        j += src.startswith(",", j)
    known = [v for v in args if v is not None]
    if word == "all":
        value = False if False in args else True if len(known) == len(args) else None
    elif word == "any":
        value = True if True in args else False if len(known) == len(args) else None
    else:
        value = (not args[0]) if word == "not" and len(args) == 1 and args[0] is not None else None
    return value, j + 1


class CfgLevels:
    def __init__(self, name, tail, active=False):
        self.stack = [[1, False, [tail], active]] if name == "cfg_attr" else []

    def comma(self, paren):
        if self.stack and paren == self.stack[-1][0]:
            self.stack[-1][1] = True

    def close(self, paren):
        while self.stack and paren < self.stack[-1][0]:
            self.stack.pop()

    def meta(self, paren):
        return bool(self.stack) and self.stack[-1][1] and paren == self.stack[-1][0]

    def nests(self, src, word, j, paren):
        open_paren = skip_trivia(src, j)
        if word == "cfg_attr" and self.meta(paren) and src.startswith("(", open_paren):
            _, _, cell, parent_on = self.stack[-1]
            on = parent_on and cfg_value(src, open_paren + 1)[0] is True
            self.stack.append([paren + 1, False, [cell[0]] if parent_on and not on else cell, on])


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
