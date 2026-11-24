// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Gate, GateNeed, Job, JobCondition, JobId, PlanFrom, RepoFacts, RepoPath, RepoView, RootSlug,
    StepKind, VerdictJob, WorkflowPlan,
};

use crate::error::{DetectError, Reason};
use crate::lane::PLAN_JOB;
use crate::plan::{CHECKOUT_DEPTH, RUST_JOB, job, quick, step};
use crate::required_size::{charge_output, needs_max_bytes};

mod pins;
mod select;

pub use select::ansible_runs;

pub(super) const LAYOUT: &str = "ansible-configs/";
const REQUIREMENTS_TXT: RepoPath = RepoPath::from_static("ansible-configs/requirements.txt");
const REQUIREMENTS_YAML: RepoPath = RepoPath::from_static("ansible-configs/requirements.yaml");
const INSTALL: &str = "python3 -m pip install --only-binary=:all: --require-hashes --no-deps --target \"$RUNNER_TEMP/ansible\" -r ansible-configs/requirements.txt";
const REMOVE_TOOLS: &str =
    "rm -rf \"$RUNNER_TEMP/ansible/ansible\" \"$RUNNER_TEMP/ansible/ansiblelint\"";
const HOOKS: &str = "python3 -c '(lambda d: (d.mkdir(parents=True, exist_ok=True), (d/\"sitecustomize.py\").write_text(\"\"), (d/\"usercustomize.py\").write_text(\"\")))(__import__(\"pathlib\").Path(__import__(\"os\").environ[\"RUNNER_TEMP\"])/\"ansible-clean\")'";
const PREFIX: &str = "PYTHONPATH=\"$RUNNER_TEMP/ansible-clean:$RUNNER_TEMP/ansible\" PATH=\"$PATH:$RUNNER_TEMP/ansible/bin\"";
const RUN_LIMIT: usize = 2000;

pub const ANSIBLE_SLUG: RootSlug = RootSlug::from_static("ansible");
// helper-gate: first in tools-v0.1.113 via 3a4114ea3d
pub(crate) const ANSIBLE_SELECT_HELPER: (u64, u64, u64) = (0, 1, 113);

fn yaml_name(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"))
}

pub(crate) fn is_layout_file(path: &RepoPath) -> bool {
    let Some(name) = path.as_str().strip_prefix(LAYOUT) else {
        return false;
    };
    !name.contains('/')
        && (name == "requirements.txt"
            || name == "requirements.yaml"
            || name == "requirements.yml"
            || yaml_name(name))
}

pub fn attach_ansible(
    view: &RepoView,
    facts: &RepoFacts,
    plan: &mut WorkflowPlan,
) -> Result<(), DetectError> {
    let Some((playbooks, reinstall)) = playbooks(view)? else {
        return Ok(());
    };
    let mut ansible = ansible_job(facts, &playbooks, &reinstall);
    gate_ansible(facts, plan, &mut ansible)?;
    record_ansible(facts, plan, &ansible);
    place_ansible(plan, ansible);
    Ok(())
}

fn ansible_job(facts: &RepoFacts, playbooks: &[RepoPath], reinstall: &pins::Reinstall) -> Job {
    let mut ansible = job(facts, JobId::from_static("ansible"));
    ansible.pr = quick(facts);
    ansible.name = Some("Ansible".to_owned());
    ansible.steps.push(step(StepKind::Checkout {
        depth: CHECKOUT_DEPTH,
        sparse: None,
    }));
    ansible.steps.push(step(StepKind::Ansible {
        name: "Install Ansible".to_owned(),
        run: INSTALL.to_owned(),
    }));
    ansible.steps.push(step(StepKind::Ansible {
        name: "Write Ansible tool pins".to_owned(),
        run: reinstall.write.clone(),
    }));
    ansible.steps.push(step(StepKind::Ansible {
        name: "Remove planted Ansible modules".to_owned(),
        run: REMOVE_TOOLS.to_owned(),
    }));
    ansible.steps.push(step(StepKind::Ansible {
        name: "Install ansible-core and ansible-lint".to_owned(),
        run: reinstall.install.clone(),
    }));
    ansible.steps.push(step(StepKind::Ansible {
        name: "Install empty Python site hooks".to_owned(),
        run: HOOKS.to_owned(),
    }));
    ansible.steps.push(step(StepKind::Ansible {
        name: "Install collections".to_owned(),
        run: prefixed(
            "python3 -m ansible.cli.galaxy collection install -r ansible-configs/requirements.yaml",
        ),
    }));
    for run in syntax_runs(playbooks) {
        ansible.steps.push(step(StepKind::Ansible {
            name: "Syntax-check playbooks".to_owned(),
            run,
        }));
    }
    ansible.steps.push(step(StepKind::Ansible {
        name: "ansible-lint".to_owned(),
        run: prefixed("python3 -m ansiblelint ansible-configs"),
    }));
    ansible
}

fn prefixed(command: &str) -> String {
    format!("{PREFIX} {command}")
}

