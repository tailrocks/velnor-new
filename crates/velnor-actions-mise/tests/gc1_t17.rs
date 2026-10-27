// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_mise::compiler_observer::{
    CompilerInvocation, OPERATION_COMPILATION, ROLE_APPLICATION, ROLE_DEPENDENCY,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn rustc_argv() -> Vec<String> {
    [
        "rustc",
        "--crate-name",
        "serde",
        "--edition=2021",
        "src/lib.rs",
        "--out-dir",
        "target/debug/deps",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn dependency_invocation(event_id: &str) -> CompilerInvocation {
    CompilerInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "recipe-build-deps".to_owned(),
        package_id: "serde".to_owned(),
        dependency_role: ROLE_DEPENDENCY.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: "rustc".to_owned(),
        executable: "/tools/rust/prefix/bin/rustc".to_owned(),
        argv: rustc_argv(),
        wrapper_chain: vec!["/opt/observer/rustc-observer".to_owned()],
        operation_kind: OPERATION_COMPILATION.to_owned(),
        doc_phase: None,
        cwd: "/work/checkout".to_owned(),
    }
}

fn rustdoc_invocation(event_id: &str, phase: &str, argv: &[&str]) -> CompilerInvocation {
    CompilerInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "recipe-doc".to_owned(),
        package_id: "velnor-actions-mise".to_owned(),
        dependency_role: ROLE_APPLICATION.to_owned(),
        lane_id: "doc".to_owned(),
        tool_kind: "rustdoc".to_owned(),
        executable: "/tools/rust/prefix/bin/rustdoc".to_owned(),
        argv: argv.iter().map(|arg| (*arg).to_owned()).collect(),
        wrapper_chain: vec!["/opt/observer/rustdoc-observer".to_owned()],
        operation_kind: OPERATION_COMPILATION.to_owned(),
        doc_phase: Some(phase.to_owned()),
        cwd: "/work/checkout".to_owned(),
    }
}

mod preserves;
mod rejects;
