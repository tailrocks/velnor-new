use std::collections::{BTreeMap, BTreeSet};

#[path = "rust_install_audit_parse_anchor_names.rs"]
mod anchor_names;

fn anchor_token(value: &str, marker: char) -> Option<(&str, &str)> {
    let value = value.strip_prefix(marker)?;
    let end = value
        .char_indices()
        .find_map(|(index, character)| is_anchor_delimiter(character).then_some(index))
        .unwrap_or(value.len());
    Some((&value[..end], &value[end..]))
}

fn is_anchor_delimiter(character: char) -> bool {
    character.is_whitespace() || is_flow_indicator(character)
}

fn is_flow_indicator(character: char) -> bool {
    matches!(character, ',' | '[' | ']' | '{' | '}')
}

#[path = "rust_install_audit_parse_layout.rs"]
mod layout;
use layout::{
    StepLayout, anchor_name, indent, sequence_head, step_field_content, step_layout, step_name,
};
#[path = "rust_install_audit_parse_scalars.rs"]
mod scalars;
use scalars::{StepRun, step_run_scalar};
#[path = "rust_install_audit_parse_mappings.rs"]
mod mappings;
use mappings::{StepTopFields, step_top_fields};
#[path = "rust_install_audit_parse_anchors.rs"]
mod anchors;
use anchors::duplicate_anchor_names as find_duplicate_anchor_names;

/// One step: its name and full text.
#[derive(Clone)]
pub(super) struct StepText {
    pub(super) name: String,
    pub(super) text: String,
    pub(super) fields: Vec<(String, String)>,
    pub(super) top_fields: Vec<(String, String)>,
    pub(super) env_fields: Vec<(String, String)>,
    pub(super) unexpected_nested: bool,
    pub(super) invalid_anchor: bool,
    pub(super) run_scalar: Option<String>,
}

impl StepText {
    pub(super) fn field(&self, wanted: &str) -> Option<&str> {
        self.fields
            .iter()
            .find_map(|(key, value)| (key == wanted).then_some(value.as_str()))
    }

    pub(super) fn uses_value(&self) -> Option<&str> {
        let raw = self.field("uses")?;
        let value = raw.split_once(" #").map_or(raw, |(value, _)| value).trim();
        Some(
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
                .or_else(|| {
                    value
                        .strip_prefix('"')
                        .and_then(|value| value.strip_suffix('"'))
                })
                .unwrap_or(value),
        )
    }

    pub(super) fn is_verifier_ref_candidate(&self) -> bool {
        self.uses_value()
            .is_some_and(|uses| uses.starts_with("./.github/actions/velnor-verify-"))
    }
}

/// Every `steps:` sequence of a rendered file with its owning job (or
/// `runs` for a composite action), as step texts in order.
pub(super) fn step_lists(text: &str) -> Vec<(String, Vec<StepText>)> {
    let lines: Vec<&str> = text.lines().collect();
    let legacy_anchor_scan = anchor_names::scan(text);
    let malformed_tag = legacy_anchor_scan.malformed_tag;
    let mut duplicate_anchors = find_duplicate_anchor_names(text);
    duplicate_anchors.extend(legacy_anchor_scan.duplicate_names);
    let mut anchors: BTreeMap<String, Option<StepText>> = BTreeMap::new();
    let mut env_anchors: BTreeMap<String, Option<Vec<(String, String)>>> = BTreeMap::new();
    let mut with_anchors: BTreeMap<String, Option<Vec<(String, String)>>> = BTreeMap::new();
    let mut run_anchors: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut lists = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim_end().ends_with("steps:") && line.trim() == "steps:" {
            let base = indent(line);
            let owner = lines[..at]
                .iter()
                .rev()
                .find(|candidate| indent(candidate) < base && candidate.trim_end().ends_with(':'))
                .map_or("", |candidate| candidate.trim().trim_end_matches(':'))
                .to_owned();
            let mut steps = Vec::new();
            at += 1;
            while at < lines.len() {
                let item = lines[at];
                if item.trim().is_empty() {
                    at += 1;
                    continue;
                }
                if indent(item) <= base {
                    break;
                }
                if sequence_head(item).is_none() {
                    at += 1;
                    continue;
                }
                let item_indent = indent(item);
                let sequence_width = match item_indent.saturating_sub(base) {
                    width @ (1 | 2) => width,
                    _ => 2,
                };
                let mut block = vec![item];
                at += 1;
                while at < lines.len()
                    && (lines[at].trim().is_empty() || indent(lines[at]) > item_indent)
                {
                    block.push(lines[at]);
                    at += 1;
                }
                steps.push(step_text(
                    &block,
                    &mut anchors,
                    &mut env_anchors,
                    &mut with_anchors,
                    &mut run_anchors,
                    &duplicate_anchors,
                    sequence_width,
                ));
            }
            lists.push((owner, steps));
            continue;
        }
        at += 1;
    }
    if malformed_tag || !duplicate_anchors.is_empty() {
        for (_, steps) in &mut lists {
            for step in steps {
                step.invalid_anchor = true;
                step.unexpected_nested = true;
            }
        }
    }
    lists
}

pub(super) fn duplicate_anchor_names(text: &str) -> BTreeSet<String> {
    let mut duplicate_anchors = find_duplicate_anchor_names(text);
    duplicate_anchors.extend(anchor_names::scan(text).duplicate_names);
    duplicate_anchors
}

