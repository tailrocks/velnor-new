// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::super::TestResult;
use super::{CancelledObservation, HelperSpec, run_cancelled_before_start_scenario};

const CASE_ENV: &str = "VELNOR_HELPER_CASE";
const FOREIGN_ENGINE: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn helper_case(name: &str) -> Option<(Vec<HelperSpec>, bool)> {
    let publication = || HelperSpec::with_role("execution-publication", HelperSpec::probe().engine);
    let initializer = || HelperSpec::with_role("storage-initializer", None);
    let unmounted = |listed| HelperSpec {
        mounts: false,
        listed,
        ..HelperSpec::probe()
    };
    Some(match name {
        "all_roles" => (
            vec![HelperSpec::probe(), publication(), initializer()],
            true,
        ),
        "running" => (
            vec![HelperSpec {
                running: true,
                ..publication()
            }],
            true,
        ),
        "unlisted_unmounted" => (vec![unmounted(false), HelperSpec::probe()], true),
        "foreign_role" => (
            vec![HelperSpec::with_role("runner", None), HelperSpec::probe()],
            false,
        ),
        "listed_unmounted" => (vec![unmounted(true), HelperSpec::probe()], false),
        "foreign_engine" => (
            vec![
                HelperSpec::with_role("resource-probe", Some(FOREIGN_ENGINE)),
                HelperSpec::probe(),
            ],
            false,
        ),
        "over_bound" => (vec![HelperSpec::probe(); 5], false),
        _ => return None,
    })
}

#[test]
fn helper_case_child() -> TestResult<()> {
    let Ok(name) = std::env::var(CASE_ENV) else {
        return Ok(());
    };
    let (specs, expect_success) = helper_case(&name).ok_or("unknown helper case")?;
    let observed = run_cancelled_before_start_scenario(&specs, expect_success)?;
    println!("{}", observed.encode());
    Ok(())
}

fn run_helper_case(name: &str) -> TestResult<(CancelledObservation, String)> {
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            &format!(
                "{}::helper_case_child",
                module_path!().trim_start_matches("scaleset_installation_public::")
            ),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CASE_ENV, name)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let observed = CancelledObservation::decode(&stdout).ok_or_else(|| {
        format!(
            "child: {stdout} {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    Ok((
        observed,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

#[test]
fn public_cancelled_run_cleanup_removes_helper_containers_before_volumes() -> TestResult<()> {
    let (observed, _) = run_helper_case("all_roles")?;
    assert_eq!(observed.helpers_left, 0);
    assert_eq!(observed.volumes_left, 0);
    Ok(())
}

#[test]
fn public_cleanup_stops_a_running_helper_before_removing_it() -> TestResult<()> {
    let (observed, _) = run_helper_case("running")?;
    assert_eq!(observed.stops, 1);
    assert_eq!(observed.helpers_left, 0);
    assert_eq!(observed.volumes_left, 0);
    Ok(())
}

#[test]
fn public_cleanup_leaves_a_container_that_does_not_mount_the_volume() -> TestResult<()> {
    let (observed, _) = run_helper_case("unlisted_unmounted")?;
    assert_eq!(observed.unmounted_left, 1);
    assert_eq!(observed.helpers_left, 0);
    assert_eq!(observed.volumes_left, 0);
    Ok(())
}

fn assert_unowned_user_blocks_cleanup(case: &str) -> TestResult<()> {
    let (observed, stderr) = run_helper_case(case)?;
    assert!(
        stderr.contains("volume helper ownership"),
        "{case}: {stderr}"
    );
    assert_eq!(observed.helpers_left + observed.unmounted_left, 2, "{case}");
    assert_eq!(observed.volumes_left, 4, "{case}");
    Ok(())
}

#[test]
fn public_cleanup_fails_closed_on_a_foreign_role_volume_user() -> TestResult<()> {
    assert_unowned_user_blocks_cleanup("foreign_role")
}

#[test]
fn public_cleanup_fails_closed_on_a_listed_container_without_the_mount() -> TestResult<()> {
    assert_unowned_user_blocks_cleanup("listed_unmounted")
}

#[test]
fn public_cleanup_fails_closed_on_a_helper_of_another_engine() -> TestResult<()> {
    assert_unowned_user_blocks_cleanup("foreign_engine")
}

#[test]
fn public_cleanup_fails_closed_when_a_volume_has_too_many_users() -> TestResult<()> {
    let (observed, stderr) = run_helper_case("over_bound")?;
    assert!(stderr.contains("volume helper bound"), "{stderr}");
    assert_eq!(observed.helpers_left, 5);
    assert_eq!(observed.stops, 0);
    assert_eq!(observed.volumes_left, 4);
    Ok(())
}
