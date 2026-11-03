// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Default)]
pub(super) struct Command(Vec<String>);

impl Command {
    pub(super) fn words(&self) -> &[String] {
        &self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
}

#[derive(Default)]
struct Lexer {
    commands: Vec<Command>,
    words: Vec<String>,
    word: String,
    open: bool,
}

impl Lexer {
    fn end_word(&mut self) {
        if self.open {
            self.words.push(std::mem::take(&mut self.word));
            self.open = false;
        }
    }

    fn end_command(&mut self) {
        self.end_word();
        if !self.words.is_empty() {
            self.commands.push(Command(std::mem::take(&mut self.words)));
        }
    }

    fn push(&mut self, ch: char) {
        self.word.push(ch);
        self.open = true;
    }
}

fn in_double(lexer: &mut Lexer, ch: char, chars: &mut std::str::Chars<'_>) -> Quote {
    match ch {
        '"' => return Quote::None,
        '\\' => match chars.next() {
            Some(next @ ('"' | '\\' | '$' | '`')) => lexer.push(next),
            Some(next) => {
                lexer.push('\\');
                lexer.push(next);
            }
            None => lexer.push('\\'),
        },
        other => lexer.push(other),
    }
    Quote::Double
}

pub(super) fn commands_of(text: &str) -> Vec<Command> {
    let mut lexer = Lexer::default();
    let mut nested = Vec::new();
    let mut quote = Quote::None;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        quote = match (quote, ch) {
            (Quote::Single, '\'') => Quote::None,
            (Quote::Single, other) => {
                lexer.push(other);
                Quote::Single
            }
            (Quote::Double, _) => in_double(&mut lexer, ch, &mut chars),
            (Quote::None, '\'') => {
                lexer.open = true;
                Quote::Single
            }
            (Quote::None, '"') => {
                lexer.open = true;
                Quote::Double
            }
            (Quote::None, '\\') => {
                if let Some(next) = chars.next() {
                    lexer.push(next);
                }
                Quote::None
            }
            (Quote::None, ';' | '&' | '|' | '\n') => {
                lexer.end_command();
                Quote::None
            }
            (Quote::None, space) if space.is_whitespace() => {
                lexer.end_word();
                Quote::None
            }
            (Quote::None, other) => {
                lexer.push(other);
                Quote::None
            }
        };
    }
    lexer.end_command();
    for command in &lexer.commands {
        for word in command.words() {
            if word.contains(char::is_whitespace) {
                nested.extend(commands_of(word));
            }
        }
    }
    lexer.commands.extend(nested);
    lexer.commands
}
