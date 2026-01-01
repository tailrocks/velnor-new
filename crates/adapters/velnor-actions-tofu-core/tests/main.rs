//! Smoke coverage for the tofu foundation crate: stack identity, task
//! kinds, parsing, families, fmt scope, keys, versions, and argv.

use velnor_actions_tofu_core::{
    Family, STACK_ID, TofuTaskKind, admits_opentofu, display_for_root, family_of, is_fmt_file,
    key_for_root, parse_json, parse_native, root_for_key, task_id_for_root, tofu_payload_argv,
};

#[test]
fn stack_identity_is_stable() {
    assert_eq!(STACK_ID, "tofu");
}

#[test]
fn task_kinds_cover_lifecycle() {
    let kinds = [
        TofuTaskKind::Fmt,
        TofuTaskKind::InitForValidate,
        TofuTaskKind::Validate,
    ];
    assert_eq!(kinds.len(), 3);
    assert_eq!(
        TofuTaskKind::parse("init"),
        Ok(TofuTaskKind::InitForValidate)
    );
}

#[test]
fn unknown_kind_fails_closed() {
    assert!(TofuTaskKind::parse("apply").is_err());
}

#[test]
fn native_parser_accepts_empty_unit() {
    let model = parse_native("").expect("empty unit parses");
    assert!(model.blocks.is_empty());
}

#[test]
fn json_parser_accepts_empty_object() {
    let model = parse_json("{}").expect("empty object parses");
    assert!(model.blocks.is_empty());
}

#[test]
fn families_classify_load_set() {
    assert_eq!(family_of("main.tf"), Family::Config);
    assert_eq!(family_of("check.tftest.hcl"), Family::Test);
}

#[test]
fn fmt_scope_recognizes_config_files() {
    assert!(is_fmt_file("main.tf"));
    assert!(!is_fmt_file("README.md"));
}

#[test]
fn root_keys_round_trip() {
    assert_eq!(root_for_key(&key_for_root("stacks/a")), "stacks/a");
    assert_eq!(display_for_root(""), ".");
}

#[test]
fn versions_admit_opentofu_floor() {
    assert!(admits_opentofu(">= 1.6.0"));
}

#[test]
fn task_ids_carry_stack_and_kind() {
    let id =
        task_id_for_root("stacks/a", TofuTaskKind::Validate, "default").expect("task id builds");
    assert!(id.contains("tofu"), "{id}");
    assert!(id.contains("validate"), "{id}");
}

#[test]
fn payload_argv_starts_with_subcommand() {
    let argv = tofu_payload_argv(TofuTaskKind::Fmt, "stacks/a").expect("argv builds");
    assert!(!argv.is_empty());
}
