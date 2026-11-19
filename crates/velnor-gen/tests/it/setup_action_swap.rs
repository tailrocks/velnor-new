// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

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
    fs::write(&entry, install.binary)?;
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))?;
    let shims = install.work.join("shims");
    fs::create_dir_all(&shims)?;
    for name in ["cp", "head", "cat", "dd", "wc"] {
        let shim = shims.join(name);
        fs::write(&shim, SWAP_SHIM)?;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755))?;
    }
    let mut child = install.spawn_with(Some(&cache), Some(&shims), |command| {
        command
            .env("ENTRY", &entry)
            .env("SHIMS", &shims)
            .process_group(0);
    })?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() > deadline {
            Command::new(crate::setup_action::program("kill"))
                .args(["-KILL", "--", &format!("-{}", child.id())])
                .status()?;
            child.wait()?;
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(
        status.is_some_and(|status| status.success()),
        "the install hung or failed"
    );
    assert_eq!(copy?, install.binary);
    Ok(())
}
