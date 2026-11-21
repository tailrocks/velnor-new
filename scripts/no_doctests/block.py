def star_prefix(lines, lo, hi):
    lo += 0 if lines[lo].lstrip().startswith("*") else 1
    while lo < hi and not lines[lo].strip():
        lo += 1
    while hi > lo and not lines[hi - 1].strip():
        hi -= 1
    column = None
    for line in lines[lo:hi]:
        for j, c in enumerate(line):
            if (column is not None and j > column) or c not in "* \t":
                return None
            if c == "*":
                if column not in (None, j):
                    return None
                column = j
                break
        if column is None or column >= len(line):
            return None
    return None if lo == hi else lines[lo][:column]


def beautify(lines):
    if len(lines) < 2:
        return lines, False
    lo, hi = 0, len(lines) - (lines[-1] == "")
    lo += all(c == "*" for c in lines[0])
    if hi > lo and lines[hi - 1] and all(c == "*" for c in lines[hi - 1]):
        hi -= 1
    if lo >= hi:
        return lines, False
    prefix = star_prefix(lines, lo, hi)
    if prefix is None:
        return lines, False
    out = lines[:]
    for k in range(lo, hi):
        if out[k].startswith(prefix):
            line = out[k][len(prefix) :]
            out[k] = line[1:] if line == "*" or line.startswith(("* ", "**")) else line
    return out, True


def block_text(lines, normalize=True):
    body, starred = beautify(lines)
    text = [line for line in body if line.strip()]
    margin = min((len(line) - len(line.lstrip(" \t")) for line in text), default=0) if normalize else 0
    if starred and not normalize:
        return body
    return [" " + line[margin:] if line.strip() else "" for line in body]
