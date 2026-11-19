// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

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
