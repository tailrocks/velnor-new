// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_rust::{dep_info_paths, rerun_if_changed};

mod embed;
mod lock;

#[test]
fn dep_info_and_build_script_output_become_the_paths_they_list() {
    let dep_info = "/w/target/debug/deps/app-0123456789abcdef.d: app/src/main.rs app/src/my\\ data.txt /r/serde/src/lib.rs\n\
                    \n\
                    /w/target/debug/deps/libapp-0123456789abcdef.rmeta: app/src/main.rs app/src/my\\ data.txt /r/serde/src/lib.rs app/src/../../shared/a.txt\n\
                    \n\
                    app/src/main.rs:\n\
                    app/src/my\\ data.txt:\n\
                    \n\
                    # env-dep:CARGO_PKG_NAME=app\n";
    assert_eq!(
        dep_info_paths(dep_info),
        [
            "app/src/main.rs",
            "app/src/my data.txt",
            "/r/serde/src/lib.rs",
            "app/src/../../shared/a.txt",
        ]
    );
    let output = "cargo:rerun-if-changed=build.rs\n\
                  cargo:rustc-env=LAB=lib\n\
                  cargo::rerun-if-changed=../shared/a b.txt\n\
                  cargo:rerun-if-changed=\n\
                  cargo:warning=cargo:rerun-if-changed=x\n";
    assert_eq!(rerun_if_changed(output), ["build.rs", "../shared/a b.txt"]);
}
