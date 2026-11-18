// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::PRIVATE_VOLUME;
use super::super::helper_stub::{Life, ListedId};
use super::{HelperSpec, Kind, TestResult, run_helper_case};

pub(super) fn case(name: &str) -> Option<(Vec<HelperSpec>, bool)> {
    let foreign = HelperSpec::of(Kind::Foreign("resource-probe"));
    match name {
        "publication_already_removed" => Some((
            vec![HelperSpec {
                life: Life::Gone,
                ..HelperSpec::of(Kind::Publication)
            }],
            true,
        )),
        "missing_id" => Some((
            vec![
                HelperSpec::of(Kind::Publication),
                HelperSpec {
                    listed_id: ListedId::Missing,
                    ..foreign
                },
            ],
            false,
        )),
        "volume_error" => Some((
            vec![HelperSpec {
                listed: false,
                ..foreign
            }],
            false,
        )),
        _ => None,
    }
}

#[test]
fn missing_foreign_id_preserves_owned_helper_and_reservation() -> TestResult<()> {
    let (observed, stderr) = run_helper_case("missing_id")?;
    assert_eq!(observed.helpers_left, 2, "{stderr}");
    assert_eq!(observed.volumes_left, 5, "{stderr}");
    assert_eq!(observed.phase, "container_created", "{stderr}");
    assert_eq!(observed.lease, "active", "{stderr}");
    assert_eq!(observed.reservation, "uncertain", "{stderr}");
    Ok(())
}

#[test]
fn volume_error_reports_phase_name_and_docker_message() -> TestResult<()> {
    let (_, stderr) = run_helper_case("volume_error")?;
    let volume = format!("{PRIVATE_VOLUME}-metadata");
    assert!(
        stderr.contains(&format!(
            "completion cleanup docker error phase=volume remove object={volume} error="
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("remove {volume}: volume is in use")),
        "{stderr}"
    );
    Ok(())
}
