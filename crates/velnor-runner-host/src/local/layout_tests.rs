// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;

use bollard::models::MountType;

use super::{local_ci_archive, local_ci_mount};

#[test]
fn the_local_ci_archive_holds_exactly_a_root_owned_read_only_directory_and_binary()
-> Result<(), Box<dyn std::error::Error>> {
    let binary = b"\x7fELF-binary".to_vec();
    let archive = local_ci_archive(&binary)?;
    let mut entries = Vec::new();
    for entry in tar::Archive::new(archive.as_slice()).entries()? {
        let mut entry = entry?;
        let header = entry.header().clone();
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents)?;
        entries.push((
            entry.path()?.display().to_string(),
            header.uid()?,
            header.gid()?,
            header.mode()?,
            header.entry_type().is_dir(),
            contents,
        ));
    }
    assert_eq!(
        entries,
        [
            (
                "metadata/local-ci/".to_owned(),
                0,
                0,
                0o555,
                true,
                Vec::new()
            ),
            (
                "metadata/local-ci/velnor-ci".to_owned(),
                0,
                0,
                0o555,
                false,
                binary
            ),
        ]
    );
    Ok(())
}

#[test]
fn the_default_metadata_archive_has_no_local_ci_directory() -> Result<(), Box<dyn std::error::Error>>
{
    let archive = super::metadata_archive()?;
    for entry in tar::Archive::new(archive.as_slice()).entries()? {
        let path = entry?.path()?.display().to_string();
        assert!(!path.contains("local"), "{path}");
    }
    Ok(())
}

#[test]
fn the_local_ci_mount_is_a_read_only_subpath_volume_mount() {
    let mount = local_ci_mount("the-metadata-volume".to_owned());
    assert_eq!(mount.typ, Some(MountType::VOLUME));
    assert_eq!(mount.source.as_deref(), Some("the-metadata-volume"));
    assert_eq!(mount.target.as_deref(), Some("/opt/velnor/local"));
    assert_eq!(mount.read_only, Some(true));
    let options = mount.volume_options.expect("options");
    assert_eq!(options.subpath.as_deref(), Some("local-ci"));
    assert_eq!(options.no_copy, Some(true));
}
