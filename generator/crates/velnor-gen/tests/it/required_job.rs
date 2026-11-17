// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_files;
use velnor_model::RepoPath;
use velnor_render::MAX_RUN_CHARACTERS;

use crate::facts::TestResult;
use crate::golden::{load, samples};
use crate::pipeline::{HELPER_LOCK, view, workflow};

fn released(version: &str, roots: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = view(roots, None)?;
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.79", version, 1);
    view.files
        .insert(RepoPath::new(".velnor/rust-ci.lock")?, lock.into_bytes());
    workflow(&view)
}

fn required(text: &str) -> Result<&str, &'static str> {
    text.find("  required:\n")
        .map(|at| &text[at..])
        .ok_or("no required job")
}

fn gate_line(text: &str) -> Result<&str, &'static str> {
    required(text)?
        .lines()
        .find_map(|line| line.trim().strip_prefix("GATE_SPEC: '"))
        .map(|line| line.trim_end_matches('\''))
        .ok_or("no gate spec")
}

fn words(line: &str, flag: &str) -> Vec<String> {
    let all: Vec<&str> = line.split(' ').collect();
    all.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].to_owned())
        .collect()
}

#[test]
fn a_helper_that_has_gate_renders_the_gate_in_two_steps_without_a_checkout() -> TestResult {
    let text = released("0.1.81", &["Cargo.toml", "lab/Cargo.toml"])?;
    let job = required(&text)?;
    assert!(job.contains(
        "    if: '${{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (always()) }}'\n"
    ));
    assert!(!job.contains("Check out"));
    assert!(!job.contains("Fail when a Rust job failed or was cancelled"));
    assert_eq!(job.matches("      - name: ").count(), 2);
    assert!(job.contains("      - name: Install velnor-ci 0.1.81\n"));
    assert!(job.contains(
        "        env:\n          NEEDS_JSON: '${{ toJSON(needs) }}'\n          GATE_SPEC: '"
    ));
    assert!(job.contains("        run: velnor-ci gate --spec-env GATE_SPEC\n"));
    assert_eq!(
        gate_line(&text)?,
        "--plan rust-plan --job rust-plan=always --job rust-root=lane:root --job rust-lab=lane:lab --job rust-inputs-audit=schedule-or-dispatch --tests rust-root=root --tests rust-lab=lab"
    );
    Ok(())
}

#[test]
fn a_helper_without_gate_keeps_the_old_verdict_step() -> TestResult {
    let text = released("0.1.80", &["Cargo.toml", "lab/Cargo.toml"])?;
    let job = required(&text)?;
    assert!(job.contains(
        "    if: '${{ (github.event_name != ''pull_request'' || (startsWith(github.ref, ''refs/pull/'') && github.event.pull_request.state != ''closed'')) && (!cancelled()) }}'\n"
    ));
    assert!(job.contains("Fail when a Rust job failed or was cancelled"));
    assert!(!job.contains("velnor-ci gate"));
    assert!(!text.contains("_tests"));
    Ok(())
}

#[test]
fn the_plan_job_exports_the_tests_flag_of_every_lane_that_counts_tests() -> TestResult {
    let text = released("0.1.81", &["Cargo.toml", "lab/Cargo.toml"])?;
    for slug in ["root", "lab"] {
        assert!(text.contains(&format!(
            "      rust_{slug}_tests: '${{{{ steps.plan.outputs.rust_{slug}_tests }}}}'\n"
        )));
    }
    Ok(())
}

#[test]
fn one_job_exports_its_own_plan_flag_and_the_gate_reads_it_from_that_job() -> TestResult {
    let text = released("0.1.81", &["Cargo.toml"])?;
    let line = gate_line(&text)?;
    assert_eq!(line, "--plan rust --job rust=always --tests rust=root");
    assert!(text.contains("      rust_root_tests: '${{ steps.plan.outputs.rust_root_tests }}'\n"));
    assert!(text.contains("      tests_run: '${{ steps.test.outputs.tests_run }}'\n"));
    assert!(text.contains("      tests_listed: '${{ steps.test.outputs.tests_listed }}'\n"));
    Ok(())
}

#[test]
fn the_gate_names_every_job_the_verdict_job_needs() -> TestResult {
    let text = released(
        "0.1.81",
        &["Cargo.toml", "lab/Cargo.toml", "tools/Cargo.toml"],
    )?;
    let job = required(&text)?;
    let needs: Vec<&str> = job
        .lines()
        .skip_while(|line| *line != "    needs:")
        .skip(1)
        .take_while(|line| line.starts_with("      - "))
        .map(|line| line.trim_start_matches("      - "))
        .collect();
    let line = gate_line(&text)?;
    let named: Vec<String> = words(line, "--job")
        .iter()
        .filter_map(|job| job.split_once('=').map(|(id, _)| id.to_owned()))
        .collect();
    assert_eq!(needs, named);
    Ok(())
}

