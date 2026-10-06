"""Literal parse-changelog 0.6.17 default-parser semantics; no Markdown rendering.

Pinned release-plz 0.3.169 Cargo.lock checksum:
c0878368d958d9eac74fcbbe62d3fa9fc0e6f1c2ea7c16f9cdbd7232722d58d9.
Source: parse-changelog src/lib.rs, ParseIter, heading, extract_version_from_title.
"""
import re

_NOTES_SPACE = "\t\n\v\f\r \u0085\u00a0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200a\u2028\u2029\u202f\u205f\u3000"
_NOTES_VERSION = re.compile(
    r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?|Unreleased")
_NOTES_PREFIX = re.compile(r"^(v|Version |Release )?")


def _notes_indent(line):
    count = len(line) - len(line.lstrip(" "))
    return line[count:] if count < 4 else line


def _notes_heading(lines, index):
    line = _notes_indent(lines[index][0])
    if line.startswith("#"):
        level = len(line) - len(line.lstrip("#"))
        if level < 7 and (len(line) == level or line[level] in " \t"):
            return level, line[level + 1:].strip(_NOTES_SPACE), False
    if index + 1 < len(lines):
        underline = _notes_indent(lines[index + 1][0])
        if underline and underline[0] in "=-":
            rest = underline[1:].rstrip(_NOTES_SPACE)
            if all(char == underline[0] for char in rest):
                return (1 if underline[0] == "=" else 2,
                        line.rstrip(_NOTES_SPACE), True)
    return None


def _notes_version(title):
    value = title.removeprefix("[")
    value = value[_NOTES_PREFIX.match(value).end():]
    stop = next((i for i, char in enumerate(value) if char in _NOTES_SPACE), len(value))
    value = value[:stop].removeprefix("[")
    return value.split("]", 1)[0]


def _notes_comment(line, active):
    while line:
        opened, closed = line.find("<!--"), line.find("-->")
        if opened < 0 and closed < 0:
            break
        if closed < 0:
            return True
        if opened < 0:
            return False
        if opened < closed:
            active, line = False, line[closed + 3:]
        else:
            active, line = True, line[opened + 4:]
    return active


def _notes_nonheading(line, fence, comment):
    trimmed = _notes_indent(line)
    if fence is not None:
        if trimmed.startswith(fence):
            fence = None
    elif trimmed and trimmed[0] in "`~":
        count = len(trimmed) - len(trimmed.lstrip(trimmed[0]))
        if count >= 3:
            fence = trimmed[:count]
    if fence is None:
        comment = _notes_comment(line, comment)
    return fence, comment


def _notes_lines(text):
    result, start = [], 0
    for line in text.split("\n"):
        result.append((line, start, start + len(line)))
        start += len(line) + 1
    return result


def _notes_next(text, lines, index, release_level):
    fence, comment, start, current = None, False, None, None
    while index < len(lines):
        line, line_start, line_end = lines[index]
        heading = None if fence is not None or comment else _notes_heading(lines, index)
        if heading is None:
            index += 1
            fence, comment = _notes_nonheading(line, fence, comment)
            if line_end == len(text):
                break
            continue
        level, title, setext = heading
        if release_level is not None:
            if level > release_level:
                index += 1
                if line_end == len(text):
                    break
                continue
            if level < release_level:
                index += 1
                if start is not None:
                    notes = text[start:line_start - 1].rstrip(_NOTES_SPACE) if start < line_start else ""
                    return (*current, notes), index, release_level
                if line_end == len(text):
                    break
                continue
            if start is not None:
                notes = text[start:line_start - 1].rstrip(_NOTES_SPACE) if start < line_start else ""
                return (*current, notes), index, release_level
        version = _notes_version(title)
        if _NOTES_VERSION.fullmatch(version) is None:
            index += 1
            if line_end == len(text):
                break
            continue
        current = version, title
        if release_level is None:
            release_level = level
        index += 2 if setext else 1
        while index < len(lines) and not lines[index][0].lstrip(_NOTES_SPACE):
            index += 1
        if index < len(lines):
            start = lines[index][1]
        else:
            break
    if current is not None:
        notes = text[start:].rstrip(_NOTES_SPACE) if start is not None else ""
        return (*current, notes), index, release_level
    return None, index, release_level


def preparation_notes(after, version):
    """Extract latest release notes, bind its version, reject malformed changelogs."""
    require(isinstance(after, bytes) and len(after) <= 2 * 1024 * 1024,
            "proposal_changelog_size")
    require(b"\x00" not in after, "proposal_changelog_nul")
    text = after.decode("utf-8")
    lines, index, level, releases, seen = _notes_lines(text), 0, None, [], set()
    while index < len(lines):
        release, index, level = _notes_next(text, lines, index, level)
        if release is None:
            break
        require(release[0] not in seen, "proposal_changelog_duplicate_version")
        seen.add(release[0])
        releases.append(release)
    require(releases, "proposal_changelog_no_release")
    selected = 1 if "unreleased" in releases[0][0].lower() else 0
    require(selected < len(releases), "proposal_changelog_no_release")
    require(releases[selected][0] == version, "proposal_changelog_version")
    return releases[selected][2]
