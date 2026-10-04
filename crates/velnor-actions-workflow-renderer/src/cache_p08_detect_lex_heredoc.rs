//! Skip unsupported here-doc bodies so their text cannot become cache keys.

pub(super) fn skip_heredocs(
    chars: &[char],
    mut cursor: usize,
    heredocs: &mut Vec<(String, bool)>,
    unsupported: &mut bool,
) -> usize {
    for (delimiter, strip_tabs) in std::mem::take(heredocs) {
        let Some(after_body) = skip_heredoc_body(chars, cursor, &delimiter, strip_tabs) else {
            *unsupported = true;
            return chars.len();
        };
        cursor = after_body;
    }
    cursor
}

pub(super) fn heredoc_delimiter(chars: &[char], mut cursor: usize) -> Option<String> {
    while chars.get(cursor).is_some_and(|ch| matches!(ch, ' ' | '\t')) {
        cursor += 1;
    }
    let quote = match chars.get(cursor).copied()? {
        '\'' => Some('\''),
        '"' => Some('"'),
        _ => None,
    };
    if quote.is_some() {
        cursor += 1;
    }
    let start = cursor;
    while let Some(ch) = chars.get(cursor) {
        if Some(*ch) == quote || quote.is_none() && ch.is_whitespace() {
            break;
        }
        cursor += 1;
    }
    let delimiter: String = chars[start..cursor].iter().copied().collect();
    (!delimiter.is_empty()).then_some(delimiter)
}

fn skip_heredoc_body(
    chars: &[char],
    mut cursor: usize,
    delimiter: &str,
    strip_tabs: bool,
) -> Option<usize> {
    while cursor <= chars.len() {
        let line_end = chars[cursor..]
            .iter()
            .position(|ch| *ch == '\n')
            .map_or(chars.len(), |offset| cursor + offset);
        let mut line = &chars[cursor..line_end];
        if strip_tabs {
            while line.first() == Some(&'\t') {
                line = &line[1..];
            }
        }
        if line.iter().copied().collect::<String>() == delimiter {
            return Some((line_end + usize::from(line_end < chars.len())).min(chars.len()));
        }
        if line_end == chars.len() {
            return None;
        }
        cursor = line_end + 1;
    }
    None
}
