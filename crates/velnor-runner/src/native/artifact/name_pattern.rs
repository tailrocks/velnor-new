// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::inputs::DOWNLOAD;
use super::js_text;

const MAX_ALTERNATIVES: usize = 256;
const MAX_EXPANDED_TOKENS: usize = 65_536;
const MAX_UNITS: usize = 65_536;
const MAX_DEPTH: usize = 32;
const SLASH: u16 = b'/' as u16;
const DOT: u16 = b'.' as u16;

#[derive(Debug, Clone, Copy)]
struct Class {
    negated: bool,
    members: u128,
}

impl Class {
    fn accepts(self, unit: u16) -> bool {
        (unit < 128 && self.members >> unit & 1 == 1) != self.negated
    }
}

#[derive(Debug, Clone, Copy)]
enum Token {
    Star,
    Any,
    Lit(u16),
    Class(Class),
}

impl Token {
    fn accepts(&self, unit: u16) -> bool {
        unit != SLASH
            && match self {
                Self::Star | Self::Any => true,
                Self::Lit(expected) => *expected == unit,
                Self::Class(class) => class.accepts(unit),
            }
    }
}

#[derive(Debug)]
enum Item {
    Token(Token),
    Group(Vec<Vec<Item>>),
}

enum Refusal {
    Syntax,
    TooMany,
    TooLarge,
    TooLong,
    TooDeep,
}

struct Parser<'a> {
    chars: &'a [char],
    at: usize,
    depth: usize,
}

fn member(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '.')
}

fn same_kind(low: char, high: char) -> bool {
    (low.is_ascii_digit() && high.is_ascii_digit())
        || (low.is_ascii_lowercase() && high.is_ascii_lowercase())
        || (low.is_ascii_uppercase() && high.is_ascii_uppercase())
}

fn unit(character: char) -> u16 {
    u16::try_from(u32::from(character)).unwrap_or_default()
}

impl Parser<'_> {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.at + offset).copied()
    }

    fn sequence(&mut self, inside_group: bool) -> Result<Vec<Item>, Refusal> {
        let mut items = Vec::new();
        while let Some(character) = self.peek(0) {
            match character {
                ',' | '}' if inside_group => break,
                '\\' | '/' | '(' | ')' | ']' | '}' | '!' | '#' => {
                    return Err(Refusal::Syntax);
                }
                '[' => items.push(Item::Token(self.class()?)),
                '{' if self.at > 0 && self.chars.get(self.at - 1) == Some(&'$') => {
                    return Err(Refusal::Syntax);
                }
                '{' => items.push(self.group()?),
                '*' => {
                    self.at += 1;
                    items.push(Item::Token(Token::Star));
                }
                '?' => {
                    self.at += 1;
                    items.push(Item::Token(Token::Any));
                }
                other => {
                    self.at += 1;
                    let mut units = [0_u16; 2];
                    items.extend(
                        other
                            .encode_utf16(&mut units)
                            .iter()
                            .map(|unit| Item::Token(Token::Lit(*unit))),
                    );
                }
            }
        }
        Ok(items)
    }

    fn group(&mut self) -> Result<Item, Refusal> {
        self.at += 1;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Refusal::TooDeep);
        }
        let mut options = vec![self.sequence(true)?];
        loop {
            match self.peek(0) {
                Some(',') => {
                    self.at += 1;
                    options.push(self.sequence(true)?);
                }
                Some('}') => {
                    self.at += 1;
                    break;
                }
                _ => return Err(Refusal::Syntax),
            }
        }
        if options.len() < 2 {
            return Err(Refusal::Syntax);
        }
        self.depth -= 1;
        Ok(Item::Group(options))
    }

    fn class(&mut self) -> Result<Token, Refusal> {
        self.at += 1;
        let negated = matches!(self.peek(0), Some('!' | '^'));
        if negated {
            self.at += 1;
        }
        let mut members = 0_u128;
        let mut listed = 0_usize;
        loop {
            let low = self.peek(0).ok_or(Refusal::Syntax)?;
            self.at += 1;
            if low == ']' && members != 0 {
                return Ok(if !negated && listed == 1 {
                    Token::Lit(u16::try_from(members.trailing_zeros()).unwrap_or_default())
                } else {
                    Token::Class(Class { negated, members })
                });
            }
            if !member(low) {
                return Err(Refusal::Syntax);
            }
            let high = match (self.peek(0), self.peek(1)) {
                (Some('-'), Some(high)) if high != ']' => {
                    self.at += 2;
                    high
                }
                _ => low,
            };
            if !member(high) || (low != high && !same_kind(low, high)) || low > high {
                return Err(Refusal::Syntax);
            }
            listed += 1 + usize::from(low != high);
            for code in unit(low)..=unit(high) {
                members |= 1_u128 << code;
            }
        }
    }
}

fn expanded_size(items: &[Item]) -> (usize, usize) {
    items
        .iter()
        .fold((1_usize, 0_usize), |(count, tokens), item| match item {
            Item::Token(_) => (count, tokens.saturating_add(count)),
            Item::Group(options) => {
                let (group_count, group_tokens) =
                    options
                        .iter()
                        .fold((0_usize, 0_usize), |(sum, total), option| {
                            let (option_count, option_tokens) = expanded_size(option);
                            (
                                sum.saturating_add(option_count),
                                total.saturating_add(option_tokens),
                            )
                        });
                (
                    count.saturating_mul(group_count),
                    tokens
                        .saturating_mul(group_count)
                        .saturating_add(count.saturating_mul(group_tokens)),
                )
            }
        })
}

