// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_release::ExpectedRelease;

use crate::error::CiError;
use crate::github::{GitHub, Repo, tag_ref_path};
use crate::release::PlainName;

pub trait Registry {
    fn is_published(&self, package: &str, version: &str) -> Result<bool, CiError>;
}

pub fn check_published_tagged(
    github: &dyn GitHub,
    repo: &Repo,
    registry: &dyn Registry,
    expected: &[ExpectedRelease],
    released: &BTreeMap<PlainName, PlainName>,
) -> Result<(), CiError> {
    for release in expected {
        let released_tag = PlainName::new("package", &release.package)
            .is_ok_and(|package| released.contains_key(&package));
        if released_tag
            || !release.publishes_to_crates_io
            || !registry.is_published(&release.package, &release.version)?
        {
            continue;
        }
        match github.get(&tag_ref_path(repo, &release.tag)) {
            Ok(_) => {}
            Err(error) if error.is_missing() => {
                let ExpectedRelease {
                    package,
                    version,
                    tag,
                    ..
                } = release;
                return Err(CiError::new(format!(
                    "{package} {version} is already on crates.io but the git tag {tag} does not exist, so release-plz skipped it and released nothing; to resolve, create the tag on the commit that was published (git tag {tag} <commit> && git push origin {tag}) and re-run, or bump the version so a new release is made"
                )));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
