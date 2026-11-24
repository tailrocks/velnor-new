// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, render, steps, uses};
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const TRIPLES: [(&str, &str); 3] = [
    ("linux-x64", "x86_64-unknown-linux-musl"),
    ("linux-arm64", "aarch64-unknown-linux-musl"),
    ("macos-arm64", "aarch64-apple-darwin"),
];

const REPRODUCIBLE: &str = "--sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner ";
const DEFAULT: &str = "[[binaries]]\npackage = \"app\"\n";
const SIDECARS: &str = "[[binaries]]\npackage = \"app\"\nchecksum_files = \"sha256\"\n";
const HOST: &str = "[[binaries]]\npackage = \"app\"\nplatforms = [\"macos-arm64\", \"linux-x64\"]\nattest = true\nchecksum_files = \"sha256\"\nbins = [{ bin = \"rg\" }]\ninclude = [\"LICENSE\", \"NOTICE\"]\n";

fn archive(triple: &str) -> String {
    format!("app-{triple}.tar.gz")
}

fn summed(tree: &Node) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let sums = job(tree, "sums-app")?;
    let line = runs(sums)
        .into_iter()
        .find(|run| run.starts_with("sha256sum "))
        .ok_or("no sha256sum step")?;
    Ok(line
        .trim_start_matches("sha256sum ")
        .trim_end_matches(" > SHA256SUMS")
        .split(' ')
        .map(str::to_owned)
        .collect())
}

fn verified(tree: &Node) -> Result<BTreeSet<String>, Box<dyn Error>> {
    Ok(runs(job(tree, "verify-app")?)
        .into_iter()
        .filter_map(|run| {
            run.split(" -- gh attestation verify ")
                .nth(1)?
                .split(' ')
                .next()
                .map(|path| path.trim_start_matches("dist/").to_owned())
        })
        .collect())
}

fn refused(config: &str) -> Option<ReleaseError> {
    match render(&[APP], Some(config))
        .err()?
        .downcast::<GenerateError>()
    {
        Ok(error) => match *error {
            GenerateError::Release(error) => Some(error),
            _ => None,
        },
        Err(_) => None,
    }
}

#[test]
fn archives_are_named_after_the_binary_and_the_rust_target_triple() -> TestResult {
    let (_, tree) = render(&[APP], Some(DEFAULT))?;
    for (platform, triple) in TRIPLES {
        let build = job(&tree, &format!("build-app-{platform}"))?;
        let pack = runs(build)
            .into_iter()
            .find(|run| run.starts_with("tar "))
            .ok_or("no pack step")?;
        assert_eq!(
            pack,
            format!(
                "tar {}-czf {} -C target/{triple}/release app",
                if platform.starts_with("linux") {
                    REPRODUCIBLE
                } else {
                    ""
                },
                archive(triple)
            )
        );
        let upload = steps(build).last().ok_or("no steps")?;
        assert_eq!(
            upload.at(&["with", "path"]).and_then(Node::text),
            Some(archive(triple).as_str())
        );
    }
    let publish = runs(job(&tree, "publish-app")?)[0].to_owned();
    for (_, triple) in TRIPLES {
        assert!(
            publish.contains(&format!(" {}", archive(triple))),
            "{publish}"
        );
    }
    assert!(!publish.contains("linux-x64"), "{publish}");
    Ok(())
}

#[test]
fn sha256sums_lists_exactly_the_archives_by_default() -> TestResult {
    let (_, tree) = render(&[APP], Some(DEFAULT))?;
    let want: BTreeSet<String> = TRIPLES.iter().map(|(_, triple)| archive(triple)).collect();
    assert_eq!(summed(&tree)?, want);
    assert!(!tree_text(&tree).contains(".sha256"));
    Ok(())
}

#[test]
fn archives_are_packed_reproducibly() -> TestResult {
    let (_, tree) = render(&[APP], Some(DEFAULT))?;
    let pack = runs(job(&tree, "build-app-linux-x64")?)
        .into_iter()
        .find(|run| run.starts_with("tar "))
        .ok_or("no pack step")?;
    for flag in [
        "--sort=name",
        "--mtime=@0",
        "--owner=0",
        "--group=0",
        "--numeric-owner",
    ] {
        assert!(pack.contains(flag), "{pack}");
    }
    Ok(())
}

fn tree_text(tree: &Node) -> String {
    let mut text = String::new();
    let Some(jobs) = tree.get("jobs") else {
        return text;
    };
    for id in jobs
        .keys()
        .into_iter()
        .filter(|id| id.ends_with("-app") || id.contains("-app-"))
    {
        let Some(job) = jobs.get(id) else { continue };
        for run in runs(job) {
            text.push_str(run);
            text.push('\n');
        }
        for step in steps(job) {
            if let Some(path) = step.at(&["with", "path"]).and_then(Node::text) {
                text.push_str(path);
                text.push('\n');
            }
        }
    }
    text
}

#[test]
fn checksum_files_adds_one_sidecar_per_archive_that_sha256sum_check_reads() -> TestResult {
    let (_, tree) = render(&[APP], Some(SIDECARS))?;
    for (platform, triple) in TRIPLES {
        let build = job(&tree, &format!("build-app-{platform}"))?;
        let name = archive(triple);
        let tool = if platform == "macos-arm64" {
            "shasum -a 256"
        } else {
            "sha256sum"
        };
        let commands = runs(build);
        let pack = commands
            .iter()
            .position(|run| run.starts_with("tar "))
            .ok_or("no pack step")?;
        assert_eq!(commands[pack + 1], format!("{tool} {name} > {name}.sha256"));
        let upload = steps(build).last().ok_or("no steps")?;
        assert_eq!(
            upload.at(&["with", "path"]).and_then(Node::text),
            Some(format!("{name}\n{name}.sha256").as_str())
        );
    }
    let want: BTreeSet<String> = TRIPLES
        .iter()
        .flat_map(|(_, triple)| [archive(triple), format!("{}.sha256", archive(triple))])
        .collect();
    assert_eq!(summed(&tree)?, want);
    let publish = runs(job(&tree, "publish-app")?)[0].to_owned();
    for name in &want {
        assert!(publish.contains(&format!(" {name}")), "{name}");
    }
    Ok(())
}

