// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;

use super::{FLAT, PINNED, decision, merge, plan, remove, world, world_over};
use crate::io::Repo;
use crate::io_plan::plan_command;
use crate::plan_api::event;

type Edit = fn(&Repo);

fn run_of(name: &str, edit: impl FnOnce(&Repo)) -> String {
    let repo = world(name, FLAT, PINNED);
    merge(&repo, edit);
    decision(&repo)
}

fn link(repo: &Repo, target: &str, at: &str) {
    let path = repo.root().join(at);
    let _ = fs::remove_file(&path);
    symlink(target, path).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn an_in_place_edit_of_a_plain_markdown_or_rust_file_skips_the_generated_check() {
    assert_eq!(
        run_of("gc-skip-md", |repo| {
            repo.write("docs/goals.md", "goals\nmore\n");
            repo.write("README.md", "readme\nmore\n");
        }),
        "skip"
    );
    assert_eq!(
        run_of("gc-skip-rs", |repo| {
            repo.write("crates/other/src/lib.rs", "pub fn o() -> u32 { 2 }\n");
        }),
        "skip"
    );
}

#[test]
fn a_structural_change_to_markdown_or_rust_runs_the_generated_check() {
    let cases: [(&str, Edit); 7] = [
        ("add", |repo| repo.write("docs/new.md", "new\n")),
        ("delete", |repo| remove(repo, "docs/goals.md")),
        ("rename", |repo| {
            repo.git(&["mv", "docs/goals.md", "docs/renamed.md"]);
        }),
        ("rename-rs", |repo| {
            repo.git(&["mv", "crates/other/src/old.rs", "crates/other/src/new.rs"]);
        }),
        ("mode", |repo| {
            fs::set_permissions(
                repo.root().join("docs/goals.md"),
                std::os::unix::fs::PermissionsExt::from_mode(0o755),
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
        }),
        ("type", |repo| {
            remove(repo, "docs/goals.md");
            link(repo, "../README.md", "docs/goals.md");
        }),
        ("retarget", |repo| {
            link(repo, "../README.md", "docs/pointer.md");
        }),
    ];
    let mut skipped = Vec::new();
    for (name, edit) in cases {
        let repo = world_over(&format!("gc-struct-{name}"), FLAT, PINNED, |repo| {
            link(repo, "goals.md", "docs/pointer.md");
        });
        merge(&repo, edit);
        if decision(&repo) != "run" {
            skipped.push(name);
        }
    }
    assert!(
        skipped.is_empty(),
        "structural changes that skipped: {skipped:?}"
    );
}

#[test]
fn a_rename_across_packages_is_read_as_a_delete_and_an_add() {
    let repo = world("gc-rename-packages", FLAT, PINNED);
    merge(&repo, |repo| {
        repo.git(&[
            "mv",
            "crates/other/src/old.rs",
            "crates/engine/src/moved.rs",
        ]);
    });
    let (output, values) = plan(&repo);
    assert_eq!(values["lint_generated_check"], "run");
    let explained = String::from_utf8_lossy(&output.stdout);
    assert!(explained.contains("most-packages:3"), "{explained}");
    assert!(
        explained.contains("other changed:crates/other/src/old.rs"),
        "{explained}"
    );
    assert!(
        explained.contains("engine changed:crates/engine/src/moved.rs"),
        "{explained}"
    );
}

#[test]
fn an_include_added_or_removed_in_a_non_closure_package_runs_a_plain_edit_skips() {
    let mut wrong = Vec::new();
    let added = run_of("gc-include-add", |repo| {
        repo.write(
            "crates/other/src/lib.rs",
            "pub fn o() -> &'static str { include_str!(\"../../../docs/goals.md\") }\n",
        );
    });
    if added != "run" {
        wrong.push("include added");
    }
    let removes = world_over("gc-include-remove", FLAT, PINNED, |repo| {
        repo.write_unstaged(
            "crates/other/src/lib.rs",
            "pub fn o() -> &'static str { include_str!(\"../../../docs/goals.md\") }\n",
        );
    });
    merge(&removes, |repo| {
        repo.write("crates/other/src/lib.rs", "pub fn o() -> u32 { 1 }\n");
    });
    if decision(&removes) != "run" {
        wrong.push("include removed");
    }
    let plain = run_of("gc-include-plain", |repo| {
        repo.write("crates/other/src/lib.rs", "pub fn o() -> u32 { 3 }\n");
    });
    if plain != "skip" {
        wrong.push("plain edit");
    }
    assert!(wrong.is_empty(), "wrong decisions: {wrong:?}");
}

#[test]
fn an_explicit_changed_list_runs_the_generated_check() {
    let repo = world("gc-changed", FLAT, PINNED);
    let outputs = repo.root().join("lint-output");
    for (changed, expected) in [
        ("docs/goals.md", "run"),
        ("crates/engine/src/lib.rs", "run"),
    ] {
        fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
        let output = plan_command(&repo)
            .args(["plan", "--changed", changed])
            .env("GITHUB_EVENT_NAME", "pull_request")
            .env("GITHUB_EVENT_PATH", event(&repo, "main"))
            .env("GITHUB_OUTPUT", &outputs)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        let written = fs::read_to_string(&outputs).unwrap_or_default();
        assert!(
            written.contains(&format!("lint_generated_check={expected}")),
            "{changed}: {written}"
        );
    }
}

fn many(name: &str, count: usize) -> String {
    let repo = world_over(name, FLAT, PINNED, |repo| {
        for index in 0..count {
            repo.write_unstaged(
                &format!("crates/other/src/bulk/f{index}.rs"),
                "pub fn f() {}\n",
            );
        }
    });
    merge(&repo, |repo| {
        for index in 0..count {
            repo.write_unstaged(
                &format!("crates/other/src/bulk/f{index}.rs"),
                "pub fn f() -> u32 { 1 }\n",
            );
        }
    });
    decision(&repo)
}

#[test]
fn in_place_rust_edits_up_to_the_read_limit_are_read_and_skip() {
    assert_eq!(many("gc-bulk-limit", 256), "skip");
}

#[test]
fn more_than_the_read_limit_of_in_place_rust_edits_runs_the_generated_check() {
    assert_eq!(many("gc-bulk-over", 257), "run");
}

#[test]
fn an_in_place_edit_inside_a_vendored_source_runs_the_generated_check() {
    let vendored = |repo: &Repo| {
        repo.write_unstaged(
            ".cargo/config.toml",
            "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"vendor\"\n",
        );
        repo.write_unstaged("vendor/foo/src/lib.rs", "pub fn foo() -> u32 { 1 }\n");
    };
    let repo = world_over("gc-vendored", FLAT, PINNED, vendored);
    merge(&repo, |repo| {
        repo.write("vendor/foo/src/lib.rs", "pub fn foo() -> u32 { 2 }\n");
    });
    assert_eq!(decision(&repo), "run");
    let control = world_over("gc-vendored-control", FLAT, PINNED, |repo| {
        repo.write_unstaged("vendor/foo/src/lib.rs", "pub fn foo() -> u32 { 1 }\n");
    });
    merge(&control, |repo| {
        repo.write("vendor/foo/src/lib.rs", "pub fn foo() -> u32 { 2 }\n");
    });
    assert_eq!(decision(&control), "skip");
}

#[test]
fn an_in_place_edit_of_a_program_the_cargo_config_names_runs_the_generated_check() {
    let configs = [
        ("wrapper", "[build]\nrustc-wrapper = \"tools/wrap.rs\"\n"),
        (
            "workspace-wrapper",
            "[build]\nrustc-workspace-wrapper = \"tools/wrap.rs\"\n",
        ),
        (
            "runner",
            "[target.'cfg(all())']\nrunner = \"tools/wrap.rs\"\n",
        ),
    ];
    for (name, config) in configs {
        let repo = world_over(&format!("gc-program-{name}"), FLAT, PINNED, |repo| {
            repo.write_unstaged(".cargo/config.toml", config);
            repo.write_unstaged("tools/wrap.rs", "#!/bin/sh\nexec \"$@\"\n");
        });
        merge(&repo, |repo| {
            repo.write("tools/wrap.rs", "#!/bin/sh\nexec \"$@\" --cfg probe\n");
        });
        assert_eq!(decision(&repo), "run", "{name}");
    }
    let control = world_over("gc-program-control", FLAT, PINNED, |repo| {
        repo.write_unstaged("tools/wrap.rs", "#!/bin/sh\nexec \"$@\"\n");
    });
    merge(&control, |repo| {
        repo.write("tools/wrap.rs", "#!/bin/sh\nexec \"$@\" --cfg probe\n");
    });
    assert_eq!(decision(&control), "skip");
}
