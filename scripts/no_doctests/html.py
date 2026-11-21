import re


BLOCK_TAGS = (
    "address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|"
    "fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|"
    "menuitem|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul"
)
ATTRIBUTE = r"(?:\s+[A-Za-z_:][\w.:-]*(?:\s*=\s*(?:[^\s\"'=<>`]+|'[^']*'|\"[^\"]*\"))?)"
STARTS = [
    (re.compile(r"<(?:script|pre|style|textarea)(?:\s|>|$)", re.I), re.compile(r"</(?:script|pre|style|textarea)>", re.I), True),
    (re.compile(r"<!--"), re.compile(r"-->"), True),
    (re.compile(r"<\?"), re.compile(r"\?>"), True),
    (re.compile(r"<![A-Za-z]"), re.compile(r">"), True),
    (re.compile(r"<!\[CDATA\["), re.compile(r"\]\]>"), True),
    (re.compile(r"</?(?:" + BLOCK_TAGS + r")(?:\s|/?>|$)", re.I), None, True),
    (re.compile(r"</?[A-Za-z][A-Za-z0-9-]*" + ATTRIBUTE + r"*\s*/?>\s*$"), None, False),
]


BLANK = "blank"


def html_block(text, paragraph):
    for start, end, interrupts in STARTS:
        if start.match(text) and (interrupts or not paragraph):
            if end is None:
                return True, BLANK
            return True, None if end.search(text, 1) else end
    return False, None
