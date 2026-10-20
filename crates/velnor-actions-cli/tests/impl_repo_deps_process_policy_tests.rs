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
        // Text tokens.
        "fn f() { let _ = std::process::Command::new(\"x\"); }\n",
        "fn f(c: &mut C) { let _ = c.output(); }\n",
        "fn f() { let _ = \"cargo\"; }\n",
        "fn f() { let _ = std::process::id(); }\n",
        // Regrouped and renamed imports.
        "use std::{process as p};\ntype R = p::Command;\nfn f(c: &mut R) { let _ = R::output(c); }\n",
        "use std::{process::{self as p}};\ntype R = p::Command;\n",
        "use std::{process::{Command as Run}};\n",
        "use std::{process};\ntype R = process::Command;\n",
        "use std::{process::*};\n",
        // Namespace aliases and shadowing.
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
        // Unparseable source fails closed.
        "fn f( {\n",
    ] {
        assert!(
            direct_tool_policy_violation(source).is_some(),
            "accepted: {source}"
        );
    }
}
