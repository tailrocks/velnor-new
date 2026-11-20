// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_rust::{binary_sums_url, lists_binary_sums, pin_lock, release_version};

use crate::lock::{COMMIT_FILE, SUMS, locked};
use crate::pipeline::workflow;

type TestResult = Result<(), Box<dyn Error>>;

const CI_ARCHIVES: &[(&str, &str, &str)] = &[
    (
        "Linux",
        "X64",
        "b91a87ecac8eda2809f202b4566d886273d7b34d6ef29f726751fcbc8bb3d502",
    ),
    (
        "Linux",
        "ARM64",
        "5ff173c7729c981f5e85d9506d5e496729eefaeabf54ab475d814a4955de0830",
    ),
    (
        "macOS",
        "ARM64",
        "2e0aa3b9b6338d1149761739d87f67def027c35ab8c4b888198cc611141f0d0a",
    ),
];

const CI_BINARIES: &[(&str, &str, &str)] = &[
    (
        "Linux",
        "X64",
        "9de941ee8e31748973283b2cd31a439119a9d22f60ec3e2884d855a8e2971c6f",
    ),
    (
        "Linux",
        "ARM64",
        "6d887a0b48bba9c85da802b1943beb3ff38a2ffbc0effbdff615e5f53fe6a4a8",
    ),
    (
        "macOS",
        "ARM64",
        "a7e9a4897a81bb86338896573e43b79b4f05e34a489549f1687117dd456648ba",
    ),
];

const ARCHIVE_SUMS: &str = "\
0e5bd0f714140aa32270568a463b252853b828318ed29e3bad93a1bf18ce2fbd  COMMIT
4e9a485939165d82a9ebd49078778ca47137f344524ce8d87d03116760d5bb6d  BINARY-SHA256SUMS
b91a87ecac8eda2809f202b4566d886273d7b34d6ef29f726751fcbc8bb3d502  velnor-ci-x86_64-unknown-linux-musl.tar.gz
5ff173c7729c981f5e85d9506d5e496729eefaeabf54ab475d814a4955de0830  velnor-ci-aarch64-unknown-linux-musl.tar.gz
2e0aa3b9b6338d1149761739d87f67def027c35ab8c4b888198cc611141f0d0a  velnor-ci-aarch64-apple-darwin.tar.gz
2a3ff18c1c5fc61a10aa66976ccf1fe669daa4af4eafec260b4c87d81afdf8f9  velnor-gen-x86_64-unknown-linux-musl.tar.gz
300f6b8a0d89cfe0643433710d7df3453c3524e7ff84c7dfc84854a78c7e120c  velnor-gen-aarch64-unknown-linux-musl.tar.gz
d8a670bd3cbbcec7a282121cbd37d7d52e1ee17793c7c453d5d7ff345ca7318c  velnor-gen-aarch64-apple-darwin.tar.gz
";

const BINARY_SUMS: &str = "\
9de941ee8e31748973283b2cd31a439119a9d22f60ec3e2884d855a8e2971c6f  velnor-ci-x86_64-unknown-linux-musl.tar.gz/velnor-ci
6d887a0b48bba9c85da802b1943beb3ff38a2ffbc0effbdff615e5f53fe6a4a8  velnor-ci-aarch64-unknown-linux-musl.tar.gz/velnor-ci
a7e9a4897a81bb86338896573e43b79b4f05e34a489549f1687117dd456648ba  velnor-ci-aarch64-apple-darwin.tar.gz/velnor-ci
";

pub(crate) fn archive_lock() -> Result<String, Box<dyn Error>> {
    archive_lock_at("0.1.81")
}

/// The archive lock pinned at `version`, for a test whose config needs that helper.
pub(crate) fn archive_lock_at(version: &str) -> Result<String, Box<dyn Error>> {
    Ok(pin_lock(
        version,
        COMMIT_FILE,
        ARCHIVE_SUMS,
        Some(BINARY_SUMS),
    )?)
}

