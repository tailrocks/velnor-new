use super::*;

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::Duration;

use velnor_actions_orchestrator::QualificationCacheBindingOutputs;

fn bind_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("RUNNER_TEMP".to_owned(), "/runner/temp".to_owned()),
        ("GITHUB_RUN_ID".to_owned(), "700".to_owned()),
        ("GITHUB_RUN_ATTEMPT".to_owned(), "1".to_owned()),
        (
            "GITHUB_EVENT_NAME".to_owned(),
            "workflow_dispatch".to_owned(),
        ),
        ("GITHUB_REPOSITORY".to_owned(), "owner/project".to_owned()),
        ("GITHUB_REF".to_owned(), "refs/heads/main".to_owned()),
        ("GITHUB_REF_PROTECTED".to_owned(), "true".to_owned()),
        (
            "GITHUB_WORKFLOW_REF".to_owned(),
            "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        ),
        ("GITHUB_WORKFLOW_SHA".to_owned(), "a".repeat(40)),
        ("GITHUB_SHA".to_owned(), "a".repeat(40)),
        ("RUNNER_OS".to_owned(), "Linux".to_owned()),
        ("RUNNER_ARCH".to_owned(), "X64".to_owned()),
        ("ImageOS".to_owned(), "ubuntu26".to_owned()),
        ("ImageVersion".to_owned(), "20261018.1".to_owned()),
        (MATRIX_KEY_ENV.to_owned(), "matrix-key".to_owned()),
        (LAYER_ENV.to_owned(), "cargo_sources".to_owned()),
        (RUNTIME_IDENTITY_ENV.to_owned(), "{}".to_owned()),
    ])
}

fn read(map: &BTreeMap<String, String>) -> Result<BindInputs, String> {
    read_inputs(|name| map.get(name).cloned().ok_or(env::VarError::NotPresent))
}

#[test]
fn reads_exact_runner_and_workflow_identity() {
    let inputs = read(&bind_environment()).expect("cache binding inputs");
    assert_eq!(inputs.run_id, 700);
    assert_eq!(inputs.run_attempt, 1);
    assert!(inputs.ref_protected);
    assert_eq!(inputs.matrix_key, "matrix-key");
    assert_eq!(inputs.layer, "cargo_sources");
    assert_eq!(inputs.runner_temp, PathBuf::from("/runner/temp"));
}

#[test]
fn rejects_missing_context_bad_booleans_and_noncanonical_numbers() {
    let mut values = bind_environment();
    values.remove("GITHUB_WORKFLOW_SHA");
    assert!(read(&values).is_err());

    let mut values = bind_environment();
    values.insert("GITHUB_REF_PROTECTED".to_owned(), "yes".to_owned());
    assert_eq!(read(&values).expect_err("bad bool"), "bad_ref_protected");

    let mut values = bind_environment();
    values.insert("GITHUB_RUN_ID".to_owned(), "0700".to_owned());
    assert_eq!(
        read(&values).expect_err("noncanonical run id"),
        "bad_run_id"
    );
}

#[test]
fn rejects_path_traversal_and_oversize_runtime_identity() {
    let mut values = bind_environment();
    values.insert("RUNNER_TEMP".to_owned(), "/runner/../other".to_owned());
    assert_eq!(
        read(&values).expect_err("path traversal"),
        "runner_temp_path_invalid"
    );

    let mut values = bind_environment();
    values.insert(
        RUNTIME_IDENTITY_ENV.to_owned(),
        "x".repeat(MAX_RUNTIME_IDENTITY_BYTES + 1),
    );
    assert_eq!(
        read(&values).expect_err("oversize runtime identity"),
        "cache_binding_input_json_invalid"
    );
}

#[cfg(unix)]
#[test]
fn output_fifo_is_rejected_without_blocking() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let output = temp.path().join("output-fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&output)
        .status()
        .expect("mkfifo");
    assert!(status.success(), "mkfifo returned {status}");

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = append_owned_output(output.as_os_str(), b"active=true\n");
        sender
            .send(result)
            .expect("test is waiting for the FIFO result");
    });
    let result = receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("FIFO open must not block");
    assert!(result.is_err());
}

#[test]
fn output_body_uses_fixed_single_line_names() {
    let outputs = QualificationCacheBindingOutputs {
        active: true,
        campaign: "campaign-1".to_owned(),
        phase: "cold".to_owned(),
        plan_id: "plan-1".to_owned(),
        run_key: "r700-a1".to_owned(),
        matrix_key: "matrix-key".to_owned(),
        layer: "cargo_sources".to_owned(),
        restore_key: Some("velnor-qcache-v1-campaign-1-cargo_sources-k1".to_owned()),
        expected_cache: None,
        save_key: Some("velnor-qcache-v1-campaign-1-cargo_sources-k1".to_owned()),
        save_if_state_changes: false,
        expected_prior_state_digest: None,
        runtime_identity_digest: Some("b3-identity".to_owned()),
    };
    let body = output_body(&outputs).expect("output body");
    assert!(body.lines().any(|line| line == "active=true"));
    assert!(
        body.lines()
            .any(|line| line == "restore_key=velnor-qcache-v1-campaign-1-cargo_sources-k1")
    );
    assert!(
        body.lines()
            .any(|line| line == "save_key=velnor-qcache-v1-campaign-1-cargo_sources-k1")
    );
    assert!(
        body.lines()
            .any(|line| line == "runtime_identity_digest=b3-identity")
    );
}

#[test]
fn output_body_rejects_line_injection() {
    let mut outputs = QualificationCacheBindingOutputs {
        active: false,
        campaign: "campaign-1".to_owned(),
        phase: "control".to_owned(),
        plan_id: "plan-1".to_owned(),
        run_key: "r700-a1".to_owned(),
        matrix_key: "matrix-key".to_owned(),
        layer: "task_result".to_owned(),
        restore_key: None,
        expected_cache: None,
        save_key: None,
        save_if_state_changes: false,
        expected_prior_state_digest: None,
        runtime_identity_digest: None,
    };
    outputs.campaign = "campaign-1\nforged=true".to_owned();
    assert_eq!(output_body(&outputs), Err("cache_binding_output_invalid"));
}

#[test]
fn output_writer_appends_owned_file_and_rejects_path_aliases() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let output = temp.path().join("github-output");
    std::fs::write(&output, "prior=true\n").expect("output file");
    append_owned_output(output.as_os_str(), b"active=true\n").expect("append output");
    assert_eq!(
        std::fs::read_to_string(&output).expect("output text"),
        "prior=true\nactive=true\n"
    );

    let directory = temp.path().join("output-directory");
    std::fs::create_dir(&directory).expect("output directory");
    assert!(append_owned_output(directory.as_os_str(), b"active=true\n").is_err());

    let traversal = temp.path().join("../outside");
    assert!(append_owned_output(traversal.as_os_str(), b"active=true\n").is_err());

    #[cfg(unix)]
    {
        let alias = temp.path().join("output-alias");
        std::os::unix::fs::symlink(&output, &alias).expect("output symlink");
        assert!(append_owned_output(alias.as_os_str(), b"forged=true\n").is_err());
        assert_eq!(
            std::fs::read_to_string(&output).expect("output text"),
            "prior=true\nactive=true\n"
        );
    }
}
