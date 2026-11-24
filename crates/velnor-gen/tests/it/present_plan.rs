// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::{JobId, RepoView, VerdictSpec, WORKFLOW_PATH};
use velnor_rust::{DetectError, Need, Reason, decide};

use crate::facts::TestResult;

fn put_config(view: &mut RepoView, config: &str) -> TestResult {
    crate::stacks::put(view, ".velnor/rust-ci.toml", config.as_bytes())
}

fn jactionlint(view: &mut RepoView, config: &str, version: Option<&str>) -> TestResult {
    crate::stacks::put(view, "mise.toml", b"[tools]\njactionlint = \"2.4.0\"\n")?;
    put_config(view, config)?;
    crate::facts::lock_pinned_tools(view)?;
    if let Some(version) = version {
        crate::pipeline::pin_helper(view, version)?;
    }
    Ok(())
}

fn gate_spec(text: &str) -> Result<VerdictSpec, String> {
    let line = text
        .split("GATE_SPEC: '")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .ok_or("no gate spec")?;
    VerdictSpec::from_text(line)
}

fn jobs(text: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(text)?["jobs"].clone())
}

fn needs(spec: &VerdictSpec, lint_ansible: &str, ansible: &str) -> BTreeMap<JobId, Need> {
    let mut needs = BTreeMap::new();
    for job in &spec.jobs {
        let result = if job.id.as_str() == "ansible" {
            ansible
        } else {
            "success"
        };
        let mut outputs = BTreeMap::new();
        if job.id == spec.plan {
            outputs.insert("lint_ansible".to_owned(), lint_ansible.to_owned());
        }
        needs.insert(
            job.id.clone(),
            Need {
                result: Some(result.to_owned()),
                outputs,
            },
        );
    }
    needs
}

#[test]
fn a_planned_lint_without_cargo_needs_a_pin_and_an_explain_plan() -> TestResult {
    let config = "[[lint]]\nkind = 'jactionlint'\ninputs = ['mise.toml']\n";
    let mut bare = RepoView::default();
    jactionlint(&mut bare, config, None)?;
    match expected_with_notes(&bare) {
        Err(GenerateError::HelperNotPinned) => {}
        other => return Err(format!("{other:?}").into()),
    }
    let mut view = RepoView::default();
    jactionlint(&mut view, config, Some("0.1.111"))?;
    let files = expected_with_notes(&view)?.0;
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    let jobs = jobs(workflow)?;
    let plans: Vec<_> = crate::lint_lane::runs(&jobs["rust-plan"])
        .into_iter()
        .filter(|run| run.contains("plan --explain"))
        .collect();
    let [plan] = plans.as_slice() else {
        return Err(format!("one plan run: {plans:?}").into());
    };
    assert!(plan.contains(". \"$VELNOR_EXEC\" plan --explain"), "{plan}");
    assert_eq!(
        jobs["rust-plan"]["outputs"]["lint_jactionlint"].as_str(),
        Some("${{ steps.plan.outputs.lint_jactionlint }}"),
        "{}",
        jobs["rust-plan"]["outputs"]
    );
    assert_eq!(
        jobs["lint-jactionlint"]["needs"],
        serde_json::json!(["rust-plan"])
    );
    let spec = gate_spec(workflow)?;
    assert_eq!(spec.plan.as_str(), "rust-plan");
    let lint = spec
        .jobs
        .iter()
        .find(|job| job.id.as_str() == "lint-jactionlint")
        .ok_or("the gate spec lacks lint-jactionlint")?;
    assert_eq!(lint.when.len(), 1, "{lint:?}");
    assert_eq!(lint.when[0].to_string(), "lint:jactionlint");
    assert!(!workflow.contains("actions/cache"), "{workflow}");
    assert!(!files.contains_key(&velnor_model::CACHE_TRIM_WORKFLOW_PATH));
    assert!(jobs.get("rust-budget").is_none(), "{jobs}");
    Ok(())
}

