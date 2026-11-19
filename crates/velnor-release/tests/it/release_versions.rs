// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{Versions, check_release_versions};

use crate::fixture::{PLZ, metadata};

fn check(plz: &str, versions: &[(&str, &str)]) -> Result<(), String> {
    let members: &[(&str, &[(&str, &str)])] = &[(
        "Cargo.toml",
        &[("app", "Cargo.toml"), ("lib", "lib/Cargo.toml")],
    )];
    let versions: Versions = versions
        .iter()
        .map(|(name, version)| {
            (
                ("Cargo.toml".to_owned(), (*name).to_owned()),
                (*version).to_owned(),
            )
        })
        .collect();
    check_release_versions(
        &[("release-plz.toml".to_owned(), plz.to_owned())],
        &metadata(members),
        &versions,
    )
}

#[test]
fn a_listed_or_inherited_package_whose_version_ends_the_tag_in_lock_is_refused() {
    for lock in [("app", "1.0.0"), ("lib", "1.0.0")] {
        let mut versions = vec![("app", "1.0.0"), ("lib", "1.0.0")];
        versions.retain(|(name, _)| *name != lock.0);
        versions.push((lock.0, "2.0.0-rc.lock"));
        let error = check(PLZ, &versions).err().unwrap_or_default();
        assert!(
            error.contains(&format!("{}-v2.0.0-rc.lock", lock.0)),
            "{error}"
        );
    }
    assert_eq!(check(PLZ, &[("app", "1.0.0"), ("lib", "2.0.0-rc")]), Ok(()));
}

#[test]
fn a_package_without_a_known_version_is_refused() {
    assert!(check(PLZ, &[("app", "1.0.0")]).is_err());
}