fn gate_ansible(
    facts: &RepoFacts,
    plan: &mut WorkflowPlan,
    ansible: &mut Job,
) -> Result<(), DetectError> {
    if !selects_ansible(facts) {
        return Ok(());
    }
    slug_collision(facts)?;
    let source = plan_source(&plan.jobs);
    let gate = Gate {
        slug: ANSIBLE_SLUG,
        need: GateNeed::Lint,
        from: if source == RUST_JOB {
            PlanFrom::WorkJob(source.clone())
        } else {
            PlanFrom::Job(source.clone())
        },
        packages: Vec::new(),
        unselected: false,
        group: None,
        also: Vec::new(),
    };
    ansible.needs = vec![source.clone()];
    let output = GateNeed::Lint.output_name(&ANSIBLE_SLUG);
    let added = plan
        .jobs
        .iter_mut()
        .find(|job| job.id == source)
        .is_some_and(|job| {
            let before = job.plan_outputs.len();
            job.plan_outputs.extend(gate.outputs());
            job.plan_outputs.sort();
            job.plan_outputs.dedup();
            job.plan_outputs.len() > before
        });
    if added {
        charge_output(&mut plan.jobs, &source, &output);
    }
    ansible.gate = Some(gate);
    Ok(())
}

fn plan_source(jobs: &[Job]) -> JobId {
    if jobs.iter().any(|job| job.id == RUST_JOB) && !jobs.iter().any(|job| job.id == PLAN_JOB) {
        RUST_JOB
    } else {
        PLAN_JOB
    }
}

fn selects_ansible(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(ANSIBLE_SELECT_HELPER))
}

pub(super) fn slug_collision(facts: &RepoFacts) -> Result<(), DetectError> {
    if facts
        .config
        .lint
        .iter()
        .any(|lint| lint.slug() == ANSIBLE_SLUG)
    {
        return Err(refuse(&REQUIREMENTS_YAML, "a lint slug is ansible"));
    }
    Ok(())
}

fn record_ansible(facts: &RepoFacts, plan: &mut WorkflowPlan, ansible: &Job) {
    let addition = needs_max_bytes(facts, std::iter::once(ansible)).saturating_sub(4);
    let Some(required) = plan
        .jobs
        .iter_mut()
        .find(|candidate| candidate.id.as_str() == "required")
    else {
        return;
    };
    required.needs.push(ansible.id.clone());
    let when: Vec<JobCondition> = ansible
        .gate
        .iter()
        .map(|gate| JobCondition::Lint(gate.slug.clone()))
        .collect();
    for step in &mut required.steps {
        if let StepKind::Verdict {
            spec,
            needs_max_bytes: size,
        } = &mut step.kind
        {
            spec.jobs.push(VerdictJob {
                id: ansible.id.clone(),
                when: when.clone(),
                tests: Vec::new(),
            });
            *size = size.saturating_add(addition);
        }
    }
}

fn place_ansible(plan: &mut WorkflowPlan, ansible: Job) {
    match plan
        .jobs
        .iter()
        .position(|candidate| candidate.id.as_str() == "required")
    {
        Some(index) => plan.jobs.insert(index, ansible),
        None => plan.jobs.push(ansible),
    }
}

fn playbooks(view: &RepoView) -> Result<Option<(Vec<RepoPath>, pins::Reinstall)>, DetectError> {
    let mut found = Vec::new();
    for path in &view.tracked_paths {
        let Some(name) = path.as_str().strip_prefix(LAYOUT) else {
            continue;
        };
        if name.contains('/') || name == "requirements.yaml" || name == "requirements.yml" {
            continue;
        }
        if !yaml_name(name) {
            continue;
        }
        let Some(bytes) = view.files.get(path) else {
            continue;
        };
        if is_play(bytes) {
            found.push(path.clone());
        }
    }
    found.sort();
    if found.is_empty() {
        return Ok(None);
    }
    let text = view
        .files
        .get(&REQUIREMENTS_TXT)
        .ok_or_else(|| refuse(&REQUIREMENTS_TXT, "requirements.txt is missing"))?;
    let reinstall = pins::reinstall(text, &REQUIREMENTS_TXT)?;
    if !view.files.contains_key(&REQUIREMENTS_YAML) {
        return Err(refuse(&REQUIREMENTS_YAML, "requirements.yaml is missing"));
    }
    Ok(Some((found, reinstall)))
}

fn is_play(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut play = false;
    let mut open = false;
    let mut key_indent = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "---" || trimmed == "..." {
            open = false;
            key_indent = None;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            open = trimmed == "-" || trimmed.starts_with("- ");
            let inline = open
                .then(|| trimmed.strip_prefix("- ").unwrap_or(""))
                .filter(|inline| play_key(inline));
            key_indent = inline.map(|_| 2);
            play |= key_indent.is_some();
            continue;
        }
        if !open {
            continue;
        }
        let learned = *key_indent.get_or_insert(indent);
        if indent == learned && play_key(trimmed) {
            play = true;
        }
    }
    play
}

fn play_key(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("hosts:") || text.starts_with("import_playbook:")
}

fn syntax_runs(playbooks: &[RepoPath]) -> Vec<String> {
    let prefix = format!("{PREFIX} python3 -m ansible.cli.playbook --syntax-check");
    let mut runs = Vec::new();
    let mut current = prefix.clone();
    for path in playbooks {
        let addition = format!(" {}", bash_single(path));
        if current.len() + addition.len() > RUN_LIMIT && current != prefix {
            runs.push(std::mem::replace(&mut current, prefix.clone()));
        }
        current.push_str(&addition);
    }
    if current != prefix {
        runs.push(current);
    }
    runs
}

fn bash_single(path: &RepoPath) -> String {
    format!("'{}'", path.as_str().replace('\'', "'\\''"))
}

pub(super) fn refuse(path: &RepoPath, value: impl Into<String>) -> DetectError {
    DetectError::invalid(
        path,
        Reason::InvalidValue {
            key: "ansible-configs",
            value: value.into(),
        },
    )
}
