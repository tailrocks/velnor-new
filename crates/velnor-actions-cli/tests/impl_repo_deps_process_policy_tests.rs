use super::direct_tool_policy_violation;

#[test]
fn whitespace_qualified_command_type_alias_is_rejected() {
    let source = "type P = std :: process :: Command; fn launch(p: &mut P) { p.status(); }";
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn aliased_process_module_type_is_rejected() {
    let source = "use std :: process as proc; type P = proc :: Command;";
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn imported_process_module_type_is_rejected() {
    let source = "use std :: process; type P = process :: Command;";
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn chained_process_module_alias_is_rejected() {
    let source = "use std as sys; use sys::process as proc; type P = proc::Command;";
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn process_glob_import_is_rejected() {
    assert_eq!(
        direct_tool_policy_violation("use std::process::*;"),
        Some("std::process::*".to_owned())
    );
}

#[test]
fn qualified_qself_command_is_rejected() {
    let source = "fn call() { let _ = <std::process::Command as Trait>::run; }";
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn typed_proof_methods_and_process_metadata_remain_allowed() {
    let source = r"
            fn check(proof: CacheRestoreProof) { proof.status(); proof.output(); proof.spawn(); }
            fn metadata() { let _pid = std::process::id(); }
            fn exit() -> std::process::ExitCode { std::process::ExitCode::SUCCESS }
        ";
    assert_eq!(direct_tool_policy_violation(source), None);
}

#[test]
fn nested_module_process_alias_is_rejected() {
    let source = r#"mod inner { use std :: process as p; fn launch() { let _ = p :: Command :: new("cargo").status(); } }"#;
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn block_process_alias_is_rejected() {
    let source = r#"fn launch() { use std :: process as p; let _ = p :: Command :: new("cargo").status(); }"#;
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn nested_module_chained_process_alias_is_rejected() {
    let source = r#"mod inner { use std as sys; use sys :: process as p; fn launch() { let _ = p :: Command :: new("cargo").status(); } }"#;
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn process_command_import_is_rejected_before_renaming() {
    assert_eq!(
        direct_tool_policy_violation("use std :: process :: Command as C;"),
        Some("process::Command".to_owned())
    );
}

#[test]
fn process_command_reexport_alias_is_rejected() {
    let source =
        "use std :: process :: Command as C; use C as D; fn launch(p: &mut D) { p.status(); }";
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("process::Command".to_owned())
    );
}

#[test]
fn process_metadata_imports_are_allowed() {
    let source = "use std :: process :: {id, ExitCode}; fn metadata() { let _pid = id(); } fn exit() -> ExitCode { ExitCode::SUCCESS }";
    assert_eq!(direct_tool_policy_violation(source), None);
}

#[test]
fn other_process_member_import_is_rejected() {
    assert_eq!(
        direct_tool_policy_violation("use std :: process :: Stdio;"),
        Some("process::Stdio".to_owned())
    );
}
