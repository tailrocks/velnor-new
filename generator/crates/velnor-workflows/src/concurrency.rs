// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};
use crate::plan::Concurrency;
use crate::raw::RawConcurrency;

const CONTEXTS: [&str; 4] = [
    "github.workflow",
    "github.ref",
    "github.event.pull_request.number",
    "github.head_ref || github.run_id",
];
const TELLS_REFS_APART: [&str; 2] = ["github.ref", "github.head_ref || github.run_id"];
const MAX_GROUP_BYTES: usize = 200;

pub(crate) fn concurrency(place: &str, raw: RawConcurrency) -> Result<Concurrency, WorkflowsError> {
    let contexts = contexts(place, &raw.group)?;
    if raw.cancel_in_progress && !contexts.iter().any(|name| TELLS_REFS_APART.contains(name)) {
        return refuse(format!(
            "the concurrency group {:?} of {place} cancels runs in progress but does not tell refs apart; add ${{{{ github.ref }}}} or ${{{{ github.head_ref || github.run_id }}}}, or a run on one branch would cancel runs on every other",
            raw.group
        ));
    }
    Ok(Concurrency {
        group: raw.group,
        cancel_in_progress: raw.cancel_in_progress,
    })
}

fn contexts(place: &str, group: &str) -> Result<Vec<&'static str>, WorkflowsError> {
    let bad = |why: &str| {
        refuse(format!(
            "the concurrency group {group:?} of {place} {why}; a group is plain text of letters, digits, and - _ . : / plus these expressions: {CONTEXTS:?}"
        ))
    };
    if group.is_empty() || group.len() > MAX_GROUP_BYTES {
        return bad("is empty or longer than 200 bytes");
    }
    let mut found = Vec::new();
    let mut rest = group;
    while !rest.is_empty() {
        let (literal, tail) = rest.split_once("${{").unwrap_or((rest, ""));
        if !literal.bytes().all(literal_byte) {
            return bad("has a character outside the plain text set");
        }
        if literal.len() == rest.len() {
            break;
        }
        let Some((inner, after)) = tail.split_once("}}") else {
            return bad("has an unterminated ${{");
        };
        let inner = inner.split_whitespace().collect::<Vec<_>>().join(" ");
        let Some(context) = CONTEXTS.iter().find(|context| **context == inner) else {
            return refuse(format!(
                "the concurrency group {group:?} of {place} reads {}, which is not allowed; allowed expressions are {CONTEXTS:?}",
                if inner.is_empty() {
                    "an empty expression"
                } else {
                    &inner
                }
            ));
        };
        found.push(*context);
        rest = after;
    }
    Ok(found)
}

fn literal_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
}
