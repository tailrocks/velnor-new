use super::direct_tool_policy_violation;

fn rejected(source: &str) {
    assert!(direct_tool_policy_violation(source).is_some(), "{source}");
}

#[test]
fn canonical_process_command_and_members_are_rejected() {
    for source in [
        "fn launch() { std::process::Command::new(\"sh\"); }",
        "use std::process::Stdio;",
        "use std::process::Command;",
        "fn launch() { let _ = ::std::process::Command::new(\"sh\"); }",
    ] {
        rejected(source);
    }
}

#[test]
fn process_namespace_imports_and_reexports_are_rejected() {
    for source in [
        "use std::process;",
        "use std::process as p;",
        "pub use std::process as p;",
        "use std::process::*;",
        "use std::*;",
        "use std::process::{self};",
        "fn run() { use std::process as p; }",
        "mod inner { pub use std::process as p; }",
        "use std::process::Command as C;",
        "use std::process::ExitCode as E;",
        "use std::process::id as process_id;",
    ] {
        rejected(source);
    }
}

#[test]
fn standard_namespace_aliases_and_crate_aliases_are_rejected() {
    for source in [
        "use std as s;",
        "use ::std as s;",
        "use std::{self as s};",
        "use r#std as s;",
        "extern crate std as s;",
        "extern crate std;",
        "extern crate core as std;",
        "use crate::other as std;",
        "use ::std;",
        "pub use ::std;",
        "use ::{std};",
        "pub use ::{std as standard};",
        "use std::{self};",
        "fn f() { use ::std; }",
        "use crate::other as process;",
        "fn f() { use std as s; }",
    ] {
        rejected(source);
    }
}

#[test]
fn standard_crate_reexports_cannot_hide_process_access() {
    let source = "pub mod wrapper { pub use ::std; } use wrapper::std::process as p; use p::Command as C; pub fn launch() { let _ = C::new(\"sh\").status(); }";
    assert_eq!(
        direct_tool_policy_violation(source),
        Some("std namespace alias".to_owned())
    );
    rejected(
        "mod wrapper { pub use ::{std}; } use wrapper::std::process as p; fn launch() { let _ = p::Command::new(\"sh\"); }",
    );
}

#[test]
fn reserved_namespace_modules_are_rejected() {
    rejected("mod std { pub mod process {} }");
    rejected("mod process {}");
    rejected("mod unrelated { mod std { pub mod process {} } }");
}

#[test]
fn process_reexports_through_crate_self_and_super_are_rejected() {
    for source in [
        "pub use std::process as p; pub fn launch() { let _ = crate::p::Command::new(\"sh\").status(); }",
        "pub use std::process as p; pub mod inner { pub fn launch() { let _ = self::p::Command::new(\"sh\").status(); } }",
        "pub use std::process as p; mod inner { use super::p as q; pub fn launch() { let _ = q::Command::new(\"sh\").status(); } }",
    ] {
        rejected(source);
    }
}

#[test]
fn raw_and_absolute_process_paths_are_rejected() {
    rejected("use r#std::process as p; fn launch() { let _ = p::Command::new(\"sh\"); }");
    rejected("fn launch() { let _ = ::std::process::Command::new(\"sh\"); }");
    rejected(
        "mod std { pub mod process { pub struct Command; } } fn launch() { let _ = ::std::process::Command; }",
    );
}

#[test]
fn metadata_paths_and_unrenamed_leaf_imports_are_allowed() {
    let source = r#"
        use std::process::{id, ExitCode};
        use std::env as env;
        use std::fs as fs;
        use std::io as io;
        use std::path::Path as FilePath;
        fn metadata() -> ExitCode {
            let _pid = id();
            let _cwd = env::current_dir();
            let _ = fs::metadata(".");
            let _ = io::ErrorKind::NotFound;
            let _path: Option<FilePath> = None;
            std::process::ExitCode::SUCCESS
        }
        fn direct_metadata() { let _pid = std::process::id(); }
    "#;
    assert_eq!(direct_tool_policy_violation(source), None);
}

#[test]
fn canonical_metadata_reference_forms_remain_allowed() {
    let forms = [
        "fn f() { let _ = std::process::id(); }",
        "fn f() -> std::process::ExitCode { std::process::ExitCode::SUCCESS }",
        "use std::process::id;",
        "use std::process::ExitCode;",
        "use std::process::{id, ExitCode};",
        "use std::process::{ExitCode, id};",
    ];
    for source in forms {
        assert_eq!(direct_tool_policy_violation(source), None, "{source}");
    }
}

#[test]
fn qualified_qself_command_is_rejected() {
    assert_eq!(
        direct_tool_policy_violation(
            "fn call() { let _ = <std::process::Command as Trait>::run; }"
        ),
        Some("process::Command".to_owned())
    );
}

#[test]
fn prior_process_policy_rejections_remain_covered() {
    for source in [
        "type P = std :: process :: Command; fn launch(p: &mut P) { p.status(); }",
        "use std :: process as proc; type P = proc :: Command;",
        "use std :: process; type P = process :: Command;",
        "use std as sys; use sys::process as proc; type P = proc::Command;",
        "use std::process::*;",
        "use std::process::Stdio;",
        "extern crate std as s; pub fn launch() { let _ = s::process::Command::new(\"sh\").status(); }",
        "use r#std as s; use s::process as p; pub fn launch() { let _ = p::Command::new(\"sh\").status(); }",
        "use std::process as p; mod x { use super::p as q; pub fn launch() { let _ = q::Command::new(\"sh\").status(); } }",
        "mod inner { use std :: process as p; fn launch() { let _ = p :: Command :: new(\"cargo\").status(); } }",
        "fn launch() { use std :: process as p; let _ = p :: Command :: new(\"cargo\").status(); }",
        "mod inner { use std as sys; use sys :: process as p; fn launch() { let _ = p :: Command :: new(\"cargo\").status(); } }",
        "use std :: process :: Command as C;",
        "use std :: process :: Command as C; use C as D; fn launch(p: &mut D) { p.status(); }",
    ] {
        rejected(source);
    }
}

#[test]
fn proof_methods_and_unrelated_command_names_remain_allowed() {
    let source = r#"
        struct Command;
        impl Command { fn new(_: &str) -> Self { Self } }
        fn check(proof: CacheRestoreProof) {
            proof.status();
            proof.output();
            proof.spawn();
            let _ = Command;
            let _ = Command::new("local");
        }
        mod unrelated { pub struct Command; }
        use crate::unrelated as p;
        fn inspect() { let _ = p::Command; }
    "#;
    assert_eq!(direct_tool_policy_violation(source), None);
}

#[test]
fn unrelated_scoped_module_alias_does_not_create_process_violation() {
    let source = "mod unrelated { pub struct Command; } use crate::unrelated as p; fn inspect() { let _ = p::Command; }";
    assert_eq!(direct_tool_policy_violation(source), None);
}

#[test]
fn malformed_rust_is_rejected() {
    assert_eq!(
        direct_tool_policy_violation("fn launch( {"),
        Some("Rust syntax parse failed".to_owned())
    );
}
