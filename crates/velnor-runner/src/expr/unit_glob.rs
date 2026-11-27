// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::pattern::{GlobError, class_end};

#[derive(Debug, Clone)]
enum Item {
    Star,
    Any,
    Unit(u16),
    Class {
        negated: bool,
        ranges: Vec<(u16, u16)>,
    },
}

impl Item {
    fn accepts(&self, candidate: u16) -> bool {
        match self {
            Item::Star => false,
            Item::Any => true,
            Item::Unit(expected) => *expected == candidate,
            Item::Class { negated, ranges } => {
                ranges
                    .iter()
                    .any(|(low, high)| *low <= candidate && candidate <= *high)
                    != *negated
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UnitGlob {
    items: Vec<Item>,
}

impl UnitGlob {
    pub(crate) fn compile(part: &str) -> Result<Option<Self>, GlobError> {
        let chars: Vec<char> = part.chars().collect();
        let mut items = Vec::new();
        let mut single_unit_items = false;
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '\\' if index + 1 < chars.len() => {
                    push_units(&mut items, chars[index + 1]);
                    index += 2;
                }
                '*' => {
                    items.push(Item::Star);
                    index += 1;
                }
                '?' => {
                    items.push(Item::Any);
                    single_unit_items = true;
                    index += 1;
                }
                '[' => {
                    if let Some(end) = class_end(&chars, index) {
                        if !chars[index..=end].iter().all(char::is_ascii) {
                            return Err(GlobError::NonAsciiClass(part.to_owned()));
                        }
                        items.push(class(&chars[index + 1..end]));
                        single_unit_items = true;
                        index = end + 1;
                    } else {
                        push_units(&mut items, '[');
                        index += 1;
                    }
                }
                character => {
                    push_units(&mut items, character);
                    index += 1;
                }
            }
        }
        Ok(single_unit_items.then_some(Self { items }))
    }

    pub(crate) fn is_match(&self, name: &str) -> bool {
        let name: Vec<u16> = name.encode_utf16().collect();
        let (mut item, mut position) = (0, 0);
        let mut resume: Option<(usize, usize)> = None;
        while position < name.len() {
            match self.items.get(item) {
                Some(Item::Star) => {
                    resume = Some((item + 1, position));
                    item += 1;
                }
                Some(current) if current.accepts(name[position]) => {
                    item += 1;
                    position += 1;
                }
                _ => match resume {
                    Some((after_star, skipped)) => {
                        item = after_star;
                        position = skipped + 1;
                        resume = Some((after_star, skipped + 1));
                    }
                    None => return false,
                },
            }
        }
        self.items[item.min(self.items.len())..]
            .iter()
            .all(|remaining| matches!(remaining, Item::Star))
    }
}

fn push_units(items: &mut Vec<Item>, character: char) {
    items.extend(
        character
            .encode_utf16(&mut [0; 2])
            .iter()
            .copied()
            .map(Item::Unit),
    );
}

fn unit(character: char) -> u16 {
    character.encode_utf16(&mut [0; 2])[0]
}

fn class(inner: &[char]) -> Item {
    let (negated, body) = match inner.split_first() {
        Some(('!' | '^', rest)) => (true, rest),
        _ => (false, inner),
    };
    let mut ranges: Vec<(u16, u16)> = Vec::new();
    let mut in_range = false;
    for (position, character) in body.iter().copied().enumerate() {
        let unit = unit(character);
        match (character, ranges.last_mut()) {
            ('-', _) if position == 0 => ranges.push((unit, unit)),
            ('-', Some(last)) if in_range => {
                last.1 = unit;
                in_range = false;
            }
            ('-', _) => in_range = true,
            (_, Some(last)) if in_range => {
                last.1 = unit;
                in_range = false;
            }
            _ => ranges.push((unit, unit)),
        }
    }
    if in_range {
        let hyphen = unit('-');
        ranges.push((hyphen, hyphen));
    }
    Item::Class { negated, ranges }
}
