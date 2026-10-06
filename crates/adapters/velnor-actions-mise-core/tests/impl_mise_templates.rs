//! Logical task-template cases.
use velnor_actions_mise_core::{MiseError, ProcessOutput, TaskTemplate};

#[test]
fn template_names_are_stable() {
    let names: Vec<&str> = TaskTemplate::ALL.iter().map(|task| task.name()).collect();
    assert_eq!(
        names,
        [
            "fmt-check",
            "dependencies",
            "actionlint",
            "zizmor",
            "clippy",
            "test-build",
            "test",
            "doctest",
            "doc",
            "msrv",
        ]
    );
    assert_eq!(TaskTemplate::ALL.len(), 10);
}

#[test]
fn template_names_roundtrip() {
    for task in TaskTemplate::ALL {
        assert_eq!(TaskTemplate::from_name(task.name()), Some(task));
    }
    for bad in [
        "",
        "fmt",
        "clippy ",
        "test_build",
        "workspace",
        ".mise/tasks/clippy",
    ] {
        assert_eq!(TaskTemplate::from_name(bad), None, "{bad} must not resolve");
    }
}

#[test]
fn template_execution_preserves_exit_status() {
    for template in TaskTemplate::ALL {
        assert!(
            template.targets_single_package() || template.is_repo_wide(),
            "{} declares its arity",
            template.name()
        );
    }
    let failed = ProcessOutput {
        stdout: Vec::new(),
        stderr: b"clippy failed".to_vec(),
        code: Some(3),
        signal: None,
        success: false,
    };
    let err = failed
        .require_success("mise")
        .expect_err("nonzero exit must surface");
    assert_eq!(
        err,
        MiseError::NonZeroExit {
            program: "mise".to_owned(),
            code: Some(3),
            stderr: "clippy failed".to_owned(),
        }
    );
    let passed = ProcessOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        code: Some(0),
        signal: None,
        success: true,
    };
    assert!(passed.require_success("mise").is_ok());
}

#[test]
fn only_dependencies_is_repo_wide() {
    for task in TaskTemplate::ALL {
        if task == TaskTemplate::Dependencies {
            assert!(task.is_repo_wide());
            assert!(!task.targets_single_package());
        } else {
            assert!(
                !task.is_repo_wide(),
                "{} must be package-scoped",
                task.name()
            );
            assert!(task.targets_single_package());
        }
    }
}