fn built_from_source(version: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = load(&samples().join("helper-source"))?;
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let bytes = view.files.get(&lock).ok_or("no lock")?;
    let text = String::from_utf8(bytes.clone())?.replacen("0.1.79", version, 1);
    view.files.insert(lock, text.into_bytes());
    let mut files = expected_files(&view)?;
    files
        .remove(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or_else(|| "no workflow".into())
}

#[test]
fn a_helper_built_from_source_never_runs_the_gate_from_the_pull_request_build() -> TestResult {
    let text = built_from_source("0.1.81")?;
    let job = required(&text)?;
    assert!(job.contains("      - name: Install velnor-ci 0.1.81\n"));
    assert!(job.contains("      - name: velnor-ci gate\n"));
    assert!(!job.contains("Download velnor-ci"));
    assert!(!job.contains("download-artifact"));
    assert!(!job.contains("Make velnor-ci executable"));
    assert!(text.contains(
        "      rust_generator_tests: '${{ steps.plan.outputs.rust_generator_tests }}'\n"
    ));
    Ok(())
}

#[test]
fn a_helper_built_from_source_keeps_the_old_verdict_while_the_plan_helper_lacks_gate() -> TestResult
{
    let text = built_from_source("0.1.80")?;
    let job = required(&text)?;
    assert!(job.contains("Fail when a Rust job failed or was cancelled"));
    assert!(!job.contains("velnor-ci gate"));
    assert!(!text.contains("_tests"));
    Ok(())
}

#[test]
fn sixty_roots_keep_the_gate_command_short_and_the_spec_in_env() -> TestResult {
    let manifests: Vec<String> = (0..60)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("crate-with-a-long-directory-name-{index}/Cargo.toml")
            }
        })
        .collect();
    let roots: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let text = released("0.1.81", &roots)?;
    let spec = gate_line(&text)?;
    assert!(spec.len() > MAX_RUN_CHARACTERS);
    assert_eq!(spec.matches("--tests ").count(), 60);
    assert!(required(&text)?.contains("        run: velnor-ci gate --spec-env GATE_SPEC\n"));
    Ok(())
}

fn many_members(count: usize) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut view = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.79", "0.1.81", 1);
    view.files
        .insert(RepoPath::new(".velnor/rust-ci.lock")?, lock.into_bytes());
    let metadata = view
        .rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no metadata")?;
    let member = metadata.members.first().ok_or("no member")?.clone();
    metadata.members = (0..count)
        .map(|index| {
            let mut member = member.clone();
            member.name = velnor_model::PackageName::new(format!(
                "package-with-a-long-but-valid-cargo-name-{index}"
            ))?;
            Ok(member)
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    Ok(view)
}

#[test]
fn a_large_plan_cannot_overflow_the_required_jobs_needs_environment() -> TestResult {
    let small = expected_files(&many_members(10)?)?;
    assert!(small.values().any(|text| text.contains("velnor-ci gate")));
    let large = expected_files(&many_members(2_500)?);
    assert!(
        matches!(&large, Err(velnor_gen::GenerateError::Rules(violations))
            if violations.iter().any(|violation|
                violation.job == "required"
                    && matches!(violation.rule, velnor_render::Rule::EnvTooLong(_)))),
        "generation must reject the expanded needs context before a runner starts"
    );
    Ok(())
}

#[test]
fn an_env_value_over_the_limit_is_a_rule_violation() {
    use velnor_render::{
        MAX_ENV_CHARACTERS, RenderedJob, RenderedStep, RenderedWorkflow, Rule, Value, check_rules,
    };
    let step = |value| RenderedStep {
        name: "step".to_owned(),
        run: Some("echo hi".to_owned()),
        env: vec![("SPEC".to_owned(), value)],
        ..RenderedStep::default()
    };
    let violations = |value| {
        check_rules(&RenderedWorkflow {
            jobs: vec![RenderedJob {
                id: "required".to_owned(),
                runs_on: "ubuntu-latest".to_owned(),
                timeout_minutes: 10,
                steps: vec![step(value)],
                ..RenderedJob::default()
            }],
        })
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|violation| violation.rule)
        .collect::<Vec<Rule>>()
    };
    assert_eq!(
        violations(Value::Text("a".repeat(MAX_ENV_CHARACTERS))),
        Vec::<Rule>::new()
    );
    assert_eq!(
        violations(Value::Text("a".repeat(MAX_ENV_CHARACTERS + 1))),
        vec![Rule::EnvTooLong(MAX_ENV_CHARACTERS + 1)]
    );
    for bytes in [MAX_ENV_CHARACTERS, MAX_ENV_CHARACTERS + 1] {
        assert_eq!(
            violations(Value::BoundedExpr {
                text: "${{ toJSON(needs) }}".to_owned(),
                max_bytes: bytes,
            }),
            if bytes == MAX_ENV_CHARACTERS {
                vec![]
            } else {
                vec![Rule::EnvTooLong(bytes)]
            }
        );
    }
    assert_eq!(
        violations(Value::Text("é".repeat(MAX_ENV_CHARACTERS))),
        vec![Rule::EnvTooLong(2 * MAX_ENV_CHARACTERS)]
    );
}
