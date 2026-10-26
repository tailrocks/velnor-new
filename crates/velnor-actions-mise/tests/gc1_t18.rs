// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_mise::compiler_observer::{
    CompilerInvocation, OPERATION_COMPILATION, ROLE_APPLICATION, ROLE_DEPENDENCY,
};
use velnor_actions_mise::native_compiler_observer::NativeInvocation;

const CC_ADAPTER: &str = "/opt/velnor-observer/native-cc";
const CXX_ADAPTER: &str = "/opt/velnor-observer/native-cxx";
const PREVIOUS_CC: &str = "/usr/bin/cc";
const PREVIOUS_CXX: &str = "/usr/bin/c++";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message.to_owned()).into()
}

fn native_argv() -> Vec<String> {
    [
        "cc",
        "-O2",
        "-DFOO=bar baz",
        "-c",
        "src/native.c",
        "-o",
        "out/native.o",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn native_invocation(event_id: &str, tool_kind: &str, executable: &str) -> NativeInvocation {
    NativeInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "recipe-native-deps".to_owned(),
        package_id: "libnative".to_owned(),
        dependency_role: ROLE_DEPENDENCY.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: tool_kind.to_owned(),
        executable: executable.to_owned(),
        argv: native_argv(),
        adapter_chain: vec![executable.to_owned()],
        parent_event_id: None,
        operation_kind: OPERATION_COMPILATION.to_owned(),
        cwd: "/work/checkout".to_owned(),
    }
}

fn parent_rustc_invocation() -> CompilerInvocation {
    CompilerInvocation {
        event_id: "event-parent-rustc".to_owned(),
        recipe_id: "recipe-build-deps".to_owned(),
        package_id: "app".to_owned(),
        dependency_role: ROLE_APPLICATION.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: "rustc".to_owned(),
        executable: "/tools/rust/prefix/bin/rustc".to_owned(),
        argv: [
            "rustc",
            "--crate-name",
            "app",
            "--edition=2021",
            "src/main.rs",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        wrapper_chain: vec!["/opt/observer/rustc-observer".to_owned()],
        operation_kind: OPERATION_COMPILATION.to_owned(),
        doc_phase: None,
        cwd: "/work/checkout".to_owned(),
    }
}

mod preserves;
mod rejects;