#[test]
fn a_new_lock_records_the_archive_digests_and_the_binary_digests() -> TestResult {
    let lock = archive_lock()?;
    let table = |name: &str| -> Vec<String> {
        lock.split("\n\n")
            .find(|part| part.starts_with(&format!("[{name}]")))
            .map(|part| part.lines().skip(1).map(str::to_owned).collect())
            .unwrap_or_default()
    };
    assert_eq!(
        table("velnor-ci"),
        [
            "linux-arm64 = \"5ff173c7729c981f5e85d9506d5e496729eefaeabf54ab475d814a4955de0830\"",
            "linux-x64 = \"b91a87ecac8eda2809f202b4566d886273d7b34d6ef29f726751fcbc8bb3d502\"",
            "macos-arm64 = \"2e0aa3b9b6338d1149761739d87f67def027c35ab8c4b888198cc611141f0d0a\"",
        ]
    );
    assert_eq!(
        table("velnor-ci-binary"),
        [
            "linux-arm64 = \"6d887a0b48bba9c85da802b1943beb3ff38a2ffbc0effbdff615e5f53fe6a4a8\"",
            "linux-x64 = \"9de941ee8e31748973283b2cd31a439119a9d22f60ec3e2884d855a8e2971c6f\"",
            "macos-arm64 = \"a7e9a4897a81bb86338896573e43b79b4f05e34a489549f1687117dd456648ba\"",
        ]
    );
    assert_eq!(
        table("velnor-gen"),
        [
            "linux-arm64 = \"300f6b8a0d89cfe0643433710d7df3453c3524e7ff84c7dfc84854a78c7e120c\"",
            "linux-x64 = \"2a3ff18c1c5fc61a10aa66976ccf1fe669daa4af4eafec260b4c87d81afdf8f9\"",
            "macos-arm64 = \"d8a670bd3cbbcec7a282121cbd37d7d52e1ee17793c7c453d5d7ff345ca7318c\"",
        ]
    );
    Ok(())
}

#[test]
fn the_pin_reads_the_archive_lines_and_ignores_raw_lines_beside_them() -> TestResult {
    let mixed = format!(
        "{ARCHIVE_SUMS}{}",
        &SUMS[SUMS.find('\n').map_or(0, |at| at + 1)..]
    );
    assert_eq!(
        pin_lock("0.1.81", COMMIT_FILE, &mixed, Some(BINARY_SUMS))?,
        archive_lock()?
    );
    Ok(())
}

#[test]
fn an_archive_release_is_not_pinned_without_a_verified_complete_binary_digest_file() {
    let swap = |from: &str, to: &str| ARCHIVE_SUMS.replacen(from, to, 1);
    let unlisted: String = ARCHIVE_SUMS
        .lines()
        .filter(|line| !line.ends_with("BINARY-SHA256SUMS"))
        .flat_map(|line| [line, "\n"])
        .collect();
    let no_archive: String = ARCHIVE_SUMS
        .lines()
        .filter(|line| !line.contains("velnor-ci-x86_64-unknown-linux-musl"))
        .flat_map(|line| [line, "\n"])
        .collect();
    let short_binary: String = BINARY_SUMS
        .lines()
        .take(2)
        .flat_map(|line| [line, "\n"])
        .collect();
    let cases = [
        (ARCHIVE_SUMS.to_owned(), None),
        (ARCHIVE_SUMS.to_owned(), Some(short_binary.as_str())),
        (ARCHIVE_SUMS.to_owned(), Some("")),
        (
            ARCHIVE_SUMS.to_owned(),
            Some(&*BINARY_SUMS.replacen("9de941ee", "9de941ef", 1)),
        ),
        (unlisted, Some(BINARY_SUMS)),
        (no_archive, Some(BINARY_SUMS)),
        (swap("4e9a4859", "4e9a485A"), Some(BINARY_SUMS)),
    ];
    for (sums, binary) in cases {
        let result = pin_lock("0.1.81", COMMIT_FILE, &sums, binary);
        assert!(result.is_err(), "{sums:?} with {binary:?} gave {result:?}");
    }
}

