use std::collections::{BTreeMap, BTreeSet};

use super::{StepLayout, anchor_token, indent, step_field_content};

pub(super) struct StepRun {
    pub(super) scalar: Option<String>,
    body_lines: Option<std::ops::Range<usize>>,
    pub(super) malformed_body: bool,
    pub(super) invalid_anchor: bool,
}

impl StepRun {
    pub(super) fn is_body_line(&self, index: usize) -> bool {
        self.body_lines
            .as_ref()
            .is_some_and(|lines| lines.contains(&index))
    }
}

pub(super) fn step_run_scalar(
    block: &[&str],
    layout: &StepLayout,
    run_anchors: &mut BTreeMap<String, Option<String>>,
    duplicate_anchors: &BTreeSet<String>,
) -> StepRun {
    let mut result = StepRun {
        scalar: None,
        body_lines: None,
        malformed_body: false,
        invalid_anchor: false,
    };
    let mut found_run = false;
    for (index, line) in block.iter().enumerate() {
        if (index != 0 || !layout.head_field) && indent(line) != layout.mapping_indent {
            continue;
        }
        let trimmed = step_field_content(index, line, block[0]).trim();
        let Some(raw) = trimmed.strip_prefix("run: ") else {
            continue;
        };
        if found_run {
            result.malformed_body = true;
            continue;
        }
        found_run = true;
        let (anchor, value) = anchored_value(raw);
        if anchor == Some("") {
            result.malformed_body = true;
            result.invalid_anchor = true;
            result.scalar = Some(value.to_owned());
            continue;
        }
        if value.starts_with('*') {
            result.scalar = anchor_token(value, '*').and_then(|(alias, remainder)| {
                if alias.is_empty() || !remainder.is_empty() || duplicate_anchors.contains(alias) {
                    None
                } else {
                    run_anchors.get(alias).and_then(Clone::clone)
                }
            });
            result.malformed_body |= result.scalar.is_none();
            result.invalid_anchor |= result.scalar.is_none();
            continue;
        }
        if value == "|" {
            let (scalar, body_lines, malformed) =
                literal_block(block, index, layout.mapping_indent);
            result.scalar = Some(scalar);
            result.body_lines = Some(body_lines);
            result.malformed_body |= malformed;
        } else {
            result.scalar = Some(value.to_owned());
        }
        if let Some(anchor) = anchor {
            if duplicate_anchors.contains(anchor) || run_anchors.contains_key(anchor) {
                result.malformed_body = true;
                result.invalid_anchor = true;
                run_anchors.insert(anchor.to_owned(), None);
            } else if let Some(scalar) = &result.scalar {
                run_anchors.insert(anchor.to_owned(), Some(scalar.clone()));
            } else {
                run_anchors.insert(anchor.to_owned(), None);
            }
        }
    }
    result
}

fn anchored_value(raw: &str) -> (Option<&str>, &str) {
    let Some((anchor, value)) = anchor_token(raw, '&') else {
        return (None, raw);
    };
    (Some(anchor), value.trim_start())
}

fn literal_block(
    block: &[&str],
    field_index: usize,
    mapping_indent: usize,
) -> (String, std::ops::Range<usize>, bool) {
    let end = (field_index + 1..block.len())
        .find(|index| !block[*index].trim().is_empty() && indent(block[*index]) <= mapping_indent)
        .unwrap_or(block.len());
    let body_lines = field_index + 1..end;
    let body_indent = body_lines
        .clone()
        .filter(|index| !block[*index].trim().is_empty())
        .map(|index| indent(block[index]))
        .next()
        .unwrap_or(mapping_indent + 1);
    let malformed = body_lines
        .clone()
        .filter(|index| !block[*index].trim().is_empty())
        .any(|index| indent(block[index]) < body_indent);
    let body = body_lines
        .clone()
        .map(|index| {
            let line = block[index];
            if line.trim().is_empty() {
                String::new()
            } else {
                line.get(body_indent..).unwrap_or_default().to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    (format!("{body}\n"), body_lines, malformed)
}