#[test]
fn a_pinned_ansible_play_follows_lint_ansible() -> TestResult {
    let play = ansible_play()?;
    let mut older = play.clone();
    crate::pipeline::pin_helper(&mut older, "0.1.111")?;
    let older_files = expected_with_notes(&older)?.0;
    let older_workflow = older_files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    let older_jobs = jobs(older_workflow)?;
    assert!(older_jobs.get("rust-plan").is_some(), "{older_jobs}");
    assert!(older_jobs.get("required").is_some(), "{older_jobs}");
    let older_if = older_jobs["ansible"]["if"].as_str().unwrap_or_default();
    assert!(!older_if.contains("lint_ansible"), "{older_if}");
    let older_spec = gate_spec(older_workflow)?;
    assert_eq!(older_spec.plan.as_str(), "rust-plan");
    let always = older_spec
        .jobs
        .iter()
        .find(|job| job.id.as_str() == "ansible")
        .ok_or("the gate spec lacks ansible")?;
    assert!(always.when.is_empty(), "{always:?}");

    let mut released = play.clone();
    crate::pipeline::pin_helper(&mut released, "0.1.112")?;
    let released_workflow = expected_with_notes(&released)?
        .0
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?
        .clone();
    let released_if = jobs(&released_workflow)?["ansible"]["if"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(!released_if.contains("lint_ansible"), "{released_if}");

    let mut pinned = play;
    crate::pipeline::pin_helper(&mut pinned, "0.1.113")?;
    let workflow = expected_with_notes(&pinned)?
        .0
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?
        .clone();
    let jobs = jobs(&workflow)?;
    assert_eq!(jobs["ansible"]["needs"], serde_json::json!(["rust-plan"]));
    let condition = jobs["ansible"]["if"]
        .as_str()
        .ok_or("no ansible condition")?;
    assert!(
        condition.contains("needs.rust-plan.outputs.lint_ansible == 'run'"),
        "{condition}"
    );
    let spec = gate_spec(&workflow)?;
    assert_eq!(spec.plan.as_str(), "rust-plan");
    let ansible = spec
        .jobs
        .iter()
        .find(|job| job.id.as_str() == "ansible")
        .ok_or("the gate spec lacks ansible")?;
    assert_eq!(ansible.when.len(), 1, "{ansible:?}");
    assert_eq!(ansible.when[0].to_string(), "lint:ansible");
    let skipped = decide(
        &spec,
        &needs(&spec, "skip", "skipped"),
        Some("pull_request"),
    );
    assert!(skipped.passed, "{}", skipped.table());
    let ran = decide(&spec, &needs(&spec, "run", "skipped"), Some("pull_request"));
    assert!(!ran.passed, "{}", ran.table());
    let mut missing = needs(&spec, "skip", "skipped");
    missing.remove(&spec.plan);
    let absent = decide(&spec, &missing, Some("pull_request"));
    assert!(!absent.passed, "{}", absent.table());
    assert!(
        absent.table().contains("not in needs"),
        "{}",
        absent.table()
    );
    Ok(())
}

fn ansible_play() -> Result<RepoView, Box<dyn std::error::Error>> {
    crate::stacks::ansible_repository("---\n- hosts: all\n", "ansible-configs/site.yml")
}

#[test]
fn label_mode_runs_ansible_on_an_unlabeled_ready_pull_request() -> TestResult {
    let mut view = ansible_play()?;
    put_config(&mut view, "pr_ci = \"label\"\n")?;
    crate::pipeline::pin_helper(&mut view, "0.1.111")?;
    let files = expected_with_notes(&view)?.0;
    assert!(!files.contains_key(&velnor_model::CACHE_TRIM_WORKFLOW_PATH));
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    let tree = crate::yaml_tree::document(workflow)?;
    let jobs = &tree["jobs"];
    for id in ["ansible", "required", "rust-plan", "ci-run-label"] {
        assert!(jobs.get(id).is_some(), "{id} missing");
    }
    assert!(jobs.get("cache").is_none(), "{jobs}");
    assert!(jobs.get("rust-budget").is_none(), "{jobs}");
    assert_eq!(
        crate::pr_ci_sim::strings(jobs["ci-run-label"].get("needs")),
        ["required".to_owned()]
    );
    let plans = crate::lint_lane::runs(&jobs["rust-plan"]);
    assert!(
        plans.iter().any(|run| run.contains("merge-queue-check")),
        "{plans:?}"
    );
    let (ready, _) = crate::pr_ci_sim::outcome(&tree, &crate::pr_ci_sim::pull(false, &[]), &[])?;
    assert_eq!(crate::pr_ci_sim::state(&ready, "ansible"), "success");
    let (draft, _) = crate::pr_ci_sim::outcome(&tree, &crate::pr_ci_sim::pull(true, &[]), &[])?;
    assert_eq!(crate::pr_ci_sim::state(&draft, "ansible"), "skipped");
    let (bug, _) =
        crate::pr_ci_sim::outcome(&tree, &crate::pr_ci_sim::labeled("bug", &["bug"]), &[])?;
    assert_eq!(crate::pr_ci_sim::state(&bug, "ansible"), "skipped");
    Ok(())
}

#[test]
fn gating_ansible_grows_the_cache_bound() -> TestResult {
    let mut view = crate::pipeline::view(&["Cargo.toml"], None)?;
    crate::pipeline::pin_helper(&mut view, "0.1.113")?;
    let core = "0123456789abcdef".repeat(4);
    let lint = "fedcba9876543210".repeat(4);
    let requirements = format!(
        "ansible-core==2.16.14 \\\n    --hash=sha256:{core}\nansible-lint==24.2.0 \\\n    --hash=sha256:{lint}\n"
    );
    crate::stacks::put(
        &mut view,
        "ansible-configs/requirements.txt",
        requirements.as_bytes(),
    )?;
    crate::stacks::put(
        &mut view,
        "ansible-configs/requirements.yaml",
        b"collections: []\n",
    )?;
    crate::stacks::put(
        &mut view,
        "ansible-configs/site.yml",
        b"---\n- hosts: all\n",
    )?;
    let facts = velnor_rust::detect(&view)?;
    let mut planned = velnor_rust::plan(&facts);
    let before = cache_bound(&planned)?;
    velnor_rust::attach_ansible(&view, &facts, &mut planned)?;
    let after = cache_bound(&planned)?;
    assert!(after > before, "before={before} after={after}");
    Ok(())
}

fn cache_bound(plan: &velnor_model::WorkflowPlan) -> Result<usize, Box<dyn std::error::Error>> {
    plan.jobs
        .iter()
        .flat_map(|job| &job.steps)
        .find_map(|step| match &step.kind {
            velnor_model::StepKind::CheckMetrics {
                needs_max_bytes, ..
            } => Some(*needs_max_bytes),
            _ => None,
        })
        .ok_or_else(|| "no cache bound".into())
}

#[test]
fn a_lint_slug_ansible_beside_a_play_is_refused() -> TestResult {
    let config = "[[lint]]\nkind = 'jactionlint'\nid = 'ansible'\ninputs = ['mise.toml']\n";
    let mut played =
        crate::stacks::ansible_repository("---\n- hosts: all\n", "ansible-configs/site.yml")?;
    jactionlint(&mut played, config, Some("0.1.113"))?;
    match expected_with_notes(&played) {
        Err(GenerateError::Detect(DetectError::Invalid {
            path,
            reason: Reason::InvalidValue { key, value },
        })) => {
            assert_eq!(path.as_str(), "ansible-configs/requirements.yaml");
            assert_eq!(key, "ansible-configs");
            assert_eq!(value, "a lint slug is ansible");
        }
        other => return Err(format!("{other:?}").into()),
    }
    let mut plain = RepoView::default();
    jactionlint(&mut plain, config, Some("0.1.113"))?;
    let workflow = expected_with_notes(&plain)?.0;
    let text = workflow
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    assert!(text.lines().any(|line| line == "  lint-ansible:"), "{text}");
    Ok(())
}