fn flatten(items: &[Item]) -> Vec<Vec<Token>> {
    items
        .iter()
        .fold(vec![Vec::new()], |built, item| match item {
            Item::Token(token) => built
                .into_iter()
                .map(|mut head| {
                    head.push(*token);
                    head
                })
                .collect(),
            Item::Group(options) => {
                let tails: Vec<Vec<Token>> =
                    options.iter().flat_map(|option| flatten(option)).collect();
                built
                    .iter()
                    .flat_map(|head| tails.iter().map(|tail| [head.as_slice(), tail].concat()))
                    .collect()
            }
        })
}

#[derive(Debug)]
struct Alternative {
    tokens: Vec<Token>,
    dotted: bool,
    literal: bool,
}

impl Alternative {
    fn matches(&self, name: &[u16]) -> bool {
        if name.first() == Some(&DOT) && !self.dotted {
            return false;
        }
        if matches!(name, [DOT] | [DOT, DOT]) && !self.literal {
            return false;
        }
        let (mut token, mut at) = (0_usize, 0_usize);
        let mut resume: Option<(usize, usize)> = None;
        while at < name.len() {
            match self.tokens.get(token) {
                Some(Token::Star) => {
                    resume = Some((token, at));
                    token += 1;
                }
                Some(expected) if expected.accepts(name[at]) => {
                    token += 1;
                    at += 1;
                }
                _ => match resume {
                    Some((star, from)) if name[from] != SLASH => {
                        resume = Some((star, from + 1));
                        token = star + 1;
                        at = from + 1;
                    }
                    _ => return false,
                },
            }
        }
        self.tokens[token..]
            .iter()
            .all(|rest| matches!(rest, Token::Star))
    }
}

#[derive(Debug)]
pub(super) struct NamePattern {
    alternatives: Vec<Alternative>,
}

fn refusal(pattern: &str, reason: &Refusal) -> String {
    match reason {
        Refusal::Syntax => format!(
            "{DOWNLOAD} pattern '{pattern}' uses minimatch syntax that is not supported by velnor-runner; supported are literal characters, a single *, ?, [abc] and [a-z] classes, and {{a,b}} lists"
        ),
        Refusal::TooMany => format!(
            "{DOWNLOAD} pattern '{pattern}' expands to more than {MAX_ALTERNATIVES} brace alternatives, which velnor-runner refuses"
        ),
        Refusal::TooLarge => format!(
            "{DOWNLOAD} pattern expands to more than {MAX_EXPANDED_TOKENS} tokens in all its brace alternatives, which velnor-runner refuses"
        ),
        Refusal::TooLong => format!(
            "{DOWNLOAD} pattern is longer than {MAX_UNITS} UTF-16 code units, which velnor-runner refuses"
        ),
        Refusal::TooDeep => format!(
            "{DOWNLOAD} pattern '{pattern}' nests brace lists deeper than {MAX_DEPTH} levels, which velnor-runner refuses"
        ),
    }
}

impl NamePattern {
    pub(super) fn parse(pattern: &str) -> Result<Self, String> {
        let pattern = js_text::trim(pattern);
        if pattern.encode_utf16().count() > MAX_UNITS {
            return Err(refusal(pattern, &Refusal::TooLong));
        }
        let chars: Vec<char> = pattern.chars().collect();
        let items = Parser {
            chars: &chars,
            at: 0,
            depth: 0,
        }
        .sequence(false)
        .map_err(|reason| refusal(pattern, &reason))?;
        let (count, tokens) = expanded_size(&items);
        if count > MAX_ALTERNATIVES {
            return Err(refusal(pattern, &Refusal::TooMany));
        }
        if tokens > MAX_EXPANDED_TOKENS {
            return Err(refusal(pattern, &Refusal::TooLarge));
        }
        let expanded = flatten(&items);
        if expanded.iter().any(|tokens| {
            tokens
                .windows(2)
                .any(|pair| matches!(pair, [Token::Star, Token::Star]))
        }) {
            return Err(refusal(pattern, &Refusal::Syntax));
        }
        let alternatives = expanded
            .into_iter()
            .map(|tokens| Alternative {
                dotted: matches!(tokens.first(), Some(Token::Lit(DOT))),
                literal: tokens.iter().all(|token| matches!(token, Token::Lit(_))),
                tokens,
            })
            .collect();
        Ok(Self { alternatives })
    }

    pub(super) fn matches(&self, name: &str) -> bool {
        let units: Vec<u16> = name.encode_utf16().collect();
        self.alternatives
            .iter()
            .any(|alternative| alternative.matches(&units))
    }
}

pub(super) fn unsupported_pattern(pattern: &str) -> Option<String> {
    NamePattern::parse(pattern).err()
}

#[cfg(test)]
mod tests {
    use super::{Parser, Token};

    fn accepted(pattern: &str) -> Vec<u16> {
        let chars: Vec<char> = pattern.chars().collect();
        let token = Parser {
            chars: &chars,
            at: 0,
            depth: 0,
        }
        .class();
        let Ok(Token::Class(class)) = token else {
            return Vec::new();
        };
        (0..=u16::MAX).filter(|unit| class.accepts(*unit)).collect()
    }

    #[test]
    fn a_class_accepts_exactly_its_members_among_all_65536_code_units() {
        let listed = |range: std::ops::RangeInclusive<char>| -> Vec<u16> {
            range
                .map(|character| u16::try_from(u32::from(character)).unwrap_or_default())
                .collect()
        };
        assert_eq!(accepted("[a-c]"), listed('a'..='c'));
        assert_eq!(accepted("[ab]"), listed('a'..='b'));
        let negated = accepted("[!a-c]");
        assert_eq!(negated.len(), 65_536 - 3);
        assert!(negated.iter().all(|unit| !(97..=99).contains(unit)));
        assert_eq!(accepted("[^a-c]"), negated);
    }
}
