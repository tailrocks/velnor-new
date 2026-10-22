use super::*;

const PATH: &str = ".github/workflows/test.yml";

fn document(fields: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "test".to_owned(),
            Yaml::Map(
                fields
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value))
                    .collect(),
            ),
        )]),
    )])
}

#[test]
fn every_admitted_concrete_job_has_a_literal_bounded_timeout() {
    for minutes in [1, 5, 6, 30, 75, 360] {
        let workflow = document(vec![
            ("runs-on", Yaml::str("ubuntu-26.04")),
            ("timeout-minutes", Yaml::Int(minutes)),
        ]);
        assert!(validate(PATH, &workflow).is_ok());
    }
    for minutes in [-1, 0, 361, i64::MAX] {
        let workflow = document(vec![("timeout-minutes", Yaml::Int(minutes))]);
        assert!(validate(PATH, &workflow).is_err(), "{minutes}");
    }
    for fields in [
        vec![],
        vec![("timeout-minutes", Yaml::str("${{ inputs.timeout }}"))],
    ] {
        assert!(validate(PATH, &document(fields)).is_err());
    }
}

#[test]
fn only_the_preview_matrix_uses_native_bounded_timeout_outputs() {
    let path = preview_path();
    let payload = payload_job("preview-payload", false, "selector");
    let valid_payload = validate(path, &payload);
    assert!(
        valid_payload.is_ok(),
        "{valid_payload:?}; path={path}; payload={payload:?}"
    );
    for (job, fail_fast, selector) in [
        ("other-job", false, "selector"),
        ("preview-payload", true, "selector"),
        ("preview-payload", false, "caller"),
    ] {
        assert!(validate(path, &payload_job(job, fail_fast, selector)).is_err());
    }
    assert!(validate(PATH, &payload_job("preview-payload", false, "selector")).is_err());
}

#[test]
fn preview_assembly_timeout_requires_exact_selector_and_workflow_run_gate() {
    let path = preview_path();
    assert!(validate(path, &assembly_job("preview-assemble", true, "selector")).is_ok());
    for (job, gate, selector, timeout) in [
        ("other-job", true, "selector", "parsed"),
        ("preview-assemble", false, "selector", "parsed"),
        ("preview-assemble", true, "unbound", "parsed"),
        ("preview-assemble", true, "selector", "source"),
        ("preview-assemble", true, "selector", "caller"),
        ("preview-assemble", true, "selector", "arithmetic"),
    ] {
        assert!(validate(path, &assembly_job_with(job, gate, selector, timeout)).is_err());
    }
    assert!(validate(PATH, &assembly_job("preview-assemble", true, "selector")).is_err());
}

fn preview_path() -> &'static str {
    velnor_actions_contract::package_release::PackageReleaseChannel::Preview.workflow_path()
}

fn payload_job(job_id: &str, fail_fast: bool, selector: &str) -> Yaml {
    let matrix = match selector {
        "selector" => "${{ fromJSON(needs.preview-eligibility.outputs.payload_matrix) }}",
        _ => "${{ fromJSON(inputs.payload_matrix) }}",
    };
    let mut workflow = document(vec![
        (
            "if",
            Yaml::str("${{ github.event_name == 'workflow_run' }}"),
        ),
        (
            "timeout-minutes",
            Yaml::str("${{ matrix.timeout_minutes }}"),
        ),
        ("needs", Yaml::Seq(vec![Yaml::str("preview-eligibility")])),
        (
            "strategy",
            Yaml::Map(vec![
                ("fail-fast".to_owned(), Yaml::Bool(fail_fast)),
                ("matrix".to_owned(), Yaml::str(matrix)),
            ]),
        ),
    ]);
    rename_job(&mut workflow, job_id);
    workflow
}

fn assembly_job(job_id: &str, gate: bool, selector: &str) -> Yaml {
    assembly_job_with(job_id, gate, selector, "parsed")
}

fn assembly_job_with(job_id: &str, gate: bool, selector: &str, timeout: &str) -> Yaml {
    let timeout = match timeout {
        "parsed" => "${{ fromJSON(needs.preview-eligibility.outputs.producer_timeout_minutes) }}",
        "source" => "${{ needs.preview-eligibility.outputs.producer_timeout_minutes }}",
        "arithmetic" => {
            "${{ fromJSON(needs.preview-eligibility.outputs.producer_timeout_minutes) + 5 }}"
        }
        _ => "${{ inputs.timeout }}",
    };
    let needs = if selector == "selector" {
        vec![
            Yaml::str("preview-eligibility"),
            Yaml::str("preview-payload"),
        ]
    } else {
        vec![Yaml::str("preview-payload")]
    };
    let gate = if gate {
        "${{ github.event_name == 'workflow_run' }}"
    } else {
        "${{ always() }}"
    };
    let mut workflow = document(vec![
        ("if", Yaml::str(gate)),
        ("timeout-minutes", Yaml::str(timeout)),
        ("needs", Yaml::Seq(needs)),
    ]);
    rename_job(&mut workflow, job_id);
    workflow
}

fn rename_job(workflow: &mut Yaml, id: &str) {
    if let Yaml::Map(fields) = workflow
        && let Some((_, Yaml::Map(jobs))) = fields.iter_mut().find(|(key, _)| key == "jobs")
    {
        jobs[0].0 = id.to_owned();
    }
}

#[test]
fn reusable_callers_only_delegate_to_locally_admitted_workflows() {
    let local = document(vec![("uses", Yaml::str("./.github/workflows/release.yml"))]);
    assert!(validate(PATH, &local).is_ok());
    let external = document(vec![(
        "uses",
        Yaml::str("example/project/.github/workflows/release.yml@main"),
    )]);
    assert!(validate(PATH, &external).is_err());
    assert!(validate(".github/actions/test/action.yml", &Yaml::Null).is_ok());
}

#[test]
fn checked_renderer_cannot_emit_an_unbounded_workflow() {
    let workflow = document(vec![("runs-on", Yaml::str("ubuntu-26.04"))]);
    assert!(crate::render::fallback::render_checked_workflow(PATH, &workflow, "0.1.0").is_err());
}

#[test]
fn reusable_inventory_requires_every_bounded_callee_and_unique_paths() {
    let caller = document(vec![(
        "uses",
        Yaml::annotated("./.github/workflows/release.yml", "reviewed"),
    )]);
    let release_workflow = document(vec![("timeout-minutes", Yaml::Int(5))]);
    let path = ".github/workflows/release.yml";
    assert!(validate_tree(&[(PATH, &caller), (path, &release_workflow)]).is_ok());
    assert!(validate_tree(&[(PATH, &caller)]).is_err());
    let unbounded = document(vec![("timeout-minutes", Yaml::Int(361))]);
    assert!(validate_tree(&[(PATH, &caller), (path, &unbounded)]).is_err());
    assert!(validate_tree(&[(path, &release_workflow), (path, &release_workflow)]).is_err());
    for reference in [
        "./.github/workflows/../release.yml",
        "./.github/workflows/release.yml@main",
        "${{ inputs.callee }}",
    ] {
        assert!(validate(PATH, &document(vec![("uses", Yaml::str(reference))])).is_err());
    }
}

#[test]
fn reusable_callers_cannot_smuggle_executable_job_fields() {
    for key in ["runs-on", "steps", "timeout-minutes"] {
        let mixed = document(vec![
            ("uses", Yaml::str("./.github/workflows/release.yml")),
            (key, Yaml::Int(360)),
        ]);
        assert!(validate(PATH, &mixed).is_err(), "{key}");
    }
}