/// One step block; `- &sN` records an anchor and `- *sN` resolves one.
fn step_text(
    block: &[&str],
    anchors: &mut BTreeMap<String, Option<StepText>>,
    env_anchors: &mut BTreeMap<String, Option<Vec<(String, String)>>>,
    with_anchors: &mut BTreeMap<String, Option<Vec<(String, String)>>>,
    run_anchors: &mut BTreeMap<String, Option<String>>,
    duplicate_anchors: &BTreeSet<String>,
    sequence_width: usize,
) -> StepText {
    let head = sequence_head(block[0]).unwrap_or_default().trim();
    if let Some(step) = resolve_step_alias(head, block, anchors, duplicate_anchors) {
        return step;
    }
    let layout = step_layout(block, sequence_width);
    let run = step_run_scalar(block, &layout, run_anchors, duplicate_anchors);
    let fields = step_fields(block, layout, &run, with_anchors, duplicate_anchors);
    let StepTopFields {
        top_fields,
        env_fields,
        unexpected_nested,
        invalid_anchor: top_fields_invalid_anchor,
        anchor_unsafe: anchor_tainted,
    } = step_top_fields(
        block,
        layout,
        env_anchors,
        with_anchors,
        &run,
        duplicate_anchors,
    );
    let name = step_name(block);
    let mut text = block.join("\n");
    if let Some(run_scalar) = &run.scalar {
        text.push('\n');
        text.push_str(run_scalar);
    }
    let step = StepText {
        name,
        text,
        fields,
        top_fields,
        env_fields,
        unexpected_nested,
        invalid_anchor: run.invalid_anchor || top_fields_invalid_anchor,
        run_scalar: run.scalar,
    };
    let step_anchor = step_anchor_name(head);
    if head.starts_with('&') && step_anchor.is_none() {
        return StepText {
            unexpected_nested: true,
            invalid_anchor: true,
            ..step
        };
    }
    if let Some(name) = step_anchor {
        if duplicate_anchors.contains(name) || anchors.contains_key(name) || anchor_tainted {
            anchors.insert(name.to_owned(), None);
            return StepText {
                unexpected_nested: true,
                invalid_anchor: true,
                ..step
            };
        }
        anchors.insert(name.to_owned(), Some(step.clone()));
    }
    step
}

fn resolve_step_alias(
    head: &str,
    block: &[&str],
    anchors: &BTreeMap<String, Option<StepText>>,
    duplicate_anchors: &BTreeSet<String>,
) -> Option<StepText> {
    if !head.starts_with('*') {
        return None;
    }
    let parsed_alias = anchor_token(head, '*').map_or("", |(alias, _)| alias);
    let alias = anchor_names::alias_name_after_asterisk(head).or_else(|| {
        anchor_token(head, '*').and_then(|(name, remainder)| {
            (!name.is_empty() && remainder.is_empty()).then_some(name)
        })
    });
    let mut step = if let Some(alias) = alias {
        anchors
            .get(alias)
            .and_then(Option::as_ref)
            .filter(|_| !duplicate_anchors.contains(alias))
            .map_or_else(|| missing_anchor(alias), clone_step)
    } else {
        missing_anchor(parsed_alias)
    };
    if block.iter().skip(1).any(|line| !line.trim().is_empty()) {
        step.unexpected_nested = true;
        step.invalid_anchor = true;
    }
    Some(step)
}

fn step_anchor_name(head: &str) -> Option<&str> {
    if !head.starts_with('&') {
        return None;
    }
    let after_marker = head.strip_prefix('&').unwrap_or_default();
    anchor_names::anchor_name_after_ampersand(after_marker)
        .or_else(|| anchor_names::anchor_value_after_ampersand(after_marker).map(|(name, _)| name))
        .or_else(|| {
            anchor_token(head, '&').and_then(|(name, _)| (!name.is_empty()).then_some(name))
        })
}

fn missing_anchor(alias: &str) -> StepText {
    StepText {
        name: format!("*{alias}"),
        text: String::new(),
        fields: Vec::new(),
        top_fields: Vec::new(),
        env_fields: Vec::new(),
        unexpected_nested: true,
        invalid_anchor: true,
        run_scalar: None,
    }
}

fn clone_step(step: &StepText) -> StepText {
    StepText {
        name: step.name.clone(),
        text: step.text.clone(),
        fields: step.fields.clone(),
        top_fields: step.top_fields.clone(),
        env_fields: step.env_fields.clone(),
        unexpected_nested: step.unexpected_nested,
        invalid_anchor: step.invalid_anchor,
        run_scalar: step.run_scalar.clone(),
    }
}

fn step_fields(
    block: &[&str],
    layout: StepLayout,
    run: &StepRun,
    with_anchors: &BTreeMap<String, Option<Vec<(String, String)>>>,
    duplicate_anchors: &BTreeSet<String>,
) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut mapping_value = false;
    for (index, line) in block.iter().enumerate() {
        if run.is_body_line(index) {
            continue;
        }
        let trimmed = step_field_content(index, line, block[0]).trim();
        let field = trimmed
            .split_once(": ")
            .or_else(|| trimmed.strip_suffix(':').map(|key| (key, "")));
        let Some((key, value)) = field else {
            continue;
        };
        let is_top_field =
            (index == 0 && layout.head_field) || indent(line) == layout.mapping_indent;
        if is_top_field {
            fields.push((key.to_owned(), value.to_owned()));
            mapping_value = matches!(key, "env" | "with")
                && (value.is_empty() || anchor_name(value, '&').is_some());
            if key == "with"
                && let Some(alias) = anchor_name(value, '*')
                && let Some(Some(aliased_fields)) = with_anchors
                    .get(alias)
                    .filter(|_| !duplicate_anchors.contains(alias))
            {
                fields.extend(aliased_fields.iter().cloned());
            }
        } else if mapping_value && indent(line) == layout.mapping_indent + layout.mapping_width {
            fields.push((key.to_owned(), value.to_owned()));
        }
    }
    fields
}

#[cfg(test)]
#[path = "rust_install_audit_parse_tests.rs"]
mod tests;
