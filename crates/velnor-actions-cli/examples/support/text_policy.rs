use unicode_general_category::{GeneralCategory, get_general_category};

pub(super) fn python_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if is_line_break(character) {
            lines.push(&text[start..index]);
            start = index + character.len_utf8();
            if character == '\r'
                && characters
                    .peek()
                    .is_some_and(|(_, next_character)| *next_character == '\n')
                && let Some((line_feed_index, _)) = characters.next()
            {
                start = line_feed_index + 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn is_line_break(character: char) -> bool {
    matches!(
        character,
        '\n' | '\r'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{001c}'
            | '\u{001d}'
            | '\u{001e}'
            | '\u{0085}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

pub(super) fn universal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub(super) fn is_python_whitespace(character: char) -> bool {
    character.is_whitespace() || matches!(character, '\u{001c}'..='\u{001f}')
}

pub(super) fn has_trailer_label(line: &str) -> bool {
    ["co-authored-by", "signed-off-by"]
        .iter()
        .any(|label| has_label_boundary(line, label))
}

fn has_label_boundary(line: &str, label: &str) -> bool {
    let mut actual = line.chars();
    for expected in label.chars() {
        let Some(character) = actual.next() else {
            return false;
        };
        if !regex_ignore_case_char(character, expected) {
            return false;
        }
    }
    actual
        .next()
        .is_none_or(|next| !is_python_word_character(next))
}

fn regex_ignore_case_char(actual: char, expected: char) -> bool {
    actual.eq_ignore_ascii_case(&expected)
        || matches!(
            (actual, expected),
            ('\u{0130}' | '\u{0131}', 'i') | ('\u{017f}', 's') | ('\u{212a}', 'k')
        )
}

fn is_python_word_character(character: char) -> bool {
    character == '_'
        || matches!(
            get_general_category(character),
            GeneralCategory::UppercaseLetter
                | GeneralCategory::LowercaseLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
                | GeneralCategory::DecimalNumber
                | GeneralCategory::LetterNumber
                | GeneralCategory::OtherNumber
        )
}
