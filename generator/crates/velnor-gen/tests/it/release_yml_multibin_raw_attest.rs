// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::ReleaseError;

use crate::release_fixture::{APP, jobs, render, steps, uses};
use crate::release_yml_binaries::{job, permissions, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const HOST: &str = "[[binaries]]\npackage = \"app\"\nformat = \"raw\"\nplatforms = [\"macos-arm64\"]\nattest = true\nbins = [{ bin = \"rg\" }]\n";

fn members() -> Vec<String> {
    ["app", "rg"]
        .iter()
        .flat_map(|bin| {
            [
                format!("{bin}-macos-arm64"),
                format!("{bin}-macos-arm64.sha256"),
                format!("{bin}-macos-arm64.provenance.json"),
            ]
        })
        .collect()
}

#[test]
fn raw_multibin_attest_lists_each_member_and_sidecar_as_a_verified_asset() -> TestResult {
    let (_, tree) = render(&[APP], Some(HOST))?;
    let sums = job(&tree, "sums-app")?;
    let attest = steps(sums)
        .iter()
        .find(|step| uses(step).is_some_and(|u| u.starts_with("actions/attest-build-provenance@")))
        .ok_or("no attest step")?;
    assert_eq!(
        attest.at(&["with", "subject-path"]).and_then(Node::text),
        Some("dist/*")
    );
    let listed = runs(sums)[0].to_owned();
    for name in members() {
        assert!(
            listed.split(' ').any(|word| word == name),
            "{name} is not in SHA256SUMS"
        );
    }

    let verified: BTreeSet<String> = runs(job(&tree, "verify-app")?)
        .into_iter()
        .filter(|run| run.contains("gh attestation verify"))
        .filter_map(|run| {
            run.split(" -- gh attestation verify ")
                .nth(1)?
                .split(' ')
                .next()
                .map(str::to_owned)
        })
        .collect();
    let expected: BTreeSet<String> = members()
        .into_iter()
        .chain(["SHA256SUMS".to_owned()])
        .map(|name| format!("dist/{name}"))
        .collect();
    assert_eq!(verified, expected);

    let publish = job(&tree, "publish-app")?;
    assert!(
        publish
            .get("needs")
            .ok_or("no needs")?
            .texts()
            .contains(&"verify-app")
    );
    assert_eq!(
        permissions(job(&tree, "verify-app")?),
        [("attestations".to_owned(), "read".to_owned())]
    );
    for (id, other) in jobs(&tree)? {
        if id != "sums-app" {
            assert!(
                !permissions(other)
                    .iter()
                    .any(|(scope, _)| scope == "id-token"),
                "{id}"
            );
        }
    }
    Ok(())
}

const FILES: &str = "[[binaries]]\npackage = \"app\"\nformat = \"raw\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\nattest = true\nfiles = [\"LICENSE\", \"legal/NOTICE\"]\nbins = [{ bin = \"rg\" }]\n";

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
fn raw_files_are_published_flat_and_attested() -> TestResult {
    let (_, tree) = render(&[APP], Some(FILES))?;
    let first = job(&tree, "build-app-linux-x64")?;
    let upload = steps(first).last().ok_or("no steps")?;
    let uploaded = upload
        .at(&["with", "path"])
        .and_then(Node::text)
        .ok_or("no path")?;
    assert!(uploaded.ends_with("\nLICENSE\nNOTICE"));
    assert!(runs(first).contains(&"cp legal/NOTICE NOTICE"));
    assert!(!runs(first).iter().any(|run| run.starts_with("cp LICENSE")));

    let second = job(&tree, "build-app-macos-arm64")?;
    let other = steps(second).last().ok_or("no steps")?;
    assert!(
        !other
            .at(&["with", "path"])
            .and_then(Node::text)
            .ok_or("no path")?
            .contains("LICENSE")
    );

    let listed = runs(job(&tree, "sums-app")?)[0].to_owned();
    for name in ["LICENSE", "NOTICE"] {
        assert_eq!(listed.split(' ').filter(|word| *word == name).count(), 1);
    }
    let verified: Vec<String> = runs(job(&tree, "verify-app")?)
        .into_iter()
        .filter_map(|run| {
            run.split(" -- gh attestation verify ")
                .nth(1)?
                .split(' ')
                .next()
                .map(str::to_owned)
        })
        .collect();
    for name in ["dist/LICENSE", "dist/NOTICE"] {
        assert_eq!(verified.iter().filter(|word| *word == name).count(), 1);
    }
    let publish = runs(job(&tree, "publish-app")?);
    assert!(publish[0].ends_with(" LICENSE NOTICE"));
    Ok(())
}

#[test]
fn raw_files_of_a_versioned_name_are_staged_beside_the_binaries() -> TestResult {
    let (_, tree) = render(
        &[APP],
        Some(&FILES.replace(
            "attest = true\n",
            "asset_name = \"{{ bin }}-{{ version }}-{{ platform }}\"\n",
        )),
    )?;
    let first = job(&tree, "build-app-linux-x64")?;
    assert!(runs(first).contains(&"cp LICENSE stage/LICENSE"));
    assert!(runs(first).contains(&"cp legal/NOTICE stage/NOTICE"));
    Ok(())
}

#[test]
fn files_with_the_archive_format_is_refused() {
    assert!(matches!(
        refused("[[binaries]]\npackage = \"app\"\nfiles = [\"LICENSE\"]\n"),
        Some(ReleaseError::InvalidBundle { .. })
    ));
}

#[test]
fn files_obey_the_bundle_path_rules() {
    for entry in [
        "../LICENSE",
        "/etc/passwd",
        "a b",
        "legal/app",
        "legal/SHA256SUMS",
    ] {
        let config =
            format!("[[binaries]]\npackage = \"app\"\nformat = \"raw\"\nfiles = [\"{entry}\"]\n");
        assert!(
            matches!(refused(&config), Some(ReleaseError::InvalidBundle { .. })),
            "{entry}"
        );
    }
}
