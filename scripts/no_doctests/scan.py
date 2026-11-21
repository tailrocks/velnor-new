from .lex import BOOLS, DOC_EQ, BYTE_PREFIX, DOC_OPEN, IDENT, LINE_DOC, RAW_PREFIX, attr_open, block_end, skip_char, number_piece, skip_raw, skip_string, skip_trivia, split_lines
from .block import block_text
from .markdown import LINE_INDENT, carried, doc_text_tabs, markdown_hit, unindent_run
from .cfg import CfgLevels, cfg_value
from .attrs import DOC_INCLUDE, flush_value, has_following_doc, plain_value, raw_value


def line_run(src, i):
    run = []
    pos = i
    joinable = not src[src.rfind("\n", 0, i) + 1 : i].strip()
    while True:
        k = src.find("\n", pos)
        end = len(src) if k < 0 else k
        text = src[pos + 3 : end]
        run.append((pos, doc_text_tabs(text), "\t" in text[: len(text) - len(text.lstrip(" \t"))]))
        j = end + 1
        while j < len(src) and src[j] in " \t":
            j += 1
        if not joinable or end >= len(src) or not LINE_DOC.match(src, j) or src.startswith("//!", j) != src.startswith("//!", i):
            return run, end
        pos = j


def doc_findings(src):
    i = 0
    attr_depth = 0
    attr_name = ""
    paren = 0
    levels = CfgLevels("", None)
    doc_frame = None
    doc_paren = 0
    doc_value = False
    pieces = []
    attr_inner = False
    tail = None
    while i < len(src) or pieces:
        if pieces and not (doc_value and i < len(src)):
            if doc_frame is None:
                hit, tail = flush_value(src, i, pieces, attr_inner, tail)
            else:
                cell = doc_frame[2]
                hit, cell[0] = flush_value(src, i, pieces, attr_inner, cell[0])
                if doc_frame[3]:
                    tail = cell[0]
            pieces = []
            if hit is not None:
                yield hit
            continue
        c = src[i]
        if src.startswith("//", i):
            if LINE_DOC.match(src, i):
                run, end = line_run(src, i)
                bodies = [body for _, body, _ in run]
                inner = src.startswith("//!", i)
                prev, seen, item = carried(tail, inner)
                tabbed = any(tab for _, _, tab in run)
                standalone = not seen and not tabbed and not has_following_doc(src, end, inner)
                hit, item, prev = markdown_hit(unindent_run(bodies) if standalone else bodies, LINE_INDENT, prev, item)
                if hit is not None:
                    yield run[hit][0]
                tail = (inner, prev, seen or any(body.strip() for body in bodies), item)
            else:
                k = src.find("\n", i)
                end = len(src) if k < 0 else k
            i = end
        elif src.startswith("/*", i):
            end = block_end(src, i)
            if DOC_OPEN.match(src, i):
                start = i + 3
                body = src[start:end]
                lines = split_lines(body[:-2] if body.endswith("*/") else body)
                inner = src.startswith("/*!", i)
                prev, seen, item = carried(tail, inner)
                standalone = not seen and not has_following_doc(src, end, inner)
                stripped = [doc_text_tabs(line) for line in block_text(lines, standalone)]
                hit, item, prev = markdown_hit(stripped, LINE_INDENT, prev, item)
                if hit is not None:
                    yield start + sum(len(line) + 1 for line in lines[:hit])
                tail = (inner, prev, seen or any(line.strip() for line in stripped), item)
            i = end
        elif c == '"':
            end = skip_string(src, i)
            if doc_value:
                pieces.append((i, plain_value(src[i:end])))
            i = end
        elif c == "'":
            j = skip_char(src, i)
            if doc_value and j is not None:
                pieces.append((i, plain_value(src[i:j])))
            i = i + 1 if j is None else j
        elif doc_value and c.isdigit():
            value, end = number_piece(src, i)
            pieces.append((i, value))
            i = end
        elif doc_value and c == "-":
            pieces.append((i, c))
            i += 1
        elif c == "#" and (j := attr_open(src, i)) is not None:
            attr_depth = 1
            paren = 0
            k = skip_trivia(src, j)
            k += 2 if src.startswith("r#", k) else 0
            name = IDENT.match(src, k)
            attr_name = name.group() if name else ""
            on = attr_name == "cfg_attr" and src.startswith("(", p := skip_trivia(src, name.end())) and cfg_value(src, p + 1)[0] is True
            levels = CfgLevels(attr_name, tail, on)
            attr_inner = src.startswith("!", skip_trivia(src, i + 1))
            i = j
        elif c in "([{":
            if attr_depth:
                paren += 1
                if c == "[":
                    attr_depth += 1
            elif c in "({":
                tail = None
            i += 1
        elif c in ")]}":
            if attr_depth:
                paren -= 1
                levels.close(paren)
                if c == "]":
                    attr_depth -= 1
                doc_value = doc_value and attr_depth > 0 and paren >= doc_paren
            elif c == "}":
                tail = None
            i += 1
        elif c in ",;":
            if attr_depth:
                doc_value = doc_value and paren > doc_paren
                if c == ",":
                    levels.comma(paren)
            else:
                tail = None
            i += 1
        elif m := IDENT.match(src, i):
            word = m.group()
            j = i + len(word)
            if word in RAW_PREFIX and j < len(src) and src[j] in '#"':
                end = skip_raw(src, j - 1)
                if end is None:
                    i = j
                else:
                    if doc_value:
                        pieces.append((i, raw_value(src[i:end], len(word))))
                    i = end
            elif word in BYTE_PREFIX and j < len(src) and src[j] == '"':
                i = skip_string(src, j)
            elif doc_value and word in BOOLS:
                pieces.append((i, word))
                i = j
            elif doc_value and src.startswith("!", skip_trivia(src, j)) and word != "concat":
                yield i
                i = j
            else:
                if word == "doc":
                    k = skip_trivia(src, j)
                    if DOC_INCLUDE.match(src, k):
                        yield i
                    elif DOC_EQ.match(src, k) and ((attr_name == "doc" and paren == 0) or levels.meta(paren)):
                        doc_value = True
                        doc_paren = paren
                        doc_frame = None if attr_name == "doc" and paren == 0 else levels.stack[-1]
                else:
                    levels.nests(src, word, j, paren)
                i = j
        else:
            i += 1
