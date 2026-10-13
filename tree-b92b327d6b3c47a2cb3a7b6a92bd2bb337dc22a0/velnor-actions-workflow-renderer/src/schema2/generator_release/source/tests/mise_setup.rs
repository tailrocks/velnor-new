use crate::schema2::GeneratorReleasePins;
use crate::schema2::Schema2WorkflowRequest;
use std::collections::BTreeSet;
use std::error::Error;
use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_workflow_steps::setup::MiseSetup;
use velnor_actions_workflow_tree::yaml::Yaml;

#[test]
fn each_archive_consumer_has_target_pinned_mise_before_extraction() -> Result<(), Box<dyn Error>> {
    let mut pins = test_pins();
    pins.linux_x86_64_setup = mise_setup('a', "2026.9.18", 'b');
    pins.macos_arm64_setup = mise_setup('c', "2026.9.19", 'd');
    pins.macos_x86_64_setup = mise_setup('e', "2026.9.20", 'f');
    let request = Schema2WorkflowRequest {
        version: "0.1.1".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        generator_release: Some(pins.clone()),
    };
    let release = super::super::super::generator_release(&request)?;
    let jobs = map_entries(map_field(map_entries(&release.workflow)?, "jobs")?)?;
    assert_candidate_manifest_setup(jobs, pins.setup_for(ReleaseTarget::LinuxX86_64))?;

    for (job_id, action, target) in [
        (
            "qualify-linux",
            "./.github/actions/generator-release-qualify-linux",
            ReleaseTarget::LinuxX86_64,
        ),
        (
            "qualify-macos",
            "./.github/actions/generator-release-qualify-macos",
            ReleaseTarget::MacosArm64,
        ),
        (
            "qualify-macos-intel",
            "./.github/actions/generator-release-qualify-macos-intel",
            ReleaseTarget::MacosX86_64,
        ),
    ] {
        assert_qualifier_setup(&release, jobs, job_id, action, pins.setup_for(target))?;
    }
    Ok(())
}

fn assert_candidate_manifest_setup(
    jobs: &[(String, Yaml)],
    setup: &MiseSetup,
) -> Result<(), Box<dyn Error>> {
    let candidate = map_field(jobs, "candidate-manifest")?;
    let steps = sequence(map_field(map_entries(candidate)?, "steps")?)?;
    assert!(steps.len() > 2);
    assert_setup_step(&steps[1], setup, "candidate-manifest")?;
    assert_guard_runs_after_step(steps, 1, "candidate-manifest");
    Ok(())
}

fn assert_qualifier_setup(
    release: &super::super::super::GeneratorRelease,
    jobs: &[(String, Yaml)],
    job_id: &str,
    action: &str,
    setup: &MiseSetup,
) -> Result<(), Box<dyn Error>> {
    let job = map_field(jobs, job_id)?;
    let steps = sequence(map_field(map_entries(job)?, "steps")?)?;
    assert_eq!(steps.len(), 2, "{job_id} must fetch source, then qualify");
    assert_source_step(&steps[0], job_id)?;
    assert_qualifier_step(&steps[1], steps, action, job_id)?;
    assert_action_extracts_guard(&release.actions, action, setup, job_id)?;
    assert_eq!(
        map_entries(map_field(map_entries(job)?, "permissions",)?)?.len(),
        0,
        "{job_id} must retain empty job permissions"
    );
    Ok(())
}

fn assert_setup_step(step: &Yaml, setup: &MiseSetup, job_id: &str) -> Result<(), Box<dyn Error>> {
    let fields = map_entries(step)?;
    assert_eq!(
        scalar(map_field(fields, "name")?)?,
        "Setup Mise",
        "{job_id}"
    );
    assert_eq!(scalar(map_field(fields, "uses")?)?, setup.uses, "{job_id}");
    let inputs = map_entries(map_field(fields, "with")?)?;
    for (field, expected) in [
        ("version", setup.version.as_str()),
        ("sha256", setup.sha256.as_str()),
        ("install", "false"),
        ("env", "false"),
        ("cache", "false"),
        ("cache_save", "false"),
    ] {
        assert_eq!(
            scalar(map_field(inputs, field)?)?,
            expected,
            "{job_id} Setup Mise input {field}"
        );
    }
    Ok(())
}

fn assert_guard_runs_after_step(steps: &[Yaml], setup_index: usize, job_id: &str) {
    let guard_indices = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            let fields = map_entries(step).ok()?;
            let run = map_field(fields, "run").ok()?;
            scalar(run)
                .ok()?
                .contains("with-owned-archive-guard.sh")
                .then_some(index)
        })
        .collect::<Vec<_>>();
    assert!(
        !guard_indices.is_empty(),
        "{job_id} has no guarded archive consumer"
    );
    assert!(
        guard_indices.iter().all(|index| *index > setup_index),
        "{job_id} must provision pinned Mise before the archive guard"
    );
}

