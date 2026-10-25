use std::collections::{BTreeMap, BTreeSet};

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

pub(super) fn step_lists(text: &str) -> Vec<(String, Vec<StepText>)> {
    let lines: Vec<&str> = text.lines().collect();
    let duplicate_anchors = find_duplicate_anchor_names(text);
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
    lists
}

pub(super) fn duplicate_anchor_names(text: &str) -> BTreeSet<String> {
    find_duplicate_anchor_names(text)
}

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
    if let Some((alias, remainder)) = anchor_token(head, '*') {
        let valid_alias = !alias.is_empty() && remainder.is_empty();
        let mut step = if valid_alias {
            anchors
                .get(alias)
                .and_then(Option::as_ref)
                .filter(|_| !duplicate_anchors.contains(alias))
                .map_or_else(|| missing_anchor(alias), clone_step)
        } else {
            missing_anchor(alias)
        };
        if block.iter().skip(1).any(|line| !line.trim().is_empty()) {
            step.unexpected_nested = true;
            step.invalid_anchor = true;
        }
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
    if let Some((name, _remainder)) = anchor_token(head, '&') {
        if name.is_empty() {
            return StepText {
                unexpected_nested: true,
                invalid_anchor: true,
                ..step
            };
        }
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
    for (index, line) in block.iter().enumerate() {
        if run.is_body_line(index) {
            continue;
        }
        let trimmed = step_field_content(index, line, block[0]).trim();
        let Some((key, value)) = trimmed.split_once(": ") else {
            continue;
        };
        fields.push((key.to_owned(), value.to_owned()));
        let is_top_field =
            (index == 0 && layout.head_field) || indent(line) == layout.mapping_indent;
        if key == "with"
            && is_top_field
            && let Some(alias) = anchor_name(value, '*')
            && let Some(Some(aliased_fields)) = with_anchors
                .get(alias)
                .filter(|_| !duplicate_anchors.contains(alias))
        {
            fields.extend(aliased_fields.iter().cloned());
        }
    }
    fields
}

impl StepText {
    pub(super) fn uses_value(&self) -> Option<&str> {
        self.fields.iter().find_map(|(key, value)| {
            (key == "uses").then(|| {
                value
                    .strip_suffix(" # zizmor: ignore[self-repository]")
                    .unwrap_or(value)
            })
        })
    }
}
