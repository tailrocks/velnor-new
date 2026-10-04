//! Source-only adversarial descriptor and identity checks.

use super::*;
use velnor_actions_contract::config::PackageUpdateFixture;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn workload(kind: &str, package_update: Option<serde_json::Value>) -> TestResultWorkload {
    Ok(serde_json::from_value(serde_json::json!({
        "name": "native", "kind": kind, "root": ".",
        "package_update": package_update,
    }))?)
}

type TestResultWorkload = Result<WorkloadConfig, Box<dyn std::error::Error>>;

fn package() -> Result<WorkloadConfig, Box<dyn std::error::Error>> {
    workload(
        "package_update_fixture",
        Some(serde_json::json!({
            "updater": "scripts/update.sh", "repository": "owner/project",
            "formula": "Formula/project.rb", "preview_formula": "Formula/project-preview.rb",
            "binary": "project", "artifacts": [{
                "prefix": "project", "target": "aarch64-apple-darwin", "archive": "tar_gz",
                "output": "formula", "preview": true, "executable": true,
            }], "supporting_manifests": [],
        })),
    )
}

fn package_payload(profile: &PackageUpdateFixture) -> Result<Vec<OsString>, OrchestratorError> {
    Ok(package_update::arguments(profile)?
        .into_iter()
        .map(OsString::from)
        .collect())
}

fn homebrew_payload(has_casks: bool) -> Result<Vec<OsString>, OrchestratorError> {
    let mut arguments = vec!["homebrew-preparation".to_owned()];
    arguments.extend(preparation::preparation_arguments(
        &source_identity()?,
        &audit::TapIdentity::from_repository("owner/homebrew-project")?,
        has_casks,
    ));
    Ok(arguments.into_iter().map(OsString::from).collect())
}

fn proposal(
    workload: &WorkloadConfig,
    phase: &str,
    payload: Vec<OsString>,
) -> Result<ProposedTask, Box<dyn std::error::Error>> {
    let descriptor = from_workload(workload, phase, &payload)?.ok_or("descriptor missing")?;
    let strings = crate::utf8::strings_of(payload)?;
    let mut task = super::super::proposal(workload, phase, strings, None);
    task.identity.environment.insert(
        NATIVE_VALIDATION_DESCRIPTOR_KEY.to_owned(),
        String::from_utf8(canonical_json_bytes(&descriptor)?)?,
    );
    Ok(task)
}

#[test]
fn both_native_semantic_requests_round_trip_without_execution() -> TestResult {
    let package = package()?;
    let profile = package.package_update.as_ref().ok_or("profile missing")?;
    let task = proposal(&package, package_update::PHASE, package_payload(profile)?)?;
    assert_eq!(
        from_proposal(&task)?,
        Some(NativeValidationDescriptor::PackageUpdateFixture {
            profile: profile.clone(),
        })
    );
    for has_casks in [false, true] {
        let task = proposal(
            &workload("homebrew_audit", None)?,
            "homebrew-tap-local",
            homebrew_payload(has_casks)?,
        )?;
        assert_eq!(
            from_proposal(&task)?,
            Some(NativeValidationDescriptor::HomebrewPreparation {
                repository: "owner/homebrew-project".to_owned(),
                has_casks,
            })
        );
    }
    Ok(())
}

#[test]
fn malformed_unknown_duplicate_null_and_noncanonical_requests_fail() -> TestResult {
    let task = proposal(
        &workload("homebrew_audit", None)?,
        "homebrew-tap-local",
        homebrew_payload(false)?,
    )?;
    for raw in [
        "null".to_owned(), "{".to_owned(),
        r#"{"kind":"homebrew-preparation","repository":"owner/homebrew-project","has_casks":false,"source_sha":"arbitrary"}"#.to_owned(),
        r#"{"kind":"homebrew-preparation","repository":"owner/homebrew-project","repository":"other/homebrew-project","has_casks":false}"#.to_owned(),
        r#"{"kind":"homebrew-preparation","repository":"OWNER/homebrew-project","has_casks":false}"#.to_owned(),
        format!(" {}", task.identity.environment[NATIVE_VALIDATION_DESCRIPTOR_KEY]),
        "x".repeat(MAX_DESCRIPTOR_BYTES + 1),
    ] {
        let mut altered = task.clone();
        altered.identity.environment.insert(NATIVE_VALIDATION_DESCRIPTOR_KEY.to_owned(), raw);
        assert!(from_proposal(&altered).is_err());
    }
    Ok(())
}

