// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_gen::{COMPATIBLE_HELPER_SERIES, GenerateError, expected_files};
use velnor_model::{HelperVersion, RepoView};
use velnor_rust::{DetectError, LOCK_PATH, Reason, pin_lock};

use crate::pipeline::{view, workflow};

type TestResult = Result<(), Box<dyn Error>>;

const SUMS: &str = "\
10e3511ab3d2f211366c6fd3633474f4a74ee6750b16b52c90758c0e8321dd3e  velnor-ci-linux-x64
56d836fd2ae8135b5306f154b3287dfe81328f7a89b0528b3b912fab6e6916a6  velnor-ci-linux-arm64
39431db3aeccf193088fa8845896c627e3da004f683d4d4699d3d697c808c607  velnor-ci-macos-arm64
61ec20debeae57cce969660e66c72315c59ce3cee84659540aee1942aad52628  velnor-gen-linux-x64
1139e26866b8200c7b42095ca21f6849bb3212af157b967e831cdf5078ae5fba  velnor-gen-linux-arm64
85e98f573e84d03701c0275e60f54a8f8b07c7c02b6b773cf25d973b960339a7  velnor-gen-macos-arm64
";

fn with_lock(lock: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut view = view(&["Cargo.toml"], None)?;
    view.tracked_paths.insert(LOCK_PATH);
    view.files.insert(LOCK_PATH, lock.as_bytes().to_vec());
    Ok(view)
}

fn locked(version: &str) -> Result<RepoView, Box<dyn Error>> {
    with_lock(&pin_lock(version, SUMS)?)
}

#[test]
fn pin_helper_writes_the_lock_file_the_sample_repository_commits() -> TestResult {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/helper-lock/view")
        .join(LOCK_PATH.as_str());
    assert_eq!(pin_lock("0.1.79", SUMS)?, fs::read_to_string(sample)?);
    Ok(())
}

#[test]
fn a_release_whose_sums_do_not_cover_both_programs_on_every_platform_is_not_pinned() {
    let without = |name: &str| -> String {
        SUMS.lines()
            .filter(|line| !line.ends_with(name))
            .flat_map(|line| [line, "\n"])
            .collect()
    };
    let twice = format!("{SUMS}{}\n", SUMS.lines().next().unwrap_or_default());
    let short = SUMS.replacen("10e3511a", "10E3511A", 1);
    let cases = [
        (without("velnor-ci-linux-arm64"), "0.1.79"),
        (without("velnor-gen-macos-arm64"), "0.1.79"),
        (twice, "0.1.79"),
        (short, "0.1.79"),
        (SUMS.to_owned(), "0.1"),
        (SUMS.to_owned(), "v0.1.79"),
        (SUMS.to_owned(), "0.1.79-rc1"),
        (SUMS.to_owned(), "0.1.79/../../x"),
        (String::new(), "0.1.79"),
    ];
    for (sums, version) in cases {
        let result = pin_lock(version, &sums);
        assert!(result.is_err(), "{version} with {sums:?} gave {result:?}");
    }
}

#[test]
fn a_locked_helper_is_fetched_right_after_the_checkout_and_nowhere_else() -> TestResult {
    let text = workflow(&locked("0.1.79")?)?;
    let steps: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("      - name: "))
        .collect();
    assert_eq!(steps[..2], ["Check out", "Fetch velnor-ci 0.1.79"]);
    assert_eq!(
        steps
            .iter()
            .filter(|step| step.starts_with("Fetch velnor-ci"))
            .count(),
        1
    );
    assert!(text.contains(
        "https://github.com/tailrocks/velnor-new/releases/download/rust-ci-v0.1.79/velnor-ci-linux-arm64"
    ));
    assert!(text.contains(
        "\"Linux-X64\":\"10e3511ab3d2f211366c6fd3633474f4a74ee6750b16b52c90758c0e8321dd3e\""
    ));
    assert!(!workflow(&view(&["Cargo.toml"], None)?)?.contains("Fetch velnor-ci"));
    Ok(())
}

#[test]
fn a_lock_file_that_is_not_exactly_what_pin_helper_writes_is_rejected() -> TestResult {
    let good = pin_lock("0.1.79", SUMS)?;
    let ci = "10e3511ab3d2f211366c6fd3633474f4a74ee6750b16b52c90758c0e8321dd3e";
    let generator = "61ec20debeae57cce969660e66c72315c59ce3cee84659540aee1942aad52628";
    let cases = [
        good.replace("linux-x64 = ", "linux-x86 = "),
        good.replacen(ci, &ci[..63], 1),
        good.replacen(ci, &ci.to_uppercase(), 1),
        good.replacen(generator, &generator[..63], 1),
        good.replacen(&format!("linux-x64 = \"{generator}\"\n"), "", 1),
        format!("{good}windows-x64 = \"{generator}\"\n"),
        good.replace(
            "[velnor-gen]",
            &format!("windows-x64 = \"{ci}\"\n\n[velnor-gen]"),
        ),
        good.replace("version = \"0.1.79\"", "version = \"0.1\""),
        good.replace("version = \"0.1.79\"\n", ""),
        format!("{good}extra = 1\n"),
        format!("{good}[velnor-gen]\n"),
        good.replace("[velnor-gen]", "[velnor-gen.more]"),
        String::new(),
        "not toml [".to_owned(),
    ];
    for lock in cases {
        let result = expected_files(&with_lock(&lock)?);
        assert!(
            matches!(
                result,
                Err(GenerateError::Detect(DetectError::Invalid {
                    reason: Reason::HelperLock(_),
                    ..
                }))
            ),
            "{lock} gave {result:?}"
        );
    }
    let mut binary = view(&["Cargo.toml"], None)?;
    binary.files.insert(LOCK_PATH, vec![0xff, 0xfe]);
    assert!(matches!(
        expected_files(&binary),
        Err(GenerateError::Detect(DetectError::Invalid {
            reason: Reason::HelperLock(_),
            ..
        }))
    ));
    Ok(())
}

#[test]
fn only_helpers_of_the_same_major_and_minor_are_accepted() -> TestResult {
    let (major, minor) = COMPATIBLE_HELPER_SERIES;
    for version in [
        format!("{major}.{minor}.0"),
        format!("{major}.{minor}.79"),
        format!("{major}.{minor}.4294967296"),
    ] {
        assert!(expected_files(&locked(&version)?).is_ok(), "{version}");
    }
    for version in [
        format!("{major}.{}.0", minor + 1),
        format!("{}.{minor}.0", major + 1),
        format!("{major}.{}.79", minor - 1),
        "1.0.0".to_owned(),
    ] {
        let result = expected_files(&locked(&version)?);
        assert_eq!(
            result,
            Err(GenerateError::IncompatibleHelper(HelperVersion::new(
                &version
            )?)),
            "{version}"
        );
        let said = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            said.contains(&format!(
                "velnor-ci {version} does not fit this velnor-gen, which works with velnor-ci {major}.{minor}.x"
            )),
            "{said}"
        );
    }
    Ok(())
}
