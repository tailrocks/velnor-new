// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::setup_action::{Install, TestResult, program};
use super::setup_action_tool_cache::seed_entry;

fn shim(install: &Install, name: &str, body: &str) -> Result<std::path::PathBuf, std::io::Error> {
    let shims = install.work.join("shims");
    fs::create_dir_all(&shims)?;
    let path = shims.join(name);
    fs::write(&path, body)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(shims)
}

#[test]
fn a_sha256sum_without_gnu_long_options_still_verifies_the_download() -> TestResult {
    let install = Install::new("sha256sum-busybox")?;
    let real = ["/usr/bin/sha256sum", "/bin/sha256sum"]
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
        .map_or_else(|| format!("{} -a 256", program("shasum")), str::to_owned);
    let shims = shim(
        &install,
        "sha256sum",
        &format!(
            "#!/bin/sh\ncase \"$*\" in\n  *--*) echo \"sha256sum: unrecognized option\" >&2; exit 1 ;;\nesac\nexec {real} \"$@\"\n"
        ),
    )?;
    let status = install.run(None, Some(&shims))?;
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert_eq!(copy?, install.binary);
    Ok(())
}

#[test]
fn an_oversized_cache_entry_is_not_copied() -> TestResult {
    let install = Install::new("cache-oversized")?;
    let (cache, entry) = seed_entry(&install)?;
    let file = fs::File::create(&entry)?;
    file.set_len(300 * 1024 * 1024)?;
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))?;
    let marker = install.work.join("copied-cache-entry");
    let shims = shim(
        &install,
        "cp",
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  {}) : > {} ;;\nesac\nexec /bin/cp \"$@\"\n",
            entry.display(),
            marker.display()
        ),
    )?;
    let status = install.run(Some(&cache), Some(&shims))?;
    let copied = marker.exists();
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert!(!copied, "the oversized entry was copied");
    assert_eq!(copy?, install.binary);
    Ok(())
}