#[test]
fn checksum_files_is_refused_for_a_raw_binary_which_always_has_its_sidecar() {
    assert!(matches!(
        refused("[[binaries]]\npackage = \"app\"\nformat = \"raw\"\nchecksum_files = \"sha256\"\n"),
        Some(ReleaseError::KeyNeedsFormat {
            key: "checksum_files",
            ..
        })
    ));
}

#[test]
fn attestation_covers_every_archive_sidecar_and_the_sums_file() -> TestResult {
    let (_, tree) = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\nattest = true\nchecksum_files = \"sha256\"\n"),
    )?;
    let sums = job(&tree, "sums-app")?;
    let attest = steps(sums)
        .iter()
        .find(|step| uses(step).is_some_and(|u| u.starts_with("actions/attest-build-provenance@")))
        .ok_or("no attest step")?;
    assert_eq!(
        attest.at(&["with", "subject-path"]).and_then(Node::text),
        Some("dist/*")
    );
    let want: BTreeSet<String> = TRIPLES
        .iter()
        .flat_map(|(_, triple)| [archive(triple), format!("{}.sha256", archive(triple))])
        .chain(["SHA256SUMS".to_owned()])
        .collect();
    assert_eq!(verified(&tree)?, want);
    Ok(())
}

#[test]
fn a_velnor_host_shaped_archive_holds_the_binary_the_helper_license_and_notice() -> TestResult {
    let (_, tree) = render(&[APP], Some(HOST))?;
    let pack = runs(job(&tree, "build-app-macos-arm64")?)
        .into_iter()
        .find(|run| run.starts_with("tar "))
        .ok_or("no pack step")?;
    assert_eq!(
        pack,
        "tar -czf app-aarch64-apple-darwin.tar.gz -C target/aarch64-apple-darwin/release app rg -C \"$GITHUB_WORKSPACE\" LICENSE -C \"$GITHUB_WORKSPACE\" NOTICE"
    );
    let want: BTreeSet<String> = [
        "app-aarch64-apple-darwin.tar.gz",
        "app-aarch64-apple-darwin.tar.gz.sha256",
        "app-x86_64-unknown-linux-musl.tar.gz",
        "app-x86_64-unknown-linux-musl.tar.gz.sha256",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(summed(&tree)?, want);
    let want_verified: BTreeSet<String> =
        want.into_iter().chain(["SHA256SUMS".to_owned()]).collect();
    assert_eq!(verified(&tree)?, want_verified);
    assert!(!tree_text(&tree).contains("NOTICE.sha256"));
    assert!(!tree_text(&tree).contains("LICENSE.sha256"));
    Ok(())
}

#[test]
fn an_archive_may_name_the_gnu_target() -> TestResult {
    let (_, tree) = render(
        &[APP],
        Some("[[binaries]]\npackage = \"app\"\ntarget = \"gnu\"\nplatforms = [\"linux-arm64\"]\n"),
    )?;
    let build = job(&tree, "build-app-linux-arm64")?;
    let commands = runs(build);
    assert!(commands.contains(&"rustup target add aarch64-unknown-linux-gnu"));
    assert!(commands.contains(
        &"tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf app-aarch64-unknown-linux-gnu.tar.gz -C target/aarch64-unknown-linux-gnu/release app"
    ));
    assert!(!commands.iter().any(|run| run.contains("musl")));
    Ok(())
}

#[test]
fn an_explicit_asset_name_still_replaces_the_triple_name() -> TestResult {
    let (_, tree) = render(
        &[APP],
        Some(
            "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nasset_name = \"{{ bin }}_{{ platform }}\"\n",
        ),
    )?;
    assert!(runs(job(&tree, "build-app-linux-x64")?).contains(
        &"tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf app_linux-x64.tar.gz -C target/x86_64-unknown-linux-musl/release app"
    ));
    Ok(())
}

#[test]
fn a_versioned_name_gets_its_sidecar_in_the_stage_folder_with_the_version_bound() -> TestResult {
    let (_, tree) = render(
        &[APP],
        Some(
            "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\nchecksum_files = \"sha256\"\nasset_name = \"{{ bin }}-{{ version }}-{{ target }}\"\n",
        ),
    )?;
    let step = steps(job(&tree, "build-app-linux-x64")?)
        .iter()
        .find(|step| {
            step.get("name")
                .and_then(Node::text)
                .is_some_and(|name| name.starts_with("Checksum "))
        })
        .ok_or("no checksum step")?;
    assert_eq!(
        step.get("run").and_then(Node::text),
        Some(
            "sha256sum \"app-${VERSION}-x86_64-unknown-linux-musl.tar.gz\" > \"app-${VERSION}-x86_64-unknown-linux-musl.tar.gz.sha256\""
        )
    );
    assert_eq!(
        step.get("working-directory").and_then(Node::text),
        Some("${{ runner.temp }}/stage")
    );
    assert_eq!(
        step.at(&["env", "VERSION"]).and_then(Node::text),
        Some("${{ steps.version.outputs.version }}")
    );
    Ok(())
}
