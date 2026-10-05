//! Lexer for the workflow renderer's fixed inline-shell subset.

use super::heredoc;
use super::{
    DetectedCommand, Detection, command_has_dynamic_executable, command_has_unmodeled_mise_payload,
    command_has_unsupported_launcher, command_starts_mise,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
}

enum Token {
    Word(String),
    Redirect,
    Boundary,
}

/// Tokenize supported shell words, operators, and command substitutions.
pub(super) fn detect_script(script: &str, depth: usize) -> Detection {
    Scanner::new(script, depth).run()
}

struct Scanner {
    chars: Vec<char>,
    depth: usize,
    cursor: usize,
    quote: Quote,
    word: String,
    in_word: bool,
    current: Vec<Token>,
    commands: Vec<Vec<Token>>,
    nested: Vec<DetectedCommand>,
    heredocs: Vec<(String, bool)>,
    unsupported: bool,
    unmodeled_execution: bool,
}

impl Scanner {
    fn new(script: &str, depth: usize) -> Self {
        Self {
            chars: script.chars().collect(),
            depth,
            cursor: 0,
            quote: Quote::None,
            word: String::new(),
            in_word: false,
            current: Vec::new(),
            commands: Vec::new(),
            nested: Vec::new(),
            heredocs: Vec::new(),
            unsupported: false,
            unmodeled_execution: false,
        }
    }

    fn run(mut self) -> Detection {
        while self.cursor < self.chars.len() {
            self.scan_char();
        }
        self.finish()
    }

    fn scan_char(&mut self) {
        let ch = self.chars[self.cursor];
        match self.quote {
            Quote::Single => self.scan_single(ch),
            Quote::Double => self.scan_double(ch),
            Quote::None => self.scan_unquoted(ch),
        }
    }

    fn scan_single(&mut self, ch: char) {
        if ch == '\'' {
            self.quote = Quote::None;
        } else {
            self.word.push(ch);
        }
        self.cursor += 1;
    }

    fn scan_double(&mut self, ch: char) {
        if ch == '"' {
            self.quote = Quote::None;
            self.cursor += 1;
        } else if ch == '`' {
            self.unsupported = true;
            self.unmodeled_execution = true;
            self.word.push(ch);
            self.cursor += 1;
        } else if ch == '\\' && self.cursor + 1 < self.chars.len() {
            self.cursor += 1;
            self.word.push(self.chars[self.cursor]);
            self.cursor += 1;
        } else if ch == '$' && self.chars.get(self.cursor + 1) == Some(&'(') {
            self.scan_substitution();
        } else {
            self.word.push(ch);
            self.cursor += 1;
        }
    }

    fn scan_unquoted(&mut self, ch: char) {
        if ch == '\'' || ch == '"' {
            self.in_word = true;
            self.quote = if ch == '\'' {
                Quote::Single
            } else {
                Quote::Double
            };
            self.cursor += 1;
        } else if ch == '\\' && self.cursor + 1 < self.chars.len() {
            self.cursor += 1;
            if self.chars[self.cursor] != '\n' {
                self.word.push(self.chars[self.cursor]);
                self.in_word = true;
            }
            self.cursor += 1;
        } else if ch == '`' {
            self.unsupported = true;
            self.unmodeled_execution = true;
            self.word.push(ch);
            self.in_word = true;
            self.cursor += 1;
        } else if ch == '$' && self.chars.get(self.cursor + 1) == Some(&'(') {
            self.in_word = true;
            self.scan_substitution();
        } else if ch == '#' && !self.in_word {
            while self.cursor < self.chars.len() && self.chars[self.cursor] != '\n' {
                self.cursor += 1;
            }
        } else if ch.is_whitespace() {
            self.scan_whitespace(ch);
        } else if matches!(ch, '<' | '>') && self.chars.get(self.cursor + 1) == Some(&'(') {
            // Process substitution can launch a hidden command.
            self.unsupported = true;
            self.unmodeled_execution = true;
            self.scan_operator(ch, 1, false);
        } else if let Some((length, boundary)) = shell_operator(&self.chars, self.cursor) {
            self.scan_operator(ch, length, boundary);
        } else {
            self.word.push(ch);
            self.in_word = true;
            self.cursor += 1;
        }
    }

