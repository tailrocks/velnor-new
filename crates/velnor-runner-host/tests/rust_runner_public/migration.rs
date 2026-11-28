// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::{HostError, ReservationId, RustRunnerFailure};

use super::support::Fixture;

type Check = Result<(), Box<dyn Error>>;

const OBSOLETE: [&str; 10] = [
    "bin", "crates", "tools", "git", "gen", "cache", "records", "work", "jit", "trusted",
];

fn ready() -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::ready()?;
    fixture.launcher().ensure_machine()?;
    Ok(fixture)
}

fn swept(fixture: &Fixture) -> Result<Vec<u64>, Box<dyn Error>> {
    Ok(fixture
        .launcher()
        .sweep()?
        .into_iter()
        .map(ReservationId::get)
        .collect())
}

fn plant(fixture: &Fixture, name: &str) -> Check {
    let deep = fixture.store.join(name).join("inner/deeper");
    std::fs::create_dir_all(&deep)?;
    std::fs::write(deep.join("file"), "stale")?;
    std::fs::set_permissions(deep, std::fs::Permissions::from_mode(0o500))?;
    std::fs::set_permissions(
        fixture.store.join(name),
        std::fs::Permissions::from_mode(0o500),
    )?;
    Ok(())
}

#[test]
fn rust_runner_sweep_removes_the_trees_of_the_old_host_store() -> Check {
    let fixture = ready()?;
    let victim = fixture.root.join("victim");
    std::fs::create_dir(&victim)?;
    std::fs::write(victim.join("keep"), "")?;
    for name in OBSOLETE {
        if name == "cache" {
            std::os::unix::fs::symlink(&victim, fixture.store.join(name))?;
        } else {
            plant(&fixture, name)?;
        }
    }
    std::fs::write(fixture.store.join("other"), "kept")?;
    assert_eq!(swept(&fixture)?, Vec::<u64>::new());
    let left: Vec<_> = OBSOLETE
        .iter()
        .filter(|name| std::fs::symlink_metadata(fixture.store.join(name)).is_ok())
        .collect();
    assert!(left.is_empty(), "left behind: {left:?}");
    assert!(victim.join("keep").exists(), "a link was followed");
    assert!(fixture.store.join("other").exists());
    assert_eq!(swept(&fixture)?, Vec::<u64>::new());
    Ok(())
}

fn job_dir(fixture: &Fixture, name: &str) -> Check {
    for dir in ["work", "stage"] {
        std::fs::create_dir_all(fixture.guest(&format!("srv/velnor/gen/{dir}/{name}/data")))?;
    }
    Ok(())
}

fn survives(fixture: &Fixture, name: &str) -> bool {
    ["work", "stage"].iter().any(|dir| {
        fixture
            .guest(&format!("srv/velnor/gen/{dir}/{name}"))
            .exists()
    })
}

#[test]
fn rust_runner_sweep_clears_the_read_only_flag_of_a_subvolume_before_removing_it() -> Check {
    let fixture = ready()?;
    job_dir(&fixture, "9")?;
    fixture.configure("FAKE_GUEST_READONLY=9\n")?;
    assert_eq!(swept(&fixture)?, Vec::<u64>::new());
    assert!(!survives(&fixture, "9"));
    Ok(())
}

#[test]
fn rust_runner_sweep_reports_a_job_dir_it_cannot_remove_instead_of_failing() -> Check {
    let fixture = ready()?;
    job_dir(&fixture, "9")?;
    job_dir(&fixture, "12")?;
    fixture.configure("FAKE_GUEST_WEDGE=1\n")?;
    assert_eq!(swept(&fixture)?, [9, 12]);
    assert!(survives(&fixture, "9") && survives(&fixture, "12"));
    fixture.configure("")?;
    assert_eq!(swept(&fixture)?, Vec::<u64>::new());
    assert!(!survives(&fixture, "9") && !survives(&fixture, "12"));
    Ok(())
}

#[test]
fn rust_runner_sweep_refuses_a_leftover_that_is_not_a_reservation() -> Check {
    let fixture = ready()?;
    job_dir(&fixture, "not-a-reservation")?;
    fixture.configure("FAKE_GUEST_WEDGE=1\n")?;
    assert_eq!(
        fixture.launcher().sweep().err(),
        Some(HostError::RustRunner(RustRunnerFailure::JobDirsWedged))
    );
    Ok(())
}

#[test]
fn rust_runner_sweep_empties_the_job_dirs_inside_the_machine() -> Check {
    let fixture = ready()?;
    for dir in ["work", "stage"] {
        let locked = fixture.guest(&format!("srv/velnor/gen/{dir}/9/locked"));
        std::fs::create_dir_all(locked.join("inner"))?;
        std::fs::write(locked.join("keep"), "")?;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500))?;
        std::fs::write(fixture.guest(&format!("srv/velnor/gen/{dir}/.hidden")), "")?;
    }
    assert_eq!(swept(&fixture)?, Vec::<u64>::new());
    for dir in ["work", "stage"] {
        let left = std::fs::read_dir(fixture.guest(&format!("srv/velnor/gen/{dir}")))?.count();
        assert_eq!(left, 0, "{dir}");
    }
    Ok(())
}
