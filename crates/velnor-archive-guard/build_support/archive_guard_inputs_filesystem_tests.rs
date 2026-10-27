// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{root_identity_matches, ufs};
use rustix::fs::AtFlags;
use std::error::Error;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;

#[test]
fn root_identity_comparison_accepts_sibling_churn_only_for_nonfinal_ancestors()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;

    let nonfinal_path = temp.path().join("nonfinal");
    fs::create_dir(&nonfinal_path)?;
    let parent = File::open(temp.path())?;
    let nonfinal = File::open(&nonfinal_path)?;
    let nonfinal_before = ufs::fstat(&nonfinal)?;
    fs::create_dir(nonfinal_path.join("sibling"))?;
    let nonfinal_after = ufs::statat(&parent, c"nonfinal", AtFlags::SYMLINK_NOFOLLOW)?;
    assert!(
        root_identity_matches(&nonfinal_before, &nonfinal_after, false),
        "nonfinal ancestor rejected unrelated sibling churn"
    );

    let final_path = temp.path().join("final");
    fs::create_dir(&final_path)?;
    let final_root = File::open(&final_path)?;
    let final_before = ufs::fstat(&final_root)?;
    fs::create_dir(final_path.join("sibling"))?;
    let final_after = ufs::statat(&parent, c"final", AtFlags::SYMLINK_NOFOLLOW)?;
    assert!(
        !root_identity_matches(&final_before, &final_after, true),
        "final checkout root accepted sibling churn"
    );

    let ancestor_path = temp.path().join("ancestor");
    let replaced_path = temp.path().join("ancestor-replaced");
    fs::create_dir(&ancestor_path)?;
    let ancestor = File::open(&ancestor_path)?;
    let ancestor_before = ufs::fstat(&ancestor)?;
    fs::rename(&ancestor_path, &replaced_path)?;
    fs::create_dir(&ancestor_path)?;
    let ancestor_after = ufs::statat(&parent, c"ancestor", AtFlags::SYMLINK_NOFOLLOW)?;
    assert!(
        !root_identity_matches(&ancestor_before, &ancestor_after, false),
        "nonfinal ancestor replacement retained its identity"
    );

    let chmod_path = temp.path().join("chmod");
    fs::create_dir(&chmod_path)?;
    fs::set_permissions(&chmod_path, fs::Permissions::from_mode(0o700))?;
    let chmod = File::open(&chmod_path)?;
    let chmod_before = ufs::fstat(&chmod)?;
    fs::set_permissions(&chmod_path, fs::Permissions::from_mode(0o750))?;
    let chmod_after = ufs::statat(&parent, c"chmod", AtFlags::SYMLINK_NOFOLLOW)?;
    assert!(
        !root_identity_matches(&chmod_before, &chmod_after, false),
        "nonfinal ancestor accepted a mode change"
    );

    Ok(())
}