    fn scan_substitution(&mut self) {
        let (body, end) = substitution_body(&self.chars, self.cursor + 2);
        if let Some((body, end)) = body.zip(end) {
            let sub = detect_script(&body, self.depth + 1);
            self.nested.extend(sub.commands);
            self.unsupported |= sub.unsupported_mise_syntax;
            self.word.push_str("$()");
            self.cursor = end + 1;
        } else {
            self.unsupported = true;
            self.unmodeled_execution = true;
            self.word.push_str("$(");
            self.cursor += 2;
        }
    }

    fn scan_whitespace(&mut self, ch: char) {
        flush_word(&mut self.word, &mut self.in_word, &mut self.current);
        if ch == '\n' {
            finish_command(&mut self.current, &mut self.commands);
            self.cursor = heredoc::skip_heredocs(
                &self.chars,
                self.cursor + 1,
                &mut self.heredocs,
                &mut self.unsupported,
            );
        } else {
            self.cursor += 1;
        }
    }

    fn scan_operator(&mut self, ch: char, length: usize, boundary: bool) {
        let io_number = matches!(ch, '<' | '>')
            && self.in_word
            && !self.word.is_empty()
            && self.word.bytes().all(|byte| byte.is_ascii_digit());
        if io_number {
            self.word.clear();
            self.in_word = false;
        } else {
            flush_word(&mut self.word, &mut self.in_word, &mut self.current);
        }
        if ch == ';' && self.chars.get(self.cursor + 1) == Some(&';') {
            self.unsupported = true;
        }
        if ch == '<' && self.chars.get(self.cursor + 1) == Some(&'<') {
            self.unsupported = true;
            self.unmodeled_execution = true;
            let strip_tabs = self.chars.get(self.cursor + 2) == Some(&'-');
            if let Some(delimiter) = heredoc::heredoc_delimiter(&self.chars, self.cursor + length) {
                self.heredocs.push((delimiter, strip_tabs));
            }
        }
        self.current.push(if boundary {
            Token::Boundary
        } else {
            Token::Redirect
        });
        if boundary {
            finish_command(&mut self.current, &mut self.commands);
        }
        self.cursor += length;
    }

    fn finish(mut self) -> Detection {
        self.unsupported |= self.quote != Quote::None;
        flush_word(&mut self.word, &mut self.in_word, &mut self.current);
        finish_command(&mut self.current, &mut self.commands);
        let mut parsed = self
            .commands
            .into_iter()
            .map(command_words)
            .collect::<Vec<_>>();
        parsed.extend(self.nested);
        if self.depth > 16 || parsed.iter().any(is_unsupported_control_command) {
            self.unsupported = true;
        }
        let has_mise_command = parsed
            .iter()
            .any(|command| command_starts_mise(&command.words));
        let has_unmodeled_head = parsed.iter().any(|command| {
            let starts_mise = command_starts_mise(&command.words);
            command_has_dynamic_executable(&command.words)
                || command_has_unsupported_launcher(&command.words)
                // Direct Mise argv owns its `exec --` payload parsing.
                || !starts_mise && command_has_unmodeled_mise_payload(&command.words)
        });
        Detection {
            unsupported_mise_syntax: self.unmodeled_execution
                || has_unmodeled_head
                || self.unsupported && has_mise_command,
            commands: parsed,
        }
    }
}

fn is_unsupported_control_command(command: &DetectedCommand) -> bool {
    let Some(first) = command.words.first() else {
        return false;
    };
    let control = {
        let word = first;
        matches!(
            word.as_str(),
            "if" | "then"
                | "elif"
                | "else"
                | "fi"
                | "for"
                | "while"
                | "until"
                | "do"
                | "done"
                | "case"
                | "esac"
                | "select"
                | "function"
                | "coproc"
                | "time"
                | "!"
                | "[["
                | "]]"
                | "(("
                | "))"
        )
    };
    control
}

