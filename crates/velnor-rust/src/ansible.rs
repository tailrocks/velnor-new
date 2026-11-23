// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{JobId, RepoPath, RepoView, StepKind, VerdictJob, WorkflowPlan};

use crate::error::{DetectError, Reason};
use crate::plan::{CHECKOUT_DEPTH, job, step};
use crate::required_size::needs_max_bytes;

const LAYOUT: &str = "ansible-configs/";
const REQUIREMENTS_TXT: RepoPath = RepoPath::from_static("ansible-configs/requirements.txt");
const REQUIREMENTS_YAML: RepoPath = RepoPath::from_static("ansible-configs/requirements.yaml");
const INSTALL: &str = "python3 -m pip install --require-hashes --no-deps --target \"$RUNNER_TEMP/ansible\" -r ansible-configs/requirements.txt";
const LINT: &str = "PYTHONPATH=\"$RUNNER_TEMP/ansible\" python3 -m ansiblelint ansible-configs";
const SYNTAX_PREFIX: &str =
    "PYTHONPATH=\"$RUNNER_TEMP/ansible\" python3 -m ansible.cli.playbook --syntax-check";
const RUN_LIMIT: usize = 2000;

pub(crate) fn is_layout_file(path: &RepoPath) -> bool {
    let Some(name) = path.as_str().strip_prefix(LAYOUT) else {
        return false;
    };
    !name.contains('/')
        && (name == "requirements.txt"
            || name == "requirements.yaml"
            || name == "requirements.yml"
            || name.ends_with(".yml")
            || name.ends_with(".yaml"))
}

pub fn attach_ansible(
    view: &RepoView,
    facts: &velnor_model::RepoFacts,
    plan: &mut WorkflowPlan,
) -> Result<(), DetectError> {
    let Some(playbooks) = playbooks(view)? else {
        return Ok(());
    };
    let mut ansible = job(facts, JobId::from_static("ansible"));
    ansible.name = Some("Ansible".to_owned());
    ansible.steps.push(step(StepKind::Checkout {
        depth: CHECKOUT_DEPTH,
        sparse: None,
    }));
    ansible.steps.push(step(StepKind::Ansible {
        name: "Install Ansible".to_owned(),
        run: INSTALL.to_owned(),
    }));
    for run in syntax_runs(&playbooks) {
        ansible.steps.push(step(StepKind::Ansible {
            name: "Syntax-check playbooks".to_owned(),
            run,
        }));
    }
    ansible.steps.push(step(StepKind::Ansible {
        name: "ansible-lint".to_owned(),
        run: LINT.to_owned(),
    }));
    let addition = needs_max_bytes(facts, std::iter::once(&ansible)).saturating_sub(4);
    if let Some(required) = plan
        .jobs
        .iter_mut()
        .find(|candidate| candidate.id.as_str() == "required")
    {
        required.needs.push(ansible.id.clone());
        for step in &mut required.steps {
            if let StepKind::Verdict {
                spec,
                needs_max_bytes,
            } = &mut step.kind
            {
                spec.jobs.push(VerdictJob {
                    id: ansible.id.clone(),
                    when: Vec::new(),
                    tests: Vec::new(),
                });
                *needs_max_bytes = needs_max_bytes.saturating_add(addition);
            }
        }
    }
    match plan
        .jobs
        .iter()
        .position(|candidate| candidate.id.as_str() == "required")
    {
        Some(index) => plan.jobs.insert(index, ansible),
        None => plan.jobs.push(ansible),
    }
    Ok(())
}

