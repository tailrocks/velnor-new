//! Logical task-template cases.
use velnor_actions_mise::TaskTemplate;

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
