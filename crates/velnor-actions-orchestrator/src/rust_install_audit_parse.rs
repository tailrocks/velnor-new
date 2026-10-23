use std::collections::BTreeMap;

/// One step: its name and full text.
#[derive(Clone)]
pub(super) struct StepText {
    pub(super) name: String,
    pub(super) text: String,
    pub(super) fields: Vec<(String, String)>,
    pub(super) top_fields: Vec<(String, String)>,
    pub(super) env_fields: Vec<(String, String)>,
    pub(super) unexpected_nested: bool,
    pub(super) run_scalar: Option<String>,
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Every `steps:` sequence of a rendered file with its owning job (or
/// `runs` for a composite action), as step texts in order.
pub(super) fn step_lists(text: &str) -> Vec<(String, Vec<StepText>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut anchors: BTreeMap<String, StepText> = BTreeMap::new();
    let mut run_anchors: BTreeMap<String, String> = BTreeMap::new();
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
                if !item.trim_start().starts_with("- ") {
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
                steps.push(step_text(&block, &mut anchors, &mut run_anchors));
            }
            lists.push((owner, steps));
            continue;
        }
        at += 1;
    }
    lists
}

/// One step block; `- &sN` records an anchor and `- *sN` resolves one.
fn step_text(
    block: &[&str],
    anchors: &mut BTreeMap<String, StepText>,
    run_anchors: &mut BTreeMap<String, String>,
) -> StepText {
    let head = block[0].trim_start().trim_start_matches("- ").trim();
    if let Some(alias) = head.strip_prefix('*') {
        return anchors
            .get(alias)
            .map_or_else(|| missing_anchor(alias), clone_step);
    }
    let fields = step_fields(block);
    let StepTopFields {
        top_fields,
        env_fields,
        unexpected_nested,
    } = step_top_fields(block);
    let name = step_name(block);
    let run_scalar = step_run_scalar(block, run_anchors);
    let mut text = block.join("\n");
    if let Some(run_scalar) = &run_scalar {
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
        run_scalar,
    };
    if let Some(anchor) = head.strip_prefix('&') {
        anchors.insert(
            anchor
                .split_whitespace()
                .next()
                .unwrap_or(anchor)
                .to_owned(),
            step.clone(),
        );
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
        run_scalar: step.run_scalar.clone(),
    }
}

fn step_fields(block: &[&str]) -> Vec<(String, String)> {
    block
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim_start().trim_start_matches("- ").trim();
            let (key, value) = trimmed.split_once(": ")?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

struct StepTopFields {
    top_fields: Vec<(String, String)>,
    env_fields: Vec<(String, String)>,
    unexpected_nested: bool,
}

fn step_top_fields(block: &[&str]) -> StepTopFields {
    let item_indent = indent(block[0]);
    let mut top_fields = Vec::new();
    let mut env_fields = Vec::new();
    let mut in_env = false;
    let mut unexpected_nested = false;
    for (index, line) in block.iter().enumerate() {
        let trimmed = line.trim_start().trim_start_matches("- ").trim();
        let field = trimmed
            .split_once(": ")
            .or_else(|| trimmed.strip_suffix(':').map(|key| (key, "")));
        let Some((key, value)) = field else {
            if index > 0 && !line.trim().is_empty() {
                unexpected_nested = true;
            }
            continue;
        };
        let line_indent = indent(line);
        if index == 0 || line_indent == item_indent + 2 {
            top_fields.push((key.to_owned(), value.to_owned()));
            in_env = key == "env" && value.is_empty();
        } else if in_env && line_indent == item_indent + 4 {
            env_fields.push((key.to_owned(), value.to_owned()));
        } else {
            unexpected_nested = true;
        }
    }
    StepTopFields {
        top_fields,
        env_fields,
        unexpected_nested,
    }
}

fn step_name(block: &[&str]) -> String {
    block
        .iter()
        .find_map(|line| {
            let trimmed = line.trim_start().trim_start_matches("- ");
            trimmed.strip_prefix("name: ")
        })
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_owned()
}

fn step_run_scalar(block: &[&str], run_anchors: &mut BTreeMap<String, String>) -> Option<String> {
    block.iter().find_map(|line| {
        let trimmed = line.trim_start().trim_start_matches("- ").trim();
        let raw = trimmed.strip_prefix("run: ")?;
        if let Some(anchor) = raw.strip_prefix('&') {
            let (name, value) = anchor.split_once(' ')?;
            run_anchors.insert(name.to_owned(), value.to_owned());
            Some(value.to_owned())
        } else if let Some(alias) = raw.strip_prefix('*') {
            run_anchors.get(alias).cloned()
        } else {
            Some(raw.to_owned())
        }
    })
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
