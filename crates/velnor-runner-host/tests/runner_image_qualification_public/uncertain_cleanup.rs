// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn public_qualifier_removes_observed_owned_dind_after_uncertain_create() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.fault = Some(ApiFault::OwnedUnsettledDindCreate);
    plan.foreign_sentinel = true;
    let (output, api, _) = invoke_with_api(&git, git.source_sha(), plan, Duration::from_secs(24))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
    assert!(
        stderr.contains("container create state is uncertain"),
        "{stderr}"
    );
    let state = api.snapshot()?;
    assert!(state.fault_reached);
    assert_eq!(state.pending_mutations, 1);
    assert!(state.foreign_container_present && state.foreign_volume_present);
    assert_eq!(state.images.len(), 2);
    assert!(state.images.contains(&digest_id('a')));
    assert!(state.images.contains(&digest_id('b')));
    let dind = state
        .created
        .iter()
        .find(|item| item.role == "dind")
        .ok_or("owned DinD container was not created")?;
    assert!(stderr.contains(&format!("recovery identity: container {}", dind.name)));
    assert!(state.containers.iter().all(|item| item.id != dind.id));
    assert_eq!(state.containers.len(), 1);
    assert_eq!(state.volumes, ["foreign-sentinel"]);
    let mounts = dind.host_config["Mounts"]
        .as_array()
        .ok_or("missing mounts")?;
    assert_eq!(mounts.len(), 2);
    let mut owned = mounts
        .iter()
        .map(|mount| mount["Source"].as_str().ok_or("missing owned volume"))
        .collect::<Result<Vec<_>, _>>()?;
    owned.sort_unstable();
    let mut attempts = state.volume_delete_attempts.clone();
    attempts.sort_unstable();
    assert_eq!(
        attempts, owned,
        "each owned volume is deleted once, after its container"
    );
    Ok(())
}

#[test]
fn public_qualifier_retains_resources_when_acknowledged_container_delete_fails() -> TestResult<()> {
    for fault in [
        ApiFault::KnownDindRemovalFailure,
        ApiFault::KnownDindRemovalBadRequest,
        ApiFault::KnownDindRemovalConflict,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        plan.foreign_sentinel = true;
        let (output, api, _) =
            invoke_with_api(&git, git.source_sha(), plan, Duration::from_secs(24))?;
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
        let state = api.snapshot()?;
        assert!(state.foreign_container_present && state.foreign_volume_present);
        let dind = state
            .created
            .iter()
            .find(|item| item.role == "dind")
            .ok_or("missing acknowledged DinD")?;
        assert_eq!(state.failed_delete_id.as_deref(), Some(dind.id.as_str()));
        assert!(state.containers.iter().any(|item| item.id == dind.id));
        assert_eq!(state.volumes.len(), 3);
        assert!(
            state.volume_delete_attempts.is_empty(),
            "attempted volume deletion after failed container removal"
        );
        for mount in dind.host_config["Mounts"]
            .as_array()
            .ok_or("missing mounts")?
        {
            let volume = mount["Source"].as_str().ok_or("missing owned volume")?;
            assert!(state.volumes.iter().any(|name| name == volume));
        }
    }
    Ok(())
}
