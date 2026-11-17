// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::PRIVATE_VOLUME;
use super::{TestResult, run_helper_case};

#[test]
fn missing_foreign_id_preserves_owned_helper_and_reservation() -> TestResult<()> {
    let (observed, stderr) = run_helper_case("missing_id")?;
    assert_eq!(observed.helpers_left, 2, "{stderr}");
    assert_eq!(observed.volumes_left, 1, "{stderr}");
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
