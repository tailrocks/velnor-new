// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const TEST_WORDS: [&str; 9] = [
    "rust",
    ".rust",
    "should_panic",
    "no_run",
    "compile_fail",
    "ignore",
    "test_harness",
    "standalone_crate",
    "fail",
];

pub(crate) fn block_lines(body: &str) -> Vec<&str> {
    let body = body.strip_suffix("*/").unwrap_or(body);
    let mut lines = body.lines();
    let first = lines.next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
    let starred = rest
        .iter()
        .filter(|line| !line.trim().is_empty())
        .all(|line| line.trim_start().starts_with('*'));
    let rest: Vec<&str> = rest
        .into_iter()
        .map(|line| {
            if starred {
                line.trim_start().strip_prefix('*').unwrap_or(line)
            } else {
                line
            }
        })
        .collect();
    let indent = if first.trim().is_empty() {
        rest.iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
            .min()
            .unwrap_or(0)
    } else {
        0
    };
    std::iter::once(first)
        .chain(
            rest.into_iter()
                .map(|line| line.get(indent..).unwrap_or("")),
        )
        .collect()
}

/// What a doc fragment holds: a fenced block, and a Rust example that rustdoc runs.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Shape {
    pub(crate) fenced: bool,
    pub(crate) tested: bool,
}

/// Reads one doc fragment the way `CommonMark` does, one line at a time.
pub(crate) fn shape_of<'a>(lines: impl IntoIterator<Item = &'a str>) -> Shape {
    let mut shape = Shape::default();
    let mut open: Option<(char, usize)> = None;
    let mut blocks = Blocks::default();
    for line in lines {
        shape.fenced |= line.contains("```") || line.contains("~~~");
        if let Some((mark, length)) = open {
            if closes(line, mark, length) {
                open = None;
                blocks.paragraph = false;
            }
        } else if let Some((mark, length, info)) = opens(line) {
            shape.tested |= rust_info(info);
            open = Some((mark, length));
            blocks.paragraph = false;
        } else if blocks.line(&expand_tabs(line)) {
            shape.fenced = true;
            shape.tested = true;
        }
    }
    shape
}

/// Strips the indentation rustdoc strips from a line doc: the least indentation of any non-blank line.
pub(crate) fn unindent(lines: Vec<&str>) -> Vec<&str> {
    let common = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| leading_blanks(line))
        .min()
        .unwrap_or(0);
    lines
        .into_iter()
        .map(|line| line.get(common..).unwrap_or(""))
        .collect()
}

pub(crate) fn tested_text(text: &str) -> bool {
    shape_of(unindent(text.lines().collect())).tested
}

/// Open `CommonMark` containers: list items (by content column) and block quotes.
#[derive(Clone, Copy)]
enum Container {
    Item { content: usize },
    Quote,
}

#[derive(Default)]
struct Blocks {
    containers: Vec<Container>,
    paragraph: bool,
}

impl Blocks {
    /// Reads one line outside a fence. Returns true when it starts an indented code block.
    /// Indentation counts from the open containers: a continuation line indented to a list
    /// item's content column is paragraph text, and an open paragraph is never interrupted by
    /// indented code.
    fn line(&mut self, line: &str) -> bool {
        if line.trim().is_empty() {
            self.paragraph = false;
            return false;
        }
        let mut rest = line;
        let mut col = 0;
        let mut depth = 0;
        for container in &self.containers {
            let step = match *container {
                Container::Item { content } => {
                    let need = content.saturating_sub(col);
                    (leading_blanks(rest) >= need).then_some(need)
                }
                Container::Quote => quote_width(rest),
            };
            let Some(step) = step else { break };
            rest = rest.get(step..).unwrap_or("");
            col += step;
            depth += 1;
        }
        let mut interrupts = self.paragraph;
        loop {
            let indent = leading_blanks(rest);
            if indent >= 4 {
                break;
            }
            let opened = match quote_width(rest) {
                Some(width) => Some((width, Container::Quote)),
                None => marker(&rest[indent..])
                    .filter(|m| !(interrupts && (m.empty || m.start.is_some_and(|n| n != 1))))
                    .map(|m| {
                        let width = indent + m.width;
                        (
                            width,
                            Container::Item {
                                content: col + width,
                            },
                        )
                    }),
            };
            let Some((width, container)) = opened else {
                break;
            };
            self.containers.truncate(depth);
            self.containers.push(container);
            depth += 1;
            rest = rest.get(width..).unwrap_or("");
            col += width;
            self.paragraph = false;
            interrupts = false;
        }
        if rest.trim().is_empty() {
            self.paragraph = false;
            return false;
        }
        if self.paragraph {
            return false;
        }
        let code = leading_blanks(rest) >= 4;
        self.containers.truncate(depth);
        self.paragraph = !code;
        code
    }
}

