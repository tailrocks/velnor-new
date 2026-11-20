// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{
    APP, Root, TOOLS, TWO_BINARIES, jobs, render, steps, two_binaries, two_roots, uses,
};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

mod sums;
mod support;

use support::{PLATFORMS, build_ids};
pub(crate) use support::{job, permissions, runs};

#[test]
fn release_yml_has_one_build_job_per_binary_and_platform_then_sums_and_publish() -> TestResult {
    let (text, tree) = two_binaries()?;
    let ids: BTreeSet<&str> = jobs(&tree)?.into_iter().map(|(id, _)| id).collect();
    let mut want: BTreeSet<String> = [
        "gate",
        "release-pr",
        "tag",
        "sums-app",
        "publish-app",
        "sums-tools",
        "publish-tools",
    ]
    .map(str::to_owned)
    .into();
    want.extend(build_ids("app", PLATFORMS));
    want.extend(build_ids("tools", &["linux-x64", "macos-arm64"]));
    assert_eq!(ids, want.iter().map(String::as_str).collect());
    for forbidden in ["strategy", "matrix", "fromJSON"] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
    let runner = |id: &str| -> Result<String, Box<dyn Error>> {
        Ok(job(&tree, id)?
            .get("runs-on")
            .and_then(Node::text)
            .ok_or("no runs-on")?
            .to_owned())
    };
    assert_eq!(runner("build-app-linux-x64")?, "ubuntu-24.04");
    assert_eq!(runner("build-app-linux-arm64")?, "ubuntu-24.04-arm");
    assert_eq!(runner("build-app-macos-arm64")?, "macos-15");
    let (_, without) = two_roots()?;
    assert!(
        jobs(&without)?
            .iter()
            .all(|(id, _)| matches!(*id, "gate" | "release-pr" | "tag" | "finalize"))
    );
    Ok(())
}

#[test]
fn release_yml_build_holds_contents_read_only_and_builds_locked_without_secrets_or_cache()
-> TestResult {
    let (_, tree) = two_binaries()?;
    let mut built = 0;
    for (id, build) in jobs(&tree)?
        .into_iter()
        .filter(|(id, _)| id.starts_with("build-"))
    {
        built += 1;
        let package = PLATFORMS
            .iter()
            .find_map(|platform| {
                id.strip_prefix("build-")
                    .and_then(|rest| rest.strip_suffix(&format!("-{platform}")))
            })
            .ok_or("no platform")?;
        assert_eq!(
            permissions(build),
            [("contents".to_owned(), "read".to_owned())],
            "{id}"
        );
        assert!(build.get("environment").is_none(), "{id}");
        assert!(build.get("outputs").is_none(), "{id}");
        assert!(
            build
                .scalars()
                .iter()
                .all(|text| !text.contains("secrets.")),
            "{id}"
        );
        let actions: Vec<&str> = steps(build)
            .iter()
            .filter_map(uses)
            .filter_map(|action| action.split_once('@').map(|(name, _)| name))
            .collect();
        assert_eq!(
            actions,
            [
                "actions/checkout",
                "jdx/mise-action",
                "actions/upload-artifact"
            ],
            "{id}"
        );
        let checkout = &steps(build)[0];
        assert_eq!(
            checkout.at(&["with", "ref"]).and_then(Node::text),
            Some(format!("refs/tags/${{{{ needs.tag.outputs.{package} }}}}").as_str()),
            "{id}"
        );
        assert_eq!(
            checkout
                .at(&["with", "persist-credentials"])
                .and_then(Node::text),
            Some("false"),
            "{id}"
        );
        let cargo: Vec<&str> = runs(build)
            .into_iter()
            .filter(|run| run.starts_with("cargo "))
            .collect();
        assert_eq!(cargo.len(), 1, "{id}");
        for flag in [
            "--release",
            "--locked",
            "--package",
            "--manifest-path",
            "--bin",
            "--target",
            "--target-dir",
        ] {
            assert!(cargo[0].contains(flag), "{id} {flag}");
        }
        assert!(
            cargo[0].contains("musl") || id.ends_with("macos-arm64"),
            "{id}"
        );
    }
    assert_eq!(built, 5);
    Ok(())
}

#[test]
fn release_yml_build_packs_the_named_bin_and_uploads_exactly_that_file() -> TestResult {
    let (_, tree) = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nbin = \"rg\"\nplatforms = [\"linux-arm64\"]\n"),
    )?;
    let build = job(&tree, "build-app-linux-arm64")?;
    assert_eq!(
        runs(build),
        [
            "test -n \"$COMMIT\" -a \"$CHECKED_OUT\" = \"$COMMIT\"",
            "sudo apt-get install --yes musl-tools",
            "rustup target add aarch64-unknown-linux-musl",
            "cargo build --release --locked --manifest-path Cargo.toml --package app --bin rg --target aarch64-unknown-linux-musl --target-dir target",
            "tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf rg-aarch64-unknown-linux-musl.tar.gz -C target/aarch64-unknown-linux-musl/release rg",
        ]
    );
    let upload = steps(build).last().ok_or("no steps")?;
    assert_eq!(
        upload.at(&["with", "name"]).and_then(Node::text),
        Some("app-linux-arm64")
    );
    assert_eq!(
        upload.at(&["with", "path"]).and_then(Node::text),
        Some("rg-aarch64-unknown-linux-musl.tar.gz")
    );
    assert_eq!(
        upload
            .at(&["with", "if-no-files-found"])
            .and_then(Node::text),
        Some("error")
    );
    let publish = job(&tree, "publish-app")?;
    assert!(runs(publish)[0].ends_with(" --dir=dist rg-aarch64-unknown-linux-musl.tar.gz"));
    Ok(())
}

