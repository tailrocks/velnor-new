// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    WORKSPACE_TARGET, failed_build_in_tools_a_with_untrusted_b_output, placed,
    stderr_of_tools_b_build, unit, workspace_output, workspace_with_members,
};

#[test]
fn every_package_id_spelling_of_a_member_selects_it_in_another_roots_build() {
    let forms: [fn(&str) -> String; 6] = [
        |_| "b:0.1.0".to_owned(),
        |url| format!("path+file://{url}#0.1.0"),
        |url| format!("file://{url}#0.1.0"),
        |url| format!("path+file://{url}#b"),
        |url| format!("path+file://{url}#b@0.1.0"),
        |url| format!("path+file://{url}"),
    ];
    for form in forms {
        let repo = failed_build_in_tools_a_with_untrusted_b_output(&[
            "cargo",
            "build",
            "-p",
            "a",
            "-p",
            "b",
            "--no-such-flag",
        ]);
        let url = repo.root().join("tools/b").display().to_string();
        let spec = form(&url);
        let stderr =
            stderr_of_tools_b_build(&repo, &["cargo", "build", "-p", &spec, "--no-such-flag"]);
        assert!(
            !stderr.contains("outside this command's explicit selection"),
            "{spec}: {stderr}"
        );
        assert!(stderr.contains("build command failed"), "{spec}: {stderr}");
    }
}

#[test]
fn every_package_id_spelling_of_an_excluded_member_takes_it_out_of_the_selection() {
    let forms: [fn(&str) -> String; 5] = [
        |url| format!("path+file://{url}#0.1.0"),
        |url| format!("file://{url}#0.1.0"),
        |url| format!("path+file://{url}#x"),
        |url| format!("path+file://{url}#x@0.1.0"),
        |url| format!("path+file://{url}"),
    ];
    for form in forms {
        let repo = workspace_with_members();
        placed(&repo, &format!("{WORKSPACE_TARGET}/debug"));
        unit(
            &repo,
            &format!("{WORKSPACE_TARGET}/debug/build/x-f000000000000005/output"),
            "cargo:rerun-if-changed=src/lib.rs\n",
        );
        let url = repo.root().join("x").display().to_string();
        let spec = form(&url);
        let stderr = workspace_output(
            &repo,
            &[
                "cargo",
                "build",
                "--workspace",
                "--exclude",
                &spec,
                "--no-such-flag",
            ],
        );
        assert!(
            stderr.contains("outside this command's explicit selection: x"),
            "{spec}: {stderr}"
        );
    }
}

#[test]
fn a_registry_or_git_exclude_spec_does_not_exclude_a_same_named_member() {
    for spec in [
        "registry+https://github.com/rust-lang/crates.io-index#x@0.1.0",
        "git+https://example.invalid/x#x@0.1.0",
    ] {
        let repo = workspace_with_members();
        placed(&repo, &format!("{WORKSPACE_TARGET}/debug"));
        unit(
            &repo,
            &format!("{WORKSPACE_TARGET}/debug/build/x-f000000000000005/output"),
            "cargo:rerun-if-changed=src/lib.rs\n",
        );
        let stderr = workspace_output(
            &repo,
            &[
                "cargo",
                "build",
                "--workspace",
                "--exclude",
                spec,
                "--no-such-flag",
            ],
        );
        assert!(
            !stderr.contains("outside this command's explicit selection"),
            "{spec}: {stderr}"
        );
        assert!(stderr.contains("build command failed"), "{spec}: {stderr}");
    }
}