#[test]
fn a_linux_only_archive_release_is_pinned() -> TestResult {
    let linux = |text: &str| -> String {
        text.lines()
            .filter(|line| {
                line.contains("linux") || line.ends_with("SUMS") || line.ends_with("COMMIT")
            })
            .flat_map(|line| [line, "\n"])
            .collect()
    };
    let sums = linux(ARCHIVE_SUMS).replace(
        "4e9a485939165d82a9ebd49078778ca47137f344524ce8d87d03116760d5bb6d",
        "501023317acc4d029a91ce011d5ac710e07432d040d17dfd0ce88d4c0a6b132a",
    );
    let lock = pin_lock("0.1.79", COMMIT_FILE, &sums, Some(&linux(BINARY_SUMS)))?;
    assert!(
        lock.contains("linux-x64") && !lock.contains("d8a670bd"),
        "{lock}"
    );
    Ok(())
}

#[test]
fn only_a_release_that_lists_the_binary_digest_file_needs_it_downloaded() -> TestResult {
    assert!(lists_binary_sums(ARCHIVE_SUMS));
    assert!(!lists_binary_sums(SUMS));
    assert_eq!(
        binary_sums_url(&release_version("0.1.81")?),
        "https://github.com/tailrocks/velnor-new/releases/download/rust-ci-v0.1.81/BINARY-SHA256SUMS"
    );
    Ok(())
}

fn step_of(text: &str, name: &str) -> Result<serde_json::Value, Box<dyn Error>> {
    let document = crate::yaml_tree::document(text)?;
    document["jobs"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, job)| job["steps"].as_array())
        .flatten()
        .find(|step| {
            step["name"]
                .as_str()
                .is_some_and(|found| found.starts_with(name))
        })
        .cloned()
        .ok_or_else(|| format!("no step {name}").into())
}

fn launcher_digest(text: &str) -> Result<String, Box<dyn Error>> {
    step_of(text, "Install the released helper launcher")?["env"]["VELNOR_HELPER_SHA256"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "no launcher digest".into())
}

#[test]
fn the_action_checks_the_archive_and_the_launcher_checks_the_binary() -> TestResult {
    let text = workflow(&crate::lock::with_lock(&archive_lock()?)?)?;
    let install = step_of(&text, "Install velnor-ci")?;
    for (input, (_, _, digest)) in [
        "sha256-linux-x64",
        "sha256-linux-arm64",
        "sha256-macos-arm64",
    ]
    .into_iter()
    .zip(CI_ARCHIVES.iter().copied())
    {
        assert_eq!(install["with"][input], digest);
    }
    let launcher = launcher_digest(&text)?;
    for &(os, arch, digest) in CI_BINARIES {
        assert!(
            launcher.contains(&format!(
                "runner.os == '{os}' && runner.arch == '{arch}' && '{digest}'"
            )),
            "{launcher}"
        );
    }
    for &(_, _, archive) in CI_ARCHIVES {
        assert!(!launcher.contains(archive), "{launcher}");
    }
    Ok(())
}

#[test]
fn an_old_lock_with_only_the_raw_digest_still_checks_that_digest_in_both_places() -> TestResult {
    let text = workflow(&locked("0.1.81")?)?;
    let raw = "10e3511ab3d2f211366c6fd3633474f4a74ee6750b16b52c90758c0e8321dd3e";
    assert!(
        text.contains(&format!("sha256-linux-x64: '{raw}'")),
        "{text}"
    );
    assert!(launcher_digest(&text)?.contains(&format!("runner.arch == 'X64' && '{raw}'")));
    assert!(!text.contains("velnor-ci-binary"));
    Ok(())
}
