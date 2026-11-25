// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::Reason;

const MAX_ACTORS: usize = 8;
const MAX_LOGIN: usize = 39;
const KEY: &str = "image.release_actors";

fn login(text: &str) -> bool {
    let name = text.strip_suffix("[bot]").unwrap_or(text);
    !name.is_empty()
        && name.len() <= MAX_LOGIN
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub(crate) fn parse(raw: &[String]) -> Result<Vec<String>, Reason> {
    let invalid = |value: &str| Reason::InvalidValue {
        key: KEY,
        value: value.to_owned(),
    };
    if raw.len() > MAX_ACTORS {
        return Err(invalid("more than 8 entries"));
    }
    let mut actors: Vec<String> = Vec::new();
    for actor in raw {
        if !login(actor) || actors.iter().any(|other| other.eq_ignore_ascii_case(actor)) {
            return Err(invalid(actor));
        }
        actors.push(actor.clone());
    }
    Ok(actors)
}