#[test]
fn required_descriptors_and_closed_owner_identity_cannot_be_reassigned() -> TestResult {
    let task = proposal(
        &workload("homebrew_audit", None)?,
        "homebrew-tap-local",
        homebrew_payload(false)?,
    )?;
    let mut missing = task.clone();
    missing.identity.environment.clear();
    assert!(from_proposal(&missing).is_err());
    let mutations: &[fn(&mut ProposedTask)] = &[
        |task| task.configuration = "node_ci".to_owned(),
        |task| task.task_kind = "homebrew-audit".to_owned(),
        |task| task.stack_id = Stack::Rust.id().to_owned(),
        |task| task.task_id.push_str("/other"),
        |task| task.component_id = "other".to_owned(),
        |task| task.identity.unit_id = "workload:other".to_owned(),
        |task| task.identity.unit_key = "other".to_owned(),
        |task| task.identity.unit_path = "nested".to_owned(),
        |task| task.identity.project_root = "nested".to_owned(),
        |task| task.identity.compile_driver = "custom".to_owned(),
        |task| task.identity.test_runner = "custom".to_owned(),
        |task| task.identity.target = "custom".to_owned(),
        |task| task.runner_profile = "custom".to_owned(),
    ];
    for mutate in mutations {
        let mut altered = task.clone();
        mutate(&mut altered);
        assert!(from_proposal(&altered).is_err());
    }
    missing.task_kind = "homebrew-audit".to_owned();
    assert_eq!(from_proposal(&missing)?, None);
    Ok(())
}

#[test]
fn descriptor_cannot_select_foreign_pins_or_a_different_fixture() -> TestResult {
    let brew = workload("homebrew_audit", None)?;
    let payload = homebrew_payload(false)?;
    for index in 0..payload.len() {
        let mut altered = payload.clone();
        altered[index] = OsString::from(if matches!(index, 1 | 2) {
            "../foreign"
        } else {
            "arbitrary"
        });
        assert!(from_workload(&brew, "homebrew-tap-local", &altered).is_err());
    }
    let package = package()?;
    let profile = package.package_update.as_ref().ok_or("profile missing")?;
    let mut task = proposal(&package, package_update::PHASE, package_payload(profile)?)?;
    let mut foreign = profile.clone();
    foreign.binary = "foreign".to_owned();
    task.payload = package_payload(&foreign)?;
    assert!(from_proposal(&task).is_err());
    assert!(from_workload(&package, "test", &task.payload).is_err());
    assert!(
        from_workload(
            &workload("package_update_fixture", None)?,
            package_update::PHASE,
            &[]
        )
        .is_err()
    );
    task.configuration = "homebrew_audit".to_owned();
    task.task_kind = "homebrew-tap-local".to_owned();
    task.task_id = "stack/workload/native/homebrew-tap-local/homebrew_audit".to_owned();
    assert!(from_proposal(&task).is_err());
    Ok(())
}

#[test]
fn unrelated_stacks_keep_configuration_names_without_native_markers() -> TestResult {
    let original = proposal(
        &workload("homebrew_audit", None)?,
        "homebrew-tap-local",
        homebrew_payload(false)?,
    )?;
    for stack in [Stack::Rust, Stack::Tofu] {
        for phase in [package_update::PHASE, "ordinary"] {
            let mut task = original.clone();
            task.stack_id = stack.id().to_owned();
            task.configuration = "package_update_fixture".to_owned();
            task.task_kind = phase.to_owned();
            assert!(from_proposal(&task).is_err());
            task.identity.environment.clear();
            assert_eq!(from_proposal(&task)?, None);
        }
    }
    Ok(())
}
