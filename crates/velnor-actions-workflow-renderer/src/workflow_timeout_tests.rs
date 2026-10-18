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
    for minutes in 1..=5 {
        let workflow = document(vec![
            ("runs-on", Yaml::str("ubuntu-26.04")),
            ("timeout-minutes", Yaml::Int(minutes)),
        ]);
        assert!(validate(PATH, &workflow).is_ok());
    }
    for minutes in [-1, 0, 6, 30, 75, 360, i64::MAX] {
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
    let unbounded = document(vec![("timeout-minutes", Yaml::Int(6))]);
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