/// Find the matching close parenthesis for a command substitution.
fn substitution_body(chars: &[char], start: usize) -> (Option<String>, Option<usize>) {
    let mut depth = 1_u8;
    let mut quote = Quote::None;
    let mut cursor = start;
    while cursor < chars.len() {
        let ch = chars[cursor];
        match quote {
            Quote::Single if ch == '\'' => quote = Quote::None,
            Quote::Double if ch == '\\' => cursor += 1,
            Quote::Double if ch == '"' => quote = Quote::None,
            Quote::None if ch == '\'' => quote = Quote::Single,
            Quote::None if ch == '"' => quote = Quote::Double,
            Quote::None if ch == '#' => {
                while cursor < chars.len() && chars[cursor] != '\n' {
                    cursor += 1;
                }
                continue;
            }
            Quote::None | Quote::Double if ch == '$' && chars.get(cursor + 1) == Some(&'(') => {
                let Some(next_depth) = depth.checked_add(1) else {
                    return (None, None);
                };
                depth = next_depth;
                if depth > 16 {
                    return (None, None);
                }
                cursor += 1;
            }
            Quote::None if ch == ')' => {
                depth -= 1;
                if depth == 0 {
                    return (
                        Some(chars[start..cursor].iter().copied().collect()),
                        Some(cursor),
                    );
                }
            }
            Quote::None if ch == '\\' => cursor += 1,
            _ => {}
        }
        cursor += 1;
    }
    (None, None)
}

fn shell_operator(chars: &[char], cursor: usize) -> Option<(usize, bool)> {
    let ch = *chars.get(cursor)?;
    let next = chars.get(cursor + 1).copied();
    match ch {
        ';' if next == Some(';') => Some((2, false)),
        ';' => Some((1, true)),
        '&' if next == Some('&') => Some((2, true)),
        '&' if next == Some('>') => Some((2, false)),
        '&' => Some((1, true)),
        '|' if next == Some('|') || next == Some('&') => Some((2, true)),
        '|' => Some((1, true)),
        '(' | ')' => Some((1, true)),
        '{' | '}' if is_delimited(chars, cursor) => Some((1, true)),
        '>' if next == Some('&') || next == Some('>') || next == Some('|') => Some((2, false)),
        '>' => Some((1, false)),
        '<' if next == Some('<') && chars.get(cursor + 2) == Some(&'-') => Some((3, false)),
        '<' if next == Some('&') || next == Some('<') => Some((2, false)),
        '<' => Some((1, false)),
        _ => None,
    }
}

fn is_delimited(chars: &[char], cursor: usize) -> bool {
    let before = cursor == 0 || chars[cursor - 1].is_whitespace();
    let after = chars
        .get(cursor + 1)
        .is_none_or(|ch| ch.is_whitespace() || matches!(*ch, ';' | '|' | '&'));
    before && after
}

fn flush_word(word: &mut String, in_word: &mut bool, current: &mut Vec<Token>) {
    if *in_word {
        current.push(Token::Word(std::mem::take(word)));
        *in_word = false;
    }
}

fn finish_command(current: &mut Vec<Token>, commands: &mut Vec<Vec<Token>>) {
    if !current.is_empty() {
        commands.push(std::mem::take(current));
    }
}

fn command_words(tokens: Vec<Token>) -> DetectedCommand {
    let mut words = Vec::new();
    let mut redirect_target = false;
    for token in tokens {
        match token {
            Token::Word(_word) if redirect_target => redirect_target = false,
            Token::Word(word) => words.push(word),
            Token::Redirect => redirect_target = true,
            Token::Boundary => {}
        }
    }
    DetectedCommand { words }
}