/// The width of a block quote marker at the start of `text`, with one optional space after it.
fn quote_width(text: &str) -> Option<usize> {
    let indent = leading_blanks(text);
    let body = &text[indent..];
    (indent < 4 && body.starts_with('>'))
        .then(|| indent + 1 + usize::from(body[1..].starts_with(' ')))
}

/// A list marker at the start of `text`: how many bytes it takes with the spaces after it.
struct Marker {
    width: usize,
    empty: bool,
    start: Option<u64>,
}

fn marker(text: &str) -> Option<Marker> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let (length, start) =
        if (1..=9).contains(&digits) && matches!(text.as_bytes().get(digits), Some(b'.' | b')')) {
            (digits + 1, text[..digits].parse::<u64>().ok())
        } else if matches!(text.as_bytes().first(), Some(b'-' | b'*' | b'+')) {
            (1, None)
        } else {
            return None;
        };
    let after = text.get(length..)?;
    let spaces = leading_blanks(after);
    let empty = after.trim().is_empty();
    if spaces == 0 && !empty {
        return None;
    }
    let gap = if empty || spaces > 4 { 1 } else { spaces };
    Some(Marker {
        width: length + gap,
        empty,
        start,
    })
}

fn leading_blanks(text: &str) -> usize {
    text.len() - text.trim_start_matches(' ').len()
}

/// Tabs advance to the next multiple of four columns, as `CommonMark` counts indentation.
fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col = 0;
    for c in line.chars() {
        if c == '\t' {
            let spaces = 4 - col % 4;
            out.extend(std::iter::repeat_n(' ', spaces));
            col += spaces;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

fn quoted(line: &str) -> &str {
    line.trim_start_matches([' ', '\t', '>'])
}

fn run_of(text: &str) -> Option<(char, usize)> {
    let mark = text.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let length = text.chars().take_while(|&c| c == mark).count();
    (length >= 3).then_some((mark, length))
}

fn opens(line: &str) -> Option<(char, usize, &str)> {
    let mut text = quoted(line);
    while let Some(after) = list_marker(text) {
        text = quoted(after);
    }
    let (mark, length) = run_of(text)?;
    let info = &text[length..];
    (mark != '`' || !info.contains('`')).then_some((mark, length, info))
}

fn closes(line: &str, mark: char, length: usize) -> bool {
    let text = quoted(line);
    run_of(text)
        .is_some_and(|(found, run)| found == mark && run >= length && text[run..].trim().is_empty())
}

fn rust_info(info: &str) -> bool {
    let words = || info.split([',', ' ', '\t']).filter(|word| !word.is_empty());
    words().any(|word| matches!(word, "rust" | ".rust"))
        || words().all(|word| {
            TEST_WORDS.contains(&word)
                || word.starts_with("edition")
                || word.starts_with("ignore-")
                || word
                    .strip_prefix('E')
                    .is_some_and(|code| code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit()))
        })
}

fn list_marker(text: &str) -> Option<&str> {
    marker(text).map(|m| text.get(m.width..).unwrap_or(""))
}
