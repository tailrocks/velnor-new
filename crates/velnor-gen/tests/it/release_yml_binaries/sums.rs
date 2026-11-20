// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::TestResult;
use super::support::{PLATFORMS, build_ids, job, runs, triple};
use crate::release_fixture::{steps, two_binaries, uses};
use crate::yaml_tree::Node;

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
        let mut want: BTreeSet<String> = BTreeSet::from(["gate".to_owned(), "tag".to_owned()]);
        want.extend(build_ids(package, &platforms));
        assert_eq!(
            needs,
            want.iter().map(String::as_str).collect(),
            "{package}"
        );
        let files: Vec<String> = platforms
            .iter()
            .map(|p| format!("{package}-{}.tar.gz", triple(p)))
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
