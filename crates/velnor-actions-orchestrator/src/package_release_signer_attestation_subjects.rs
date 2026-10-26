// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::package_release::PackageReleaseReceipt;

use super::{OrchestratorError, sha256};
use crate::internal::internal;

pub(super) fn expected_subjects(
    receipt: &PackageReleaseReceipt,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut subjects = BTreeMap::new();
    for asset in &receipt.assets {
        if matches!(
            asset.kind,
            velnor_actions_contract::package_release::PackageReleaseAssetKind::Payload
                | velnor_actions_contract::package_release::PackageReleaseAssetKind::Companion
        ) {
            subjects.insert(asset.name.clone(), asset.sha256.clone());
        }
    }
    let receipt_bytes = receipt
        .canonical_bytes()
        .map_err(|_| internal("package_attestation_receipt_encode"))?;
    subjects.insert("release.json".to_owned(), sha256(&receipt_bytes));
    let declared = receipt
        .evidence
        .attested_subject_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if subjects.keys().cloned().collect::<BTreeSet<_>>() != declared {
        return Err(internal("package_attestation_subject_inventory_invalid"));
    }
    Ok(subjects)
}