fn assert_action_extracts_guard(
    actions: &[(String, Yaml)],
    action: &str,
    setup: &MiseSetup,
    job_id: &str,
) -> Result<(), Box<dyn Error>> {
    let path = format!("{}/action.yml", action.trim_start_matches("./"));
    let (_, document) = actions
        .iter()
        .find(|(candidate, _)| candidate == &path)
        .ok_or_else(|| format!("missing generated action {path}"))?;
    let runs = map_entries(map_field(map_entries(document)?, "runs")?)?;
    let steps = sequence(map_field(runs, "steps")?)?;
    assert_setup_step(
        steps.first().ok_or("qualifier action has no steps")?,
        setup,
        job_id,
    )?;
    assert_guard_runs_after_step(steps, 0, job_id);
    assert!(
        steps.iter().any(|step| {
            map_entries(step)
                .ok()
                .and_then(|fields| map_field(fields, "run").ok())
                .and_then(|run| scalar(run).ok())
                .is_some_and(|run| run.contains("with-owned-archive-guard.sh"))
        }),
        "{job_id} action must use the owned archive guard"
    );
    Ok(())
}

fn mise_setup(action: char, version: &str, digest: char) -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{}", action.to_string().repeat(40)),
        version: version.to_owned(),
        sha256: digest.to_string().repeat(64),
    }
}
fn test_pins() -> GeneratorReleasePins {
    let setup = MiseSetup {
        uses: format!("jdx/mise-action@{}", "a".repeat(40)),
        version: "2026.9.18".to_owned(),
        sha256: "b".repeat(64),
    };
    GeneratorReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_build_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "gh@2.102.0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        build_argv: vec!["mise".to_owned(), "exec".to_owned()],
        actionlint_argv: vec!["mise".to_owned(), "exec".to_owned()],
        zizmor_argv: vec!["mise".to_owned(), "exec".to_owned()],
        gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "gh@2.102.0",
            "--",
            "gh",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rust_version: "1.98.1".to_owned(),
        mr_boxington_version: "1.21.1".to_owned(),
    }
}

fn map_entries(value: &Yaml) -> Result<&[(String, Yaml)], Box<dyn Error>> {
    match value {
        Yaml::Map(entries) => Ok(entries),
        _ => Err("expected YAML mapping".into()),
    }
}

fn map_field<'a>(entries: &'a [(String, Yaml)], key: &str) -> Result<&'a Yaml, Box<dyn Error>> {
    entries
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| format!("missing YAML mapping field {key}").into())
}

fn has_field(entries: &[(String, Yaml)], key: &str) -> bool {
    entries.iter().any(|(name, _)| name == key)
}

fn sequence(value: &Yaml) -> Result<&[Yaml], Box<dyn Error>> {
    match value {
        Yaml::Seq(items) => Ok(items),
        _ => Err("expected YAML sequence".into()),
    }
}

fn scalar(value: &Yaml) -> Result<&str, Box<dyn Error>> {
    match value {
        Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. } => Ok(value),
        _ => Err(format!("expected YAML scalar string, got {value:?}").into()),
    }
}

fn assert_source_step(step: &Yaml, job_id: &str) -> Result<(), Box<dyn Error>> {
    let source = map_entries(step)?;
    assert_eq!(scalar(map_field(source, "shell")?)?, "bash", "{job_id}");
    let source_run = scalar(map_field(source, "run")?)?;
    assert!(
        !has_field(source, "uses"),
        "{job_id} must acquire source with an inline executable step"
    );
    assert!(
        source_run.contains("tailrocks/velnor-new")
            && source_run.contains("https://github.com/{repository}.git")
            && source_run.contains("GITHUB_REPOSITORY")
            && source_run.contains("GITHUB_SHA")
            && source_run.contains(r#"r"[0-9a-f]{40}""#)
            && source_run.contains("--depth=1")
            && source_run.contains("--no-tags")
            && source_run
                .contains("run_git(\"checkout\", \"--quiet\", \"--detach\", \"FETCH_HEAD\"")
            && source_run.contains("head != commit"),
        "{job_id} must validate, fetch, and verify the exact public source commit"
    );
    for credential in [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "MISE_GITHUB_TOKEN",
    ] {
        assert!(
            source_run.contains(&format!("\"{credential}\""))
                && source_run.contains("environment.pop(name, None)"),
            "{job_id} must scrub {credential} before fetching source"
        );
    }
    assert!(
        !has_field(source, "env"),
        "{job_id} source acquisition must not receive a credential env"
    );
    Ok(())
}

fn assert_qualifier_step(
    step: &Yaml,
    steps: &[Yaml],
    expected_action: &str,
    job_id: &str,
) -> Result<(), Box<dyn Error>> {
    let qualification = map_entries(step)?;
    assert_eq!(
        scalar(map_field(qualification, "uses")?)?,
        expected_action,
        "{job_id}"
    );
    for step in steps {
        let fields = map_entries(step)?;
        if let Some((_, uses)) = fields.iter().find(|(key, _)| key == "uses") {
            assert!(
                !scalar(uses)?.starts_with("actions/checkout@"),
                "{job_id} must not depend on the default checkout action"
            );
        }
    }
    Ok(())
}
