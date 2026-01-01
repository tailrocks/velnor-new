//! Staged validation gates fail closed before any tool runs.

use velnor_actions_mise::ProcessOutput;
use velnor_actions_orchestrator_staged_validation::validate::{
    diagnose, is_workflow_path, validate_staged, verify_velnor_repository_files,
};
use velnor_actions_workflow_tree::rendered::{RenderedFile, RenderedTree};

#[test]
fn workflow_paths_match_workflows_dir_yaml_only() {
    assert!(is_workflow_path(".github/workflows/ci.yml"));
    assert!(is_workflow_path(".github/workflows/ci.yaml"));
    assert!(!is_workflow_path(".github/actionlint.yaml"));
    assert!(!is_workflow_path(".github/workflows/notes.txt"));
    assert!(!is_workflow_path("workflows/ci.yml"));
}

#[test]
fn diagnose_prefers_stdout_then_stderr_then_exit_code() {
    let output = ProcessOutput {
        stdout: b"lint failed".to_vec(),
        stderr: b"ignored".to_vec(),
        code: Some(2),
        signal: None,
        success: false,
    };
    assert_eq!(diagnose(&output), "lint failed");
    let output = ProcessOutput {
        stdout: Vec::new(),
        stderr: b"stderr only".to_vec(),
        code: Some(2),
        signal: None,
        success: false,
    };
    assert_eq!(diagnose(&output), "stderr only");
    let output = ProcessOutput {
        stdout: Vec::new(),
        stderr: b"   ".to_vec(),
        code: Some(3),
        signal: None,
        success: false,
    };
    assert_eq!(diagnose(&output), "exit_code:3");
}

#[test]
fn diagnose_truncates_long_output() {
    let output = ProcessOutput {
        stdout: vec![b'x'; 5000],
        stderr: Vec::new(),
        code: Some(1),
        signal: None,
        success: false,
    };
    let text = diagnose(&output);
    assert!(text.len() < 5000);
    assert!(text.ends_with("…[truncated]"));
}

#[test]
fn missing_repository_files_verify_to_none() {
    let root = tempfile::TempDir::new().expect("root");
    let lock = verify_velnor_repository_files(root.path()).expect("absent");
    assert!(lock.is_none());
}

#[test]
fn malformed_lock_fails_verification() {
    let root = tempfile::TempDir::new().expect("root");
    std::fs::create_dir(root.path().join(".velnor")).expect("dirs");
    std::fs::write(root.path().join(".velnor/generator.lock"), "!!!not-a-lock").expect("lock");
    assert!(verify_velnor_repository_files(root.path()).is_err());
}

#[test]
fn staged_validation_rejects_missing_config_and_empty_workflows() {
    let empty = RenderedTree {
        files: Vec::new(),
        symlinks: Vec::new(),
    };
    assert!(validate_staged(&empty).is_err());
    let config_only = RenderedTree {
        files: vec![RenderedFile {
            path: ".github/actionlint.yaml".into(),
            bytes: "config: true\n".into(),
        }],
        symlinks: Vec::new(),
    };
    assert!(validate_staged(&config_only).is_err());
}
