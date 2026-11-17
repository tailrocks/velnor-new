// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::super::TestResult;
use super::helper_stub::Life;
use super::{CancelledObservation, HelperSpec, Kind, run_cancelled_before_start_scenario};

#[path = "helper_cases_review.rs"]
mod review;

const CASE_ENV: &str = "VELNOR_HELPER_CASE";
const FOREIGN_ENGINE: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn helper_case(name: &str) -> Option<(Vec<HelperSpec>, bool)> {
    if let Some(case) = review::case(name) {
        return Some(case);
    }
    let publication = HelperSpec::of(Kind::Publication);
    let initializer = HelperSpec::of(Kind::Initializer);
    let foreign = |role| HelperSpec::of(Kind::Foreign(role));
    let unmounted = |listed| HelperSpec {
        mounts: false,
        listed,
        ..foreign("resource-probe")
    };
    Some(match name {
        "publication" => (vec![publication], true),
        "initializer" => (vec![initializer], true),
        "initializer_already_removed" => (
            vec![HelperSpec {
                life: Life::Gone,
                ..initializer
            }],
            true,
        ),
        "initializer_volumes_absent" => (
            vec![HelperSpec {
                life: Life::Gone,
                volumes_present: false,
                ..initializer
            }],
            true,
        ),
        "initializer_volumes_absent_container_present" => (
            vec![HelperSpec {
                volumes_present: false,
                ..initializer
            }],
            true,
        ),
        "initializer_never_listed" => (
            vec![HelperSpec {
                mounts: false,
                listed: false,
                life: Life::Running,
                ..initializer
            }],
            true,
        ),
        "initializer_running" => (
            vec![HelperSpec {
                life: Life::Running,
                ..initializer
            }],
            true,
        ),
        "unlisted_unmounted" => (vec![unmounted(false), publication], true),
        "lookalike_publication" => (vec![foreign("execution-publication")], false),
        "lookalike_initializer" => (vec![foreign("storage-initializer")], false),
        "foreign_beside_journaled" => (vec![publication, foreign("resource-probe")], false),
        "journaled_foreign_engine" => (
            vec![HelperSpec {
                engine: Some(FOREIGN_ENGINE),
                ..publication
            }],
            false,
        ),
        "initializer_foreign_operation" => (
            vec![HelperSpec {
                operation: Some("9-9"),
                ..initializer
            }],
            false,
        ),
        "listed_unmounted" => (vec![unmounted(true)], false),
        "over_bound" => (vec![foreign("resource-probe"); 5], false),
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
fn public_cancelled_run_retires_a_journaled_publication_helper_before_removing_volumes()
-> TestResult<()> {
    let (observed, stderr) = run_helper_case("publication")?;
    assert_eq!(observed.helpers_left, 0, "{stderr}");
    assert_eq!(observed.volumes_left, 0, "{stderr}");
    assert_eq!(observed.phase, "removed", "{stderr}");
    assert_eq!(observed.lease, "released", "{stderr}");
    assert!(!stderr.contains("UNROUTED"), "{stderr}");
    Ok(())
}

#[test]
fn public_cancelled_run_retires_a_journaled_storage_initializer_before_removing_volumes()
-> TestResult<()> {
    let (observed, stderr) = run_helper_case("initializer")?;
    assert_eq!(observed.helpers_left, 0, "{stderr}");
    assert_eq!(observed.volumes_left, 0, "{stderr}");
    assert_eq!(observed.initializer, "removed/not_requested", "{stderr}");
    assert!(!stderr.contains("UNROUTED"), "{stderr}");
    Ok(())
}

#[test]
fn public_cleanup_settles_an_initializer_whose_container_a_crash_already_removed() -> TestResult<()>
{
    let (observed, stderr) = run_helper_case("initializer_already_removed")?;
    assert_eq!(observed.helpers_left, 0, "{stderr}");
    assert_eq!(observed.volumes_left, 0, "{stderr}");
    assert_eq!(observed.initializer, "removed/not_requested", "{stderr}");
    Ok(())
}

#[test]
fn public_cleanup_settles_an_initializer_when_every_owned_volume_is_already_absent()
-> TestResult<()> {
    let (observed, stderr) = run_helper_case("initializer_volumes_absent")?;
    assert_eq!(observed.present_left, 0, "{stderr}");
    assert_eq!(observed.volumes_left, 0, "{stderr}");
    assert_eq!(observed.initializer, "removed/not_requested", "{stderr}");
    Ok(())
}

#[test]
fn public_cleanup_removes_a_journaled_initializer_when_every_owned_volume_is_already_absent()
-> TestResult<()> {
    let (observed, stderr) = run_helper_case("initializer_volumes_absent_container_present")?;
    assert_eq!(observed.present_left, 0, "{stderr}");
    assert_eq!(observed.stops, 0, "{stderr}");
    assert_eq!(observed.volumes_left, 0, "{stderr}");
    assert_eq!(observed.initializer, "removed/not_requested", "{stderr}");
    Ok(())
}

#[test]
fn public_cleanup_removes_a_journaled_initializer_that_was_never_a_volume_user() -> TestResult<()> {
    let (observed, stderr) = run_helper_case("initializer_never_listed")?;
    assert_eq!(observed.present_left, 0, "{stderr}");
    assert_eq!(observed.stops, 1, "{stderr}");
    assert_eq!(observed.volumes_left, 0, "{stderr}");
    assert_eq!(observed.initializer, "removed/not_requested", "{stderr}");
    Ok(())
}

#[test]
fn public_cleanup_stops_a_running_storage_initializer_before_removing_it() -> TestResult<()> {
    let (observed, stderr) = run_helper_case("initializer_running")?;
    assert_eq!(observed.stops, 1, "{stderr}");
    assert_eq!(observed.helpers_left, 0, "{stderr}");
    assert_eq!(observed.volumes_left, 0, "{stderr}");
    assert_eq!(observed.initializer, "removed/not_requested", "{stderr}");
    Ok(())
}

#[test]
fn public_cleanup_leaves_a_container_that_does_not_mount_the_volume() -> TestResult<()> {
    let (observed, stderr) = run_helper_case("unlisted_unmounted")?;
    assert_eq!(observed.unmounted_left, 1, "{stderr}");
    assert_eq!(observed.helpers_left, 0, "{stderr}");
    assert_eq!(observed.volumes_left, 0, "{stderr}");
    assert_eq!(observed.phase, "removed", "{stderr}");
    Ok(())
}

fn assert_fails_closed(
    case: &str,
    helpers: usize,
    volumes_left: usize,
) -> TestResult<CancelledObservation> {
    let (observed, stderr) = run_helper_case(case)?;
    assert!(
        stderr.contains("volume helper ownership"),
        "{case}: {stderr}"
    );
    assert_eq!(
        observed.helpers_left + observed.unmounted_left,
        helpers,
        "{case}: {stderr}"
    );
    assert_eq!(observed.volumes_left, volumes_left, "{case}: {stderr}");
    assert_eq!(observed.stops, 0, "{case}");
    Ok(observed)
}

#[test]
fn public_cleanup_fails_closed_on_a_label_lookalike_publication_helper() -> TestResult<()> {
    assert_fails_closed("lookalike_publication", 1, 1).map(drop)
}

#[test]
fn public_cleanup_fails_closed_on_a_label_lookalike_storage_initializer() -> TestResult<()> {
    assert_fails_closed("lookalike_initializer", 1, 1).map(drop)
}

#[test]
fn public_cleanup_deletes_nothing_when_a_foreign_container_sits_beside_a_journaled_helper()
-> TestResult<()> {
    let observed = assert_fails_closed("foreign_beside_journaled", 2, 1)?;
    assert_eq!(observed.phase, "container_created");
    assert_eq!(observed.lease, "active");
    Ok(())
}

#[test]
fn public_cleanup_fails_closed_on_a_journaled_publication_with_a_foreign_engine_label()
-> TestResult<()> {
    assert_fails_closed("journaled_foreign_engine", 1, 1).map(drop)
}

#[test]
fn public_cleanup_fails_closed_on_a_storage_initializer_with_a_foreign_operation_key()
-> TestResult<()> {
    assert_fails_closed("initializer_foreign_operation", 1, 4).map(drop)
}

#[test]
fn public_cleanup_fails_closed_on_a_listed_container_without_the_mount() -> TestResult<()> {
    assert_fails_closed("listed_unmounted", 1, 1).map(drop)
}

#[test]
fn public_cleanup_fails_closed_when_a_volume_has_too_many_users() -> TestResult<()> {
    let (observed, stderr) = run_helper_case("over_bound")?;
    assert!(stderr.contains("volume helper bound"), "{stderr}");
    assert_eq!(observed.helpers_left, 5);
    assert_eq!(observed.stops, 0);
    assert_eq!(observed.volumes_left, 1);
    Ok(())
}
