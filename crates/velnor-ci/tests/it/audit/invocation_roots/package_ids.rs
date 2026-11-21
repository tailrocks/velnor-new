// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;

use super::{WORKSPACE_TARGET, placed, unit, workspace_output};

const REFUSAL: &str = "outside this command's explicit selection";

const ROOT: &str = "@ROOT@";

const FORMS: [(&str, &str, bool); 24] = [
    ("x", "x", true),
    ("beta", "beta", true),
    ("x@0.1.0", "x", true),
    ("beta@0.1.0", "beta", true),
    ("x:0.1.0", "x", true),
    ("beta:0.1.0", "beta", true),
    ("path+file://@ROOT@/x", "x", true),
    ("file://@ROOT@/x", "x", true),
    ("path+file://@ROOT@/x#0.1.0", "x", true),
    ("file://@ROOT@/x#0.1.0", "x", true),
    ("path+file://@ROOT@/x#x", "x", true),
    ("path+file://@ROOT@/x#x@0.1.0", "x", true),
    ("path+file://@ROOT@/pkg-beta#beta", "beta", true),
    ("file://@ROOT@/pkg-beta#beta", "beta", true),
    ("path+file://@ROOT@/pkg-beta#beta@0.1.0", "beta", true),
    ("file://@ROOT@/pkg-beta#beta@0.1.0", "beta", true),
    ("path+file://@ROOT@/pkg-beta#0.1.0", "beta", false),
    (
        "registry+https://github.com/rust-lang/crates.io-index#x@0.1.0",
        "x",
        false,
    ),
    ("git+https://example.invalid/x#x@0.1.0", "x", false),
    ("x@9.9.9", "x", false),
    ("x:9.9.9", "x", false),
    ("path+file://@ROOT@/x#9.9.9", "x", false),
    ("path+file://@ROOT@/x#x@9.9.9", "x", false),
    ("path+file://@ROOT@/pkg-beta#beta@9.9.9", "beta", false),
];

fn workspace_with_beta() -> Repo {
    let repo = Repo::new("audit-invocation-package-ids");
    unit(&repo, ".gitignore", "target/\ntarget-velnor/\n");
    unit(&repo, "mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    unit(
        &repo,
        "Cargo.toml",
        "[workspace]\nmembers = [\"x\", \"pkg-beta\"]\nresolver = \"3\"\n",
    );
    unit(&repo, "Cargo.lock", "version = 4\n");
    for (directory, package) in [("x", "x"), ("pkg-beta", "beta")] {
        unit(
            &repo,
            &format!("{directory}/Cargo.toml"),
            &format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        unit(&repo, &format!("{directory}/src/lib.rs"), "");
    }
    repo.git(&["add", "--all"]);
    repo
}

fn untrusted_output_of(repo: &Repo, package: &str) {
    placed(repo, &format!("{WORKSPACE_TARGET}/debug"));
    unit(
        repo,
        &format!("{WORKSPACE_TARGET}/debug/build/{package}-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
}

fn spec_in(repo: &Repo, form: &str) -> String {
    form.replace(ROOT, &repo.root().display().to_string())
}

fn refused(stderr: &str, package: &str) -> bool {
    stderr.contains(&format!("{REFUSAL}: {package} (build-script output)"))
}

#[test]
fn every_package_id_form_selects_its_member_under_package_flag() {
    let mut wrong = Vec::new();
    for (form, package, selects) in FORMS {
        let repo = workspace_with_beta();
        untrusted_output_of(&repo, package);
        let spec = spec_in(&repo, form);
        let stderr = workspace_output(&repo, &["cargo", "build", "-p", &spec, "--no-such-flag"]);
        let right = if selects {
            !stderr.contains(REFUSAL) && stderr.contains("build command failed")
        } else {
            refused(&stderr, package)
        };
        if !right {
            wrong.push(spec);
        }
    }
    assert!(
        wrong.is_empty(),
        "-p does not select as expected: {wrong:#?}"
    );
}

#[test]
fn every_package_id_form_excludes_its_member_under_exclude_flag() {
    let mut wrong = Vec::new();
    for (form, package, excludes) in FORMS {
        let repo = workspace_with_beta();
        untrusted_output_of(&repo, package);
        let spec = spec_in(&repo, form);
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
        let right = if excludes {
            refused(&stderr, package)
        } else {
            !stderr.contains(REFUSAL) && stderr.contains("build command failed")
        };
        if !right {
            wrong.push(spec);
        }
    }
    assert!(
        wrong.is_empty(),
        "--exclude does not exclude as expected: {wrong:#?}"
    );
}
