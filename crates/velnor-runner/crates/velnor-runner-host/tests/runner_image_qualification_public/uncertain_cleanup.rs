// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn public_qualifier_removes_volumes_only_after_uncertain_container_absence() -> TestResult<()> {
    for fault in [
        ApiFault::OwnedUnsettledDindCreate,
        ApiFault::OwnedUnsettledDindRemovalFailure,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.fault = Some(fault);
        plan.foreign_sentinel = true;
        let (output, api, _) =
            invoke_with_api(&git, git.source_sha(), plan, Duration::from_secs(24))?;
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
        if fault == ApiFault::OwnedUnsettledDindCreate {
            assert_eq!(state.containers.len(), 1);
            assert_eq!(state.containers[0].name, "foreign-sentinel");
            assert_eq!(state.volumes, ["foreign-sentinel"]);
        } else {
            assert_eq!(state.containers.len(), 2);
            assert!(state.containers.iter().any(|item| item.id == dind.id));
            assert_eq!(state.volumes.len(), 3);
            for mount in dind.host_config["Mounts"]
                .as_array()
                .ok_or("missing mounts")?
            {
                let volume = mount["Source"].as_str().ok_or("missing owned volume")?;
                assert!(state.volumes.iter().any(|name| name == volume));
                assert!(stderr.contains(&format!("recovery identity: volume {volume}")));
            }
        }
    }
    Ok(())
}
