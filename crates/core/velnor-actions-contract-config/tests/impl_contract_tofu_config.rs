//! Strict minimal `[stacks.tofu]` configuration cases (T09).
use velnor_actions_contract_config::{StacksConfig, TofuStackConfig, Utf8RepoRelDir, VelnorConfig};
use velnor_actions_contract_planning::{
    DetectedProject, DetectionStatus, IGNORED_REASON, apply_stack_ignores, selected_projects,
};

/// Roots list from raw spellings.
fn roots(raw: &[&str]) -> Vec<Utf8RepoRelDir> {
    raw.iter()
        .map(|entry| Utf8RepoRelDir::from_raw((*entry).to_owned()))
        .collect()
}

/// Tofu config from raw root spellings.
fn tofu(raw: &[&str]) -> TofuStackConfig {
    TofuStackConfig { roots: roots(raw) }
}

#[test]
fn dot_and_sorted_subdirs_validate() {
    let config = tofu(&[".", "envs/prod", "modules/vpc"]);
    assert!(config.validate("config.toml").is_ok());
}

#[test]
fn single_subdir_root_validates() {
    let config = tofu(&["infra"]);
    assert!(config.validate("config.toml").is_ok());
}

#[test]
fn empty_roots_rejected() {
    let err = tofu(&[]).validate("config.toml").expect_err("empty fails");
    assert!(err.to_string().contains("empty_roots"), "{err}");
}

#[test]
fn unsorted_roots_rejected() {
    let err = tofu(&["zeta", "."])
        .validate("config.toml")
        .expect_err("unsorted fails");
    assert!(err.to_string().contains("must_be_sorted"), "{err}");
}

#[test]
fn duplicate_roots_rejected() {
    let err = tofu(&[".", "."])
        .validate("config.toml")
        .expect_err("duplicate fails");
    assert!(err.to_string().contains("duplicate_root"), "{err}");
}

#[test]
fn absolute_and_drive_roots_rejected() {
    for raw in ["/etc/tofu", "C:/infra", "c:infra"] {
        let err = tofu(&[raw])
            .validate("config.toml")
            .expect_err("non-relative fails");
        let text = err.to_string();
        assert!(
            text.contains("absolute_root") || text.contains("drive_letter"),
            "{raw}: {text}"
        );
    }
}

#[test]
fn dot_and_dotdot_segments_rejected() {
    for (raw, code) in [
        ("./infra", "dot_segment"),
        ("infra/./prod", "dot_segment"),
        ("../infra", "dotdot_segment"),
        ("infra/../prod", "dotdot_segment"),
        ("..", "dotdot_segment"),
    ] {
        let err = tofu(&[raw])
            .validate("config.toml")
            .expect_err("bad segment fails");
        assert!(err.to_string().contains(code), "{raw}: {err}");
    }
}

#[test]
fn empty_segments_rejected() {
    for raw in ["infra//prod", "infra/", ""] {
        let err = tofu(&[raw])
            .validate("config.toml")
            .expect_err("empty segment fails");
        let text = err.to_string();
        assert!(
            text.contains("empty_segment") || text.contains("empty_root"),
            "{raw:?}: {text}"
        );
    }
}

#[test]
fn backslash_and_control_bytes_rejected() {
    for (raw, code) in [
        ("infra\\prod", "backslash"),
        ("infra\tprod", "control_byte"),
        ("infra\nprod", "control_byte"),
        ("infra\0prod", "control_byte"),
    ] {
        let err = tofu(&[raw])
            .validate("config.toml")
            .expect_err("unsafe spelling fails");
        assert!(err.to_string().contains(code), "{raw:?}: {err}");
    }
    let err = tofu(&["infra\nprod"])
        .validate("config.toml")
        .expect_err("control fails");
    assert_eq!(err.to_string().lines().count(), 1, "single line: {err}");
}

#[test]
fn unknown_tofu_keys_rejected() {
    let parsed: Result<TofuStackConfig, _> =
        serde_json::from_str(r#"{"roots":["."],"vars":{"a":"b"}}"#);
    assert!(parsed.is_err(), "extra key must fail");
    let parsed: Result<StacksConfig, _> =
        serde_json::from_str(r#"{"ignore":[],"tofu":{"roots":[]}}"#);
    assert!(parsed.is_ok(), "tofu table parses");
}

#[test]
fn missing_roots_key_rejected() {
    let parsed: Result<TofuStackConfig, _> = serde_json::from_str("{}");
    assert!(parsed.is_err(), "roots is required");
}

#[test]
fn absent_tofu_table_stays_valid_and_schema_stable() {
    assert_eq!(VelnorConfig::SCHEMA, 1);
    let stacks = StacksConfig {
        ignore: Vec::new(),
        rust: None,
        tofu: None,
    };
    assert!(stacks.validate("config.toml").is_ok());
    let rendered = serde_json::to_string(&stacks).expect("serializes");
    assert!(!rendered.contains("tofu"), "additive only: {rendered}");
}

#[test]
fn unit_prefix_folds_dot_only() {
    assert_eq!(Utf8RepoRelDir::from_raw(".".to_owned()).unit_prefix(), "");
    assert_eq!(
        Utf8RepoRelDir::from_raw("envs/prod".to_owned()).unit_prefix(),
        "envs/prod"
    );
}

#[test]
fn reserved_root_spelling_rejected() {
    for raw in [&["root"][..], &[".", "root"][..]] {
        let err = tofu(raw).validate("config.toml").expect_err("root fails");
        assert!(
            err.to_string().contains("reserved_root_key"),
            "{raw:?}: {err}"
        );
    }
    assert!(Utf8RepoRelDir::parse("root").is_err());
    assert!(Utf8RepoRelDir::parse("roots").is_ok());
    assert!(Utf8RepoRelDir::parse("a/root").is_ok());
}

#[test]
fn tofu_is_a_registered_ignorable_stack() {
    assert!(VelnorConfig::REGISTERED_STACKS.contains(&"tofu"));
    let stacks = StacksConfig {
        ignore: vec!["tofu".to_owned()],
        rust: None,
        tofu: None,
    };
    assert!(stacks.validate("config.toml").is_ok());
    let stacks = StacksConfig {
        ignore: vec!["cobol".to_owned()],
        rust: None,
        tofu: None,
    };
    let err = stacks.validate("config.toml").expect_err("bogus id fails");
    assert!(err.to_string().contains("unknown_stack_id"), "{err}");
}

#[test]
fn tofu_records_ignore_with_registered_reason() {
    let projects = vec![
        DetectedProject {
            stack_id: "tofu".to_owned(),
            project_root: String::new(),
            manifest: "main.tf".to_owned(),
        },
        DetectedProject {
            stack_id: "rust".to_owned(),
            project_root: String::new(),
            manifest: "Cargo.toml".to_owned(),
        },
    ];
    let statuses = apply_stack_ignores(projects, &["tofu".to_owned()]);
    assert_eq!(statuses.len(), 2);
    assert!(matches!(
        &statuses[0],
        DetectionStatus::Ignored { reason, .. } if reason == IGNORED_REASON
    ));
    assert!(matches!(statuses[1], DetectionStatus::Selected(_)));
    let selected = selected_projects(&statuses);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].stack_id, "rust");
}
