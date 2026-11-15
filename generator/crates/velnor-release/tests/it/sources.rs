// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_release::{ReleaseError, ReleasePlan};

use crate::fixture::{TestResult, files, refusal};

fn with_mise(text: &str) -> velnor_release::ReleaseFiles {
    let mut files = files();
    files.mise_tomls = vec![text.to_owned()];
    files
}

#[test]
fn release_plz_must_come_from_an_approved_source_without_extra_options() {
    for text in [
        "[tools.release-plz]\nversion = \"0.4.3\"\npath = \"./fake\"\n",
        "[tools.release-plz]\nversion = \"0.4.3\"\nref = \"main\"\n",
        "[tools.release-plz]\nversion = \"0.4.3\"\nprefix = \"x\"\n",
        "[tools.\"github:evil/release-plz\"]\nversion = \"0.4.3\"\n",
        "[tools.\"cargo:https://example.com/release-plz\"]\nversion = \"0.4.3\"\n",
    ] {
        let error = refusal(&with_mise(text));
        assert!(
            matches!(error, Some(ReleaseError::ReleasePlzSource { .. })),
            "{text}: {error:?}"
        );
    }
}

#[test]
fn the_plan_keeps_the_mise_key_that_matched_the_lock() -> TestResult {
    let mut files = with_mise("[tools]\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n");
    files.mise_lock =
        "[[tools.\"aqua:release-plz/release-plz\"]]\nversion = \"0.4.3\"\n".to_owned();
    assert_eq!(
        ReleasePlan::read(&files)?.release_plz_tool,
        "aqua:release-plz/release-plz"
    );
    assert_eq!(
        ReleasePlan::read(&crate::fixture::files())?.release_plz_tool,
        "release-plz"
    );
    Ok(())
}

#[test]
fn a_binary_without_a_buildable_bin_target_is_refused_before_anything_is_tagged() {
    let mut files = files();
    files.release_toml = Some("[[binaries]]\npackage = \"app\"\nbin = \"rg\"\n".to_owned());
    files.bin_targets = Some(BTreeSet::from([("app".to_owned(), "app".to_owned())]));
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::BinaryTargetMissing { package, bin }) if package == "app" && bin == "rg"),
        "{error:?}"
    );
    files.bin_targets = Some(BTreeSet::from([("app".to_owned(), "rg".to_owned())]));
    assert_eq!(refusal(&files), None);
}