#[test]
fn release_yml_builds_a_root_package_from_its_own_manifest_and_target_directory() -> TestResult {
    let (_, tree) = two_binaries()?;
    let build = job(&tree, "build-tools-linux-x64")?;
    assert_eq!(
        runs(build),
        [
            "test -n \"$COMMIT\" -a \"$CHECKED_OUT\" = \"$COMMIT\"",
            "sudo apt-get install --yes musl-tools",
            "rustup target add x86_64-unknown-linux-musl",
            "cargo build --release --locked --manifest-path Cargo.toml --package tools --bin tools --target x86_64-unknown-linux-musl --target-dir target",
            "tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf tools-x86_64-unknown-linux-musl.tar.gz -C tools/target/x86_64-unknown-linux-musl/release tools",
        ]
    );
    Ok(())
}

#[test]
fn release_yml_publish_checks_out_nothing_runs_one_command_and_holds_only_contents_write()
-> TestResult {
    let (_, tree) = two_binaries()?;
    for (package, files) in [
        (
            "app",
            "app-x86_64-unknown-linux-musl.tar.gz app-aarch64-unknown-linux-musl.tar.gz app-aarch64-apple-darwin.tar.gz",
        ),
        (
            "tools",
            "tools-x86_64-unknown-linux-musl.tar.gz tools-aarch64-apple-darwin.tar.gz",
        ),
    ] {
        let publish = job(&tree, &format!("publish-{package}"))?;
        assert_eq!(
            permissions(publish),
            [("contents".to_owned(), "write".to_owned())]
        );
        assert!(publish.get("environment").is_none());
        assert!(publish.get("outputs").is_none());
        for step in steps(publish) {
            let action = uses(step).unwrap_or_default();
            assert!(!action.starts_with("actions/checkout@"), "{package}");
            assert!(
                !action.starts_with("actions/create-github-app-token@"),
                "{package}"
            );
            assert!(!action.starts_with("jdx/mise-action@"), "{package}");
        }
        assert_eq!(
            runs(publish),
            [format!(
                "velnor-ci release publish --repo=\"$REPOSITORY\" --tag=\"$TAG\" --app=velnor-release --dir=dist {files}"
            )]
        );
        let command = steps(publish).last().ok_or("no steps")?;
        assert_eq!(
            command.at(&["env", "GITHUB_TOKEN"]).and_then(Node::text),
            Some("${{ github.token }}")
        );
        assert_eq!(
            command.at(&["env", "REPOSITORY"]).and_then(Node::text),
            Some("${{ github.repository }}")
        );
        assert_eq!(
            command.at(&["env", "TAG"]).and_then(Node::text),
            Some(format!("${{{{ needs.tag.outputs.{package} }}}}").as_str())
        );
        let needs = publish.get("needs").ok_or("no needs")?.texts();
        assert_eq!(needs, ["gate", "tag", &format!("sums-{package}")]);
        let downloaded: Vec<&str> = steps(publish)
            .iter()
            .filter(|step| uses(step).is_some_and(|u| u.starts_with("actions/download-artifact@")))
            .filter_map(|step| step.at(&["with", "name"]).and_then(Node::text))
            .collect();
        assert_eq!(downloaded.last(), Some(&format!("{package}-sums").as_str()));
        assert_eq!(downloaded.len(), files.split(' ').count() + 1);
    }
    Ok(())
}

#[test]
fn release_yml_tag_exports_the_released_tag_of_each_binary_package_only() -> TestResult {
    let (_, tree) = two_binaries()?;
    let outputs = job(&tree, "tag")?.get("outputs").ok_or("no outputs")?;
    let keys = outputs.keys();
    assert_eq!(keys, ["app", "tools", "app_commit", "tools_commit"]);
    for package in ["app", "tools"] {
        assert_eq!(
            outputs.get(package).and_then(Node::text),
            Some(format!("${{{{ steps.record.outputs.{package} }}}}").as_str())
        );
    }
    let (_, without) = two_roots()?;
    let kept = job(&without, "tag")?.get("outputs").map(Node::keys);
    assert_eq!(kept, Some(vec!["app", "tools"]));
    Ok(())
}

#[test]
fn release_yml_refuses_a_bin_name_that_would_add_a_second_command() -> TestResult {
    for bin in ["rg; echo", "a b", "$(id)", "a.b"] {
        let result = render(
            &[APP, Root { ..TOOLS }],
            Some(&format!(
                "roots = [\".\", \"tools\"]\n[[binaries]]\npackage = \"app\"\nbin = {bin:?}\n"
            )),
        );
        let error = result.err().ok_or("rendered")?;
        assert!(
            matches!(
                error.downcast_ref::<GenerateError>(),
                Some(GenerateError::Release(ReleaseError::InvalidBinName { .. }))
            ),
            "{bin:?}: {error}"
        );
    }
    assert!(render(&[APP, TOOLS], Some(TWO_BINARIES)).is_ok());
    Ok(())
}
