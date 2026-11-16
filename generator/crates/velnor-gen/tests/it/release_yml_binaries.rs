// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use crate::release_fixture::{jobs, steps, two_binaries, two_roots, uses};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const PLATFORMS: [&str; 3] = ["linux-x64", "linux-arm64", "macos-arm64"];

pub(crate) fn tag_of(package: &str) -> &'static str {
    if package == "tools" { "tag-1" } else { "tag-0" }
}

fn tagged_ref(package: &str) -> String {
    format!(
        "refs/tags/${{{{ needs.{}.outputs.{package} }}}}",
        tag_of(package)
    )
}

pub(crate) fn job<'a>(tree: &'a Node, id: &str) -> Result<&'a Node, Box<dyn Error>> {
    Ok(tree
        .at(&["jobs", id])
        .ok_or_else(|| format!("no job {id}"))?)
}

fn permissions(job: &Node) -> Vec<(String, String)> {
    job.get("permissions").map_or_else(Vec::new, |node| {
        node.keys()
            .into_iter()
            .filter_map(|key| {
                node.get(key)
                    .and_then(Node::text)
                    .map(|access| (key.to_owned(), access.to_owned()))
            })
            .collect()
    })
}

pub(crate) fn runs(job: &Node) -> Vec<&str> {
    steps(job)
        .iter()
        .filter_map(|step| step.get("run").and_then(Node::text))
        .collect()
}

fn build_ids(package: &str, platforms: &[&str]) -> Vec<String> {
    platforms
        .iter()
        .map(|platform| format!("build-{package}-{platform}"))
        .collect()
}

#[test]
fn release_yml_has_one_build_job_per_binary_and_platform_then_sums_and_publish() -> TestResult {
    let (text, tree) = two_binaries()?;
    let ids: BTreeSet<&str> = jobs(&tree)?.into_iter().map(|(id, _)| id).collect();
    let mut want: BTreeSet<String> = [
        "gate",
        "release-pr",
        "tag-0",
        "tag-1",
        "sums-app",
        "publish-app",
        "sums-tools",
        "publish-tools",
    ]
    .map(str::to_owned)
    .into();
    want.extend(build_ids("app", &PLATFORMS));
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
    assert!(jobs(&without)?.iter().all(|(id, _)| matches!(
        *id,
        "gate" | "release-pr" | "tag-0" | "tag-1" | "finalize-0" | "finalize-1"
    )));
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
            Some(tagged_ref(package).as_str()),
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
fn release_yml_builds_a_root_package_from_its_own_manifest_and_target_directory() -> TestResult {
    let (_, tree) = two_binaries()?;
    let build = job(&tree, "build-tools-linux-x64")?;
    assert_eq!(
        runs(build),
        [
            "test \"$CHECKED_OUT\" = \"$COMMIT\"",
            "sudo apt-get install --yes musl-tools",
            "rustup target add x86_64-unknown-linux-musl",
            "cargo build --release --locked --manifest-path Cargo.toml --package tools --bin tools --target x86_64-unknown-linux-musl --target-dir target",
            "tar -czf tools-linux-x64.tar.gz -C tools/target/x86_64-unknown-linux-musl/release tools",
        ]
    );
    Ok(())
}

#[test]
fn release_yml_sums_has_no_permissions_and_hashes_exactly_the_builds_of_its_package() -> TestResult
{
    let (_, tree) = two_binaries()?;
    for (package, platforms) in [
        ("app", PLATFORMS.to_vec()),
        ("tools", vec!["linux-x64", "macos-arm64"]),
    ] {
        let sums = job(&tree, &format!("sums-{package}"))?;
        assert_eq!(
            sums.get("permissions"),
            Some(&Node::Map(Vec::new())),
            "{package}"
        );
        assert!(sums.get("environment").is_none());
        let needs: BTreeSet<&str> = sums
            .get("needs")
            .ok_or("no needs")?
            .texts()
            .into_iter()
            .collect();
        let mut want: BTreeSet<String> =
            BTreeSet::from(["gate".to_owned(), tag_of(package).to_owned()]);
        want.extend(build_ids(package, &platforms));
        assert_eq!(
            needs,
            want.iter().map(String::as_str).collect(),
            "{package}"
        );
        let files: Vec<String> = platforms
            .iter()
            .map(|p| format!("{package}-{p}.tar.gz"))
            .collect();
        assert_eq!(
            runs(sums),
            [
                format!("sha256sum {} > SHA256SUMS", files.join(" ")),
                format!("shasum -a 256 {} > SHA256SUMS", files.join(" ")),
            ],
            "{package}"
        );
        let downloaded: Vec<&str> = steps(sums)
            .iter()
            .filter(|step| uses(step).is_some_and(|u| u.starts_with("actions/download-artifact@")))
            .filter_map(|step| step.at(&["with", "name"]).and_then(Node::text))
            .collect();
        assert_eq!(
            downloaded,
            platforms
                .iter()
                .map(|p| format!("{package}-{p}"))
                .collect::<Vec<_>>()
        );
        assert!(
            steps(sums)
                .iter()
                .all(|step| uses(step).is_none_or(|u| !u.starts_with("actions/checkout@")))
        );
    }
    Ok(())
}

#[test]
fn release_yml_publish_checks_out_nothing_runs_one_command_and_holds_only_contents_write()
-> TestResult {
    let (_, tree) = two_binaries()?;
    for (package, files) in [
        (
            "app",
            "app-linux-x64.tar.gz app-linux-arm64.tar.gz app-macos-arm64.tar.gz",
        ),
        ("tools", "tools-linux-x64.tar.gz tools-macos-arm64.tar.gz"),
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
                "velnor-ci release publish --repo \"$REPOSITORY\" --tag \"$TAG\" --app velnor-release --dir dist {files}"
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
            Some(format!("${{{{ needs.{}.outputs.{package} }}}}", tag_of(package)).as_str())
        );
        let needs = publish.get("needs").ok_or("no needs")?.texts();
        assert_eq!(needs, ["gate", tag_of(package), &format!("sums-{package}")]);
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
fn release_yml_each_tag_job_exports_the_released_tag_of_the_packages_of_its_root() -> TestResult {
    let (_, tree) = two_binaries()?;
    for package in ["app", "tools"] {
        let outputs = job(&tree, tag_of(package))?
            .get("outputs")
            .ok_or("no outputs")?;
        assert_eq!(outputs.keys(), [package]);
        assert_eq!(
            outputs.get(package).and_then(Node::text),
            Some(format!("${{{{ steps.record.outputs.{package} }}}}").as_str())
        );
    }
    let (_, without) = two_roots()?;
    assert_eq!(
        job(&without, "tag-0")?.get("outputs").map(Node::keys),
        Some(vec!["app"])
    );
    assert_eq!(
        job(&without, "tag-1")?.get("outputs").map(Node::keys),
        Some(vec!["tools"])
    );
    Ok(())
}
