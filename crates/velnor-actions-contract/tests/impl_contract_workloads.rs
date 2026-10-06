//! Portable workload contract rejects command and path escape hatches.
use velnor_actions_contract::{StacksConfig, WorkloadConfig, WorkloadKind, is_valid_workload_path};

fn workload(name: &str, kind: WorkloadKind) -> WorkloadConfig {
    WorkloadConfig {
        name: name.to_owned(),
        kind,
        root: velnor_actions_contract::Utf8RepoRelDir::from_raw(".".to_owned()),
        inputs: Vec::new(),
        paths: Vec::new(),
        scripts: None,
        gradle: None,
        package_update: None,
        native_desktop: None,
    }
}

#[test]
fn fixed_kinds_and_default_root_parse() {
    let value: WorkloadConfig =
        serde_json::from_value(serde_json::json!({"name":"container", "kind":"docker_build"}))
            .expect("defaults parse");
    assert_eq!(value.kind, WorkloadKind::DockerBuild);
    assert_eq!(value.root.as_str(), ".");
    assert!(value.inputs.is_empty());
    assert!(value.paths.is_empty());
    assert!(value.validate("config.toml").is_ok());
    for kind in [
        WorkloadKind::BunCi,
        WorkloadKind::SwiftTest,
        WorkloadKind::Reuse,
    ] {
        assert!(workload("check", kind).validate("config.toml").is_ok());
    }
    assert!(
        serde_json::from_value::<WorkloadConfig>(serde_json::json!({"name": "x", "kind": "shell"}))
            .is_err()
    );
    for key in ["args", "shell", "command"] {
        let body = serde_json::json!({"name": "x", "kind": "reuse", key: "id"});
        assert!(serde_json::from_value::<WorkloadConfig>(body).is_err());
    }
}

#[test]
fn hostile_paths_and_roots_fail_closed() {
    for path in [
        "",
        ".",
        "..",
        "../a",
        "a/../b",
        "a/./b",
        "/a",
        "C:/a",
        "a\\b",
        "a//b",
        "a/",
        "-a",
        "a/-b",
        "a b",
        "a\nb",
        "a;id",
        "$(id)",
        "`id`",
        "${{secrets.x}}",
        "a'",
        "a*",
        "a?",
        "Casks/../tablerock-app@preview.rb",
        "Casks/-tablerock-app@preview.rb",
        "Casks/tablerock-app@$(id).rb",
    ] {
        assert!(!is_valid_workload_path(path), "{path:?}");
        let mut value = workload("check", WorkloadKind::Reuse);
        value.inputs = vec![path.to_owned()];
        assert!(value.validate("config.toml").is_err(), "{path:?}");
        value.inputs.clear();
        value.root = velnor_actions_contract::Utf8RepoRelDir::from_raw(path.to_owned());
        assert_eq!(
            value.validate("config.toml").is_ok(),
            path == ".",
            "{path:?}"
        );
    }
    for path in [
        "Dockerfile",
        ".reuse/dep5",
        "src/Test.swift",
        "a-b_c+1/file.rb",
        "Casks/tablerock-app@preview.rb",
    ] {
        assert!(is_valid_workload_path(path), "{path}");
    }
}

#[test]
fn explicit_file_kinds_require_paths_and_other_kinds_forbid_them() {
    for kind in [WorkloadKind::RubySyntax, WorkloadKind::Shellcheck] {
        let mut value = workload("check", kind);
        assert!(value.validate("config.toml").is_err());
        value.paths = vec!["scripts/check.sh".to_owned()];
        assert!(value.validate("config.toml").is_ok());
    }
    let mut value = workload("check", WorkloadKind::Reuse);
    value.paths = vec!["file".to_owned()];
    assert!(value.validate("config.toml").is_err());
}

#[test]
fn explicit_files_stay_inside_the_workload_root() {
    for kind in [WorkloadKind::RubySyntax, WorkloadKind::Shellcheck] {
        let mut value = workload("check", kind);
        value.root = velnor_actions_contract::Utf8RepoRelDir::from_raw("tools".to_owned());
        for path in ["outside.rb", "tools-other/check.rb", "tools"] {
            value.paths = vec![path.to_owned()];
            let err = value.validate("config.toml").expect_err("outside root");
            assert!(err.to_string().contains("path_outside_workload_root"));
        }
        value.paths = vec!["tools/check.rb".to_owned()];
        assert!(value.validate("config.toml").is_ok());
        value.root = velnor_actions_contract::Utf8RepoRelDir::from_raw(".".to_owned());
        value.paths = vec!["check.rb".to_owned()];
        assert!(value.validate("config.toml").is_ok());
    }
}

#[test]
fn lists_require_sorted_unique_values_and_safe_names() {
    for name in ["", ".", "..", "-check", "a/b", "a b", "${{x}}"] {
        assert!(
            workload(name, WorkloadKind::Reuse)
                .validate("config.toml")
                .is_err()
        );
    }
    for paths in [vec!["b", "a"], vec!["a", "a"]] {
        let mut value = workload("check", WorkloadKind::Reuse);
        value.inputs = paths.into_iter().map(str::to_owned).collect();
        assert!(value.validate("config.toml").is_err());
    }
    for names in [["b", "a"], ["a", "a"]] {
        let value = StacksConfig {
            ignore: Vec::new(),
            rust: None,
            tofu: None,
            workloads: names
                .into_iter()
                .map(|name| workload(name, WorkloadKind::Reuse))
                .collect(),
        };
        assert!(value.validate("config.toml").is_err());
    }
}

#[test]
fn ignore_cannot_retire_declared_workload_obligations() {
    for workloads in [Vec::new(), vec![workload("licenses", WorkloadKind::Reuse)]] {
        let stacks = StacksConfig {
            ignore: vec!["workload".to_owned()],
            rust: None,
            tofu: None,
            workloads,
        };
        let error = stacks.validate("config.toml").expect_err("required stack");
        assert!(
            error
                .to_string()
                .contains("cannot_ignore_declared_workloads")
        );
    }
}

#[test]
fn native_checks_require_declared_profiles_and_reject_removed_names() {
    for kind in [
        WorkloadKind::NativeXcodeProjectCi,
        WorkloadKind::NativeSwiftPackageCi,
    ] {
        let error = workload("native", kind)
            .validate("config.toml")
            .expect_err("profile required");
        assert!(error.to_string().contains("missing_native_desktop_profile"));
    }
    for kind in [
        "jackin_desktop_ci",
        "jackin_swift_package_ci",
        "gradle_test",
    ] {
        assert!(
            serde_json::from_value::<WorkloadConfig>(
                serde_json::json!({"name":"check","kind":kind})
            )
            .is_err()
        );
    }
}
