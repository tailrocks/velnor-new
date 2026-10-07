use std::collections::BTreeSet;
use std::error::Error;
use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_workflow_generator::GeneratorReleasePins;
use velnor_actions_workflow_generator::Schema2WorkflowRequest;
use velnor_actions_workflow_generator::generator_release::{GeneratorRelease, generator_release};
use velnor_actions_workflow_steps::setup::MiseSetup;
use velnor_actions_workflow_tree::yaml::Yaml;

pub(crate) fn distinct_pins() -> GeneratorReleasePins {
    let mut pins = test_pins();
    pins.linux_x86_64_setup = mise_setup('a', "2026.9.18", 'b');
    pins.macos_arm64_setup = mise_setup('c', "2026.9.19", 'd');
    pins.macos_x86_64_setup = mise_setup('e', "2026.9.20", 'f');
    pins
}

pub(crate) fn render(pins: &GeneratorReleasePins) -> Result<GeneratorRelease, Box<dyn Error>> {
    let request = Schema2WorkflowRequest {
        version: "0.1.1".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        generator_release: Some(pins.clone()),
    };
    Ok(generator_release(&request)?)
}

pub(crate) fn workflow_jobs(
    release: &GeneratorRelease,
) -> Result<&[(String, Yaml)], Box<dyn Error>> {
    map_entries(map_field(map_entries(&release.workflow)?, "jobs")?)
}

pub(crate) fn qualifier_cases() -> [(&'static str, &'static str, ReleaseTarget); 3] {
    [
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
    ]
}

fn candidate_steps(jobs: &[(String, Yaml)]) -> Result<&[Yaml], Box<dyn Error>> {
    let candidate = map_field(jobs, "candidate-manifest")?;
    let steps = sequence(map_field(map_entries(candidate)?, "steps")?)?;
    assert!(steps.len() > 2);
    Ok(steps)
}

#[test]
fn setup_for_returns_the_target_pinned_setup() {
    let pins = distinct_pins();
    assert_eq!(
        pins.setup_for(ReleaseTarget::LinuxX86_64).version,
        "2026.9.18"
    );
    assert_eq!(
        pins.setup_for(ReleaseTarget::MacosArm64).version,
        "2026.9.19"
    );
    assert_eq!(
        pins.setup_for(ReleaseTarget::MacosX86_64).version,
        "2026.9.20"
    );
}

#[test]
fn candidate_manifest_provisions_target_pinned_mise() -> Result<(), Box<dyn Error>> {
    let pins = distinct_pins();
    let release = render(&pins)?;
    let jobs = workflow_jobs(&release)?;
    let steps = candidate_steps(jobs)?;
    assert_setup_step(
        &steps[1],
        pins.setup_for(ReleaseTarget::LinuxX86_64),
        "candidate-manifest",
    )?;
    Ok(())
}

#[test]
fn candidate_manifest_guards_archives_only_after_mise() -> Result<(), Box<dyn Error>> {
    let pins = distinct_pins();
    let release = render(&pins)?;
    let jobs = workflow_jobs(&release)?;
    assert_guard_runs_after_step(candidate_steps(jobs)?, 1, "candidate-manifest");
    Ok(())
}

pub(crate) fn assert_setup_step(
    step: &Yaml,
    setup: &MiseSetup,
    job_id: &str,
) -> Result<(), Box<dyn Error>> {
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

pub(crate) fn assert_guard_runs_after_step(steps: &[Yaml], setup_index: usize, job_id: &str) {
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

pub(crate) fn map_entries(value: &Yaml) -> Result<&[(String, Yaml)], Box<dyn Error>> {
    match value {
        Yaml::Map(entries) => Ok(entries),
        _ => Err("expected YAML mapping".into()),
    }
}

pub(crate) fn map_field<'a>(
    entries: &'a [(String, Yaml)],
    key: &str,
) -> Result<&'a Yaml, Box<dyn Error>> {
    entries
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| format!("missing YAML mapping field {key}").into())
}

pub(crate) fn has_field(entries: &[(String, Yaml)], key: &str) -> bool {
    entries.iter().any(|(name, _)| name == key)
}

pub(crate) fn sequence(value: &Yaml) -> Result<&[Yaml], Box<dyn Error>> {
    match value {
        Yaml::Seq(items) => Ok(items),
        _ => Err("expected YAML sequence".into()),
    }
}

pub(crate) fn scalar(value: &Yaml) -> Result<&str, Box<dyn Error>> {
    match value {
        Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. } => Ok(value),
        _ => Err(format!("expected YAML scalar string, got {value:?}").into()),
    }
}
