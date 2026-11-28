// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::{HostError, RustRunnerFailure};

use super::support::Fixture;

type Check = Result<(), Box<dyn Error>>;

const ENTRIES: [&str; 4] = [
    "var/lib/velnor/cache",
    "var/lib/velnor/bin",
    "var/lib/velnor/locks",
    "srv/velnor/gen/work",
];

fn replace_with_victim_link(
    fixture: &Fixture,
    entry: &str,
) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let victim = fixture.root.join("victim");
    std::fs::create_dir_all(&victim)?;
    std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o700))?;
    let path = fixture.guest(entry);
    std::fs::remove_dir_all(&path)?;
    std::os::unix::fs::symlink(&victim, &path)?;
    Ok(victim)
}

#[test]
fn rust_runner_refuses_a_store_entry_that_is_not_a_directory_and_changes_nothing() -> Check {
    let mut accepted = Vec::new();
    for entry in ENTRIES {
        let fixture = Fixture::ready()?;
        fixture.launcher().ensure_machine()?;
        let victim = replace_with_victim_link(&fixture, entry)?;
        let installs = fixture.commands("install")?;
        let refused = fixture.launcher().ensure_machine().err()
            == Some(HostError::RustRunner(RustRunnerFailure::MachineLayout));
        let untouched = std::fs::metadata(&victim)?.permissions().mode() & 0o7777 == 0o700
            && fixture.commands("install")? == installs;
        if !(refused && untouched) {
            accepted.push(format!("{entry}: refused {refused}, untouched {untouched}"));
        }
    }
    let filed = Fixture::ready()?;
    filed.launcher().ensure_machine()?;
    let path = filed.guest("var/lib/velnor/git");
    std::fs::remove_dir_all(&path)?;
    std::fs::write(&path, "not a directory")?;
    let installs = filed.commands("install")?;
    if filed.launcher().ensure_machine().err()
        != Some(HostError::RustRunner(RustRunnerFailure::MachineLayout))
        || filed.commands("install")? != installs
    {
        accepted.push("a file".to_owned());
    }
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
    Ok(())
}
