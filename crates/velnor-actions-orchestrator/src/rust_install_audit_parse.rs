//! Small ordered reader for emitted workflow step mappings.

use std::collections::{BTreeMap, BTreeSet};

#[path = "rust_install_audit_parse_anchor_names.rs"]
mod anchor_names;
#[path = "rust_install_audit_parse_fields.rs"]
mod fields;
use fields::{ParsedStepFields, mapping_layout};

#[derive(Clone)]
pub(super) struct StepText {
    pub(super) name: String,
    pub(super) text: String,
    pub(super) fields: Vec<(String, String)>,
    pub(super) env_fields: Vec<(String, String)>,
    pub(super) run_scalar: Option<String>,
    pub(super) unexpected_nested: bool,
    pub(super) invalid_anchor: bool,
}

impl StepText {
    pub(super) fn field(&self, wanted: &str) -> Option<&str> {
        self.fields
            .iter()
            .find_map(|(key, value)| (key == wanted).then_some(value.as_str()))
    }

    pub(super) fn uses_value(&self) -> Option<&str> {
        let raw = self.field("uses")?;
        let value = raw.split_once(" #").map_or(raw, |(value, _)| value);
        let value = value.trim();
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
    let anchor_scan = anchor_names::scan(text);
    let malformed_tag = anchor_scan.malformed_tag;
    let duplicate_anchors = anchor_scan.duplicate_names;
    let mut anchors: BTreeMap<String, StepText> = BTreeMap::new();
    let mut map_anchors: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut scalar_anchors: BTreeMap<String, String> = BTreeMap::new();
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
                    &mut map_anchors,
                    &mut scalar_anchors,
                    &duplicate_anchors,
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

fn step_text(
    block: &[&str],
    anchors: &mut BTreeMap<String, StepText>,
    map_anchors: &mut BTreeMap<String, Vec<(String, String)>>,
    scalar_anchors: &mut BTreeMap<String, String>,
    duplicate_anchors: &BTreeSet<String>,
) -> StepText {
    let head = sequence_head(block[0]).unwrap_or_default().trim();
    if head.starts_with('*') {
        let alias = anchor_names::alias_name_after_asterisk(head);
        let resolved = alias
            .filter(|alias| !duplicate_anchors.contains(*alias))
            .and_then(|alias| anchors.get(alias).cloned());
        let Some(mut step) = resolved else {
            let alias_name = alias.unwrap_or("");
            return StepText {
                name: format!("*{alias_name}"),
                text: block.join("\n"),
                fields: Vec::new(),
                env_fields: Vec::new(),
                run_scalar: None,
                unexpected_nested: true,
                invalid_anchor: true,
            };
        };
        if block.iter().skip(1).any(|line| !line.trim().is_empty()) {
            step.unexpected_nested = true;
        }
        return step;
    }

    let base = indent(block[0]);
    let layout = mapping_layout(block, head, base);
    let mut parsed = ParsedStepFields::new(layout.malformed_indent);
    for (index, line) in block.iter().enumerate() {
        parsed.read_line(index, line, head, layout, scalar_anchors, duplicate_anchors);
    }
    parsed.finish(map_anchors, scalar_anchors, duplicate_anchors);

    let name = parsed
        .fields
        .iter()
        .find_map(|(key, value)| (key == "name").then_some(value.as_str()))
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_owned();
    let step_anchor = head
        .strip_prefix('&')
        .and_then(anchor_names::anchor_value_after_ampersand)
        .map(|(anchor, _)| anchor);
    let invalid_anchor = (head.starts_with('&') && step_anchor.is_none())
        || step_anchor.is_some_and(|anchor| duplicate_anchors.contains(anchor));
    let step = StepText {
        name,
        text: block.join("\n"),
        fields: parsed.fields,
        env_fields: parsed.env_fields,
        run_scalar: parsed.run_scalar,
        unexpected_nested: parsed.unexpected_nested || invalid_anchor || parsed.invalid_anchor,
        invalid_anchor: invalid_anchor || parsed.invalid_anchor,
    };
    if let Some(anchor) = step_anchor.filter(|anchor| !duplicate_anchors.contains(*anchor)) {
        anchors.insert(anchor.to_owned(), step.clone());
    }
    step
}

fn sequence_head(line: &str) -> Option<&str> {
    let item = line.trim_start();
    if item == "-" {
        Some("")
    } else {
        item.strip_prefix("- ")
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

#[cfg(test)]
#[path = "rust_install_audit_parse_tests.rs"]
mod tests;
