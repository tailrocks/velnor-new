// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    fixture_archive, make_fixture, prepare_crate_release, test_git, velnor_actions_orchestrator,
};

#[test]
pub(super) fn reconciliation_accepts_completed_metadata_repairs()
-> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::tempdir()?;
    let (intent, request, github, registry) = make_fixture(source.path())?;
    github.approved.set(true);
    registry.absent.set(false);
    *registry.archive.borrow_mut() = fixture_archive(source.path())?;
    let tag_receipt = prepare_crate_release(&intent, &request, &github, &registry, source.path())?;
    github.tag_exists.set(true);
    github.release_variant.set(4);
    let release_receipt =
        prepare_crate_release(&intent, &request, &github, &registry, source.path())?;
    github.release_variant.set(0);
    for receipt in [tag_receipt, release_receipt] {
        assert!(
            velnor_actions_orchestrator::crate_release_controller::validate_reconciliation_receipt(
                &receipt,
                &intent,
                &request,
                &github,
                &registry,
                crate::CrateSource::new(
                    source.path(),
                    test_git(),
                    &crate::UnprivilegedSourceExecution
                )
            )?
        );
    }
    Ok(())
}