fn playbooks(view: &RepoView) -> Result<Option<Vec<RepoPath>>, DetectError> {
    let mut playbooks = Vec::new();
    for path in &view.tracked_paths {
        let Some(name) = path.as_str().strip_prefix(LAYOUT) else {
            continue;
        };
        if name.contains('/') || name == "requirements.yaml" || name == "requirements.yml" {
            continue;
        }
        if !name.ends_with(".yml") && !name.ends_with(".yaml") {
            continue;
        }
        let Some(bytes) = view.files.get(path) else {
            continue;
        };
        if is_play(bytes) {
            playbooks.push(path.clone());
        }
    }
    playbooks.sort();
    if playbooks.is_empty() {
        return Ok(None);
    }
    let text = view
        .files
        .get(&REQUIREMENTS_TXT)
        .ok_or_else(|| refuse(&REQUIREMENTS_TXT, "requirements.txt is missing"))?;
    hashed(text)?;
    if !view.files.contains_key(&REQUIREMENTS_YAML) {
        return Err(refuse(&REQUIREMENTS_YAML, "requirements.yaml is missing"));
    }
    Ok(Some(playbooks))
}

fn is_play(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut play = false;
    let mut key_indent = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "---" || trimmed == "..." {
            key_indent = None;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 && (trimmed == "-" || trimmed.starts_with("- ")) {
            let inline = trimmed.strip_prefix("- ").unwrap_or("");
            play |= play_key(inline);
            key_indent = Some(2);
            continue;
        }
        if key_indent == Some(indent) && play_key(trimmed) {
            play = true;
        }
    }
    play
}

fn play_key(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("hosts:") || text.starts_with("import_playbook:")
}

fn hashed(bytes: &[u8]) -> Result<(), DetectError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        refuse(
            &REQUIREMENTS_TXT,
            "requirements.txt is not utf-8".to_owned(),
        )
    })?;
    let mut pending = String::new();
    let mut names = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(head) = trimmed.strip_suffix('\\') {
            pending.push_str(head.trim_end());
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        let joined = std::mem::take(&mut pending);
        let joined = joined.trim();
        if joined.is_empty() || joined.starts_with('#') {
            continue;
        }
        names.insert(requirement(joined).map_err(|reason| {
            refuse(&REQUIREMENTS_TXT, format!("line {}: {reason}", index + 1))
        })?);
    }
    if !pending.trim().is_empty() {
        return Err(refuse(
            &REQUIREMENTS_TXT,
            "requirements.txt ends with a continuation".to_owned(),
        ));
    }
    for name in ["ansible-core", "ansible-lint"] {
        if !names.contains(name) {
            return Err(refuse(
                &REQUIREMENTS_TXT,
                format!("requirements.txt does not pin {name}"),
            ));
        }
    }
    Ok(())
}

fn requirement(line: &str) -> Result<String, String> {
    let mut parts = line.split_whitespace();
    let spec = parts.next().ok_or("empty requirement")?;
    let Some((name, version)) = spec.split_once("==") else {
        return Err(format!("{spec} must be name==version"));
    };
    if name.is_empty()
        || version.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(format!("invalid requirement {spec}"));
    }
    let mut hashes = 0;
    for part in parts {
        let Some(hex) = part.strip_prefix("--hash=sha256:") else {
            return Err(format!("unsupported requirement option {part}"));
        };
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("a hash must be sha256 and 64 hex characters".to_owned());
        }
        hashes += 1;
    }
    if hashes == 0 {
        return Err(format!("{name} has no --hash=sha256"));
    }
    Ok(name.to_owned())
}

fn syntax_runs(playbooks: &[RepoPath]) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = SYNTAX_PREFIX.to_owned();
    for path in playbooks {
        let addition = format!(" {}", path.as_str());
        if current.len() + addition.len() > RUN_LIMIT && current != SYNTAX_PREFIX {
            runs.push(std::mem::replace(&mut current, SYNTAX_PREFIX.to_owned()));
        }
        current.push_str(&addition);
    }
    if current != SYNTAX_PREFIX {
        runs.push(current);
    }
    runs
}

fn refuse(path: &RepoPath, value: impl Into<String>) -> DetectError {
    DetectError::invalid(
        path,
        Reason::InvalidValue {
            key: "ansible-configs",
            value: value.into(),
        },
    )
}
