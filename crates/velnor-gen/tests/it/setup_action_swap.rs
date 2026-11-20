// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use crate::setup_action::Install;
use crate::setup_action_tool_cache::seed_entry;

type TestResult = Result<(), Box<dyn Error>>;

const SWAP_SHIM: &str = "#!/bin/sh\n\
for arg in \"$@\"; do\n\
  if [ \"$arg\" = \"$ENTRY\" ] && [ -f \"$ENTRY\" ]; then rm -f \"$ENTRY\"; mkfifo \"$ENTRY\"; fi\n\
done\n\
PATH=\"${PATH#\"$SHIMS\":}\"\n\
exec \"$(basename \"$0\")\" \"$@\"\n";

#[test]
fn a_cache_entry_swapped_for_a_fifo_after_the_checks_cannot_hang_the_install() -> TestResult {
    let install = Install::new("cache-swap")?;
    let (cache, entry) = seed_entry(&install)?;
    fs::write(&entry, fs::read(install.archive_path())?)?;
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))?;
    let shims = install.work.join("shims");
    fs::create_dir_all(&shims)?;
    for name in ["cp", "head", "cat", "dd", "wc"] {
        let shim = shims.join(name);
        fs::write(&shim, SWAP_SHIM)?;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755))?;
    }
    let status = install
        .spawn_with(Some(&cache), Some(&shims), |command| {
            command.env("ENTRY", &entry).env("SHIMS", &shims);
        })?
        .wait_within(Duration::from_secs(30))?;
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(
        status.is_some_and(|status| status.success()),
        "the install hung or failed"
    );
    assert_eq!(copy?, install.binary);
    Ok(())
}
