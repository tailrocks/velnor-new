use super::direct_tool_policy_violation;

#[test]
fn process_policy_accepts_exit_code_in_every_import_form() {
    for source in [
        "use std::process::ExitCode;\nfn main() -> ExitCode { ExitCode::SUCCESS }\n",
        "use std::{io, process::ExitCode};\nfn f() -> ExitCode { ExitCode::FAILURE }\n",
        "fn f() -> std::process::ExitCode { std::process::ExitCode::SUCCESS }\n",
        "use crate::args::Command;\nfn f(c: Command) { match c { Command::Init => {} } }\n",
    ] {
        assert_eq!(direct_tool_policy_violation(source), None, "{source}");
    }
}

#[test]
fn process_policy_rejects_hidden_process_launches() {
    for source in [
        // The scanner checks text tokens.
        "fn f() { let _ = std::process::Command::new(\"x\"); }\n",
        // The tests cover grouped and renamed imports.
        "use std::{process as p};\ntype R = p::Command;\nfn f(c: &mut R) { let _ = R::output(c); }\n",
        "use std::{process::{self as p}};\ntype R = p::Command;\n",
        "use std::{process::{Command as Run}};\n",
        "use std::{process};\ntype R = process::Command;\n",
        "use std::{process::*};\n",
        // The tests cover namespace aliases and shadowing.
        "use std as s;\ntype R = s::process::Command;\n",
        "use std::{self as s};\n",
        "use ::std as s;\n",
        "use std::*;\n",
        "extern crate std as s;\n",
        "mod process {}\n",
        "use r#std::r#process as p;\n",
        "fn f() { let _ = ::std::process::abort; }\n",
        "fn f() { let _ = <std::process::Child>::id; }\n",
        "use std::os::unix::process::CommandExt;\n",
        // The checker rejects source that it cannot parse.
        "fn f( {\n",
    ] {
        assert!(
            direct_tool_policy_violation(source).is_some(),
            "The checker accepted this source: {source}."
        );
    }
}

#[test]
fn process_policy_accepts_cache_metadata_and_proof_methods() {
    let source = r#"
        struct CacheRoots { cargo_home: PathBuf, mbx_cache_dir: PathBuf, rustup_home: PathBuf }
        fn inspect(proof: CacheRestoreProof) {
            proof.output();
            proof.spawn();
            let _ = "cargo";
        }
    "#;
    assert_eq!(direct_tool_policy_violation(source), None);
}

#[test]
fn process_policy_rejects_a_command_inside_macro_tokens() {
    let source = r#"macro_rules! launch { () => { std::process::Command::new("x") }; }"#;
    assert!(direct_tool_policy_violation(source).is_some());
}

#[test]
fn process_policy_rejects_spaced_nested_and_aliased_macro_paths() {
    for source in [
        r#"macro_rules! launch { () => { std :: process :: Command :: new("x").status() }; }"#,
        r#"macro_rules! launch { () => { { (:: r#std :: r#process :: Command :: new("x").output()) } }; }"#,
        r#"macro_rules! launch { () => { use std as s; s :: process :: Command :: new("x") }; }"#,
        r#"macro_rules! launch { () => { use std :: process as p; p :: Command :: new("x") }; }"#,
        r#"macro_rules! launch { ($path:path) => { $path :: Command :: new("x") }; } launch!(std :: process);"#,
    ] {
        assert!(
            direct_tool_policy_violation(source).is_some(),
            "The checker accepted this macro source: {source}."
        );
    }
}

#[test]
fn process_policy_allows_macro_metadata_and_proof_arguments() {
    let source = r#"
        macro_rules! metadata { () => { (std :: process :: id(), :: std :: process :: ExitCode :: SUCCESS) }; }
        macro_rules! proof { ($proof:expr) => { ($proof.status(), $proof.output(), $proof.spawn()) }; }
        fn text() { let _ = "std :: process :: Command :: new"; }
    "#;
    assert_eq!(direct_tool_policy_violation(source), None);
}
