//! Explicit first-adoption authority for an existing legacy rolling release.
//!
//! A fingerprint identifies reviewed bytes and server object IDs. It grants no
//! authority to reset native history or to admit later unrelated uploads.

use super::{RollingAsset, RollingAssetState};
use crate::{ContractError, canonical_json_bytes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Reviewed legacy release snapshot; every tuple is independently observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollingBootstrapSnapshot {
    /// Fixed versioned domain separator.
    pub schema: String,
    /// Configured existing release database ID.
    pub release_id: u64,
    /// Literal rolling tag, always `preview`.
    pub tag: String,
    /// Actual peeled Git reference, never release target metadata.
    pub tag_source: String,
    /// Complete exact legacy asset inventory, including API state and digest.
    pub assets: Vec<RollingAsset>,
}

impl RollingBootstrapSnapshot {
    /// Hash a validated canonical snapshot independent of API enumeration order.
    /// # Errors
    /// Rejects malformed identities, duplicate objects or unknown upload state.
    pub fn sha256(&self) -> Result<String, ContractError> {
        self.validate()?;
        let mut snapshot = self.clone();
        snapshot
            .assets
            .sort_by(|left, right| (left.id, &left.name).cmp(&(right.id, &right.name)));
        Ok(crate::canonical::sha256_hex(&canonical_json_bytes(
            &snapshot,
        )?))
    }

    fn validate(&self) -> Result<(), ContractError> {
        let ids: BTreeSet<_> = self.assets.iter().map(|asset| asset.id).collect();
        let names: BTreeSet<_> = self.assets.iter().map(|asset| &asset.name).collect();
        if self.schema != "velnor.rolling-preview-bootstrap.v1"
            || self.release_id == 0
            || self.tag != "preview"
            || !super::validation::lower_hex(&self.tag_source, 40)
            || self.assets.len() > 4096
            || ids.len() != self.assets.len()
            || names.len() != self.assets.len()
            || self.assets.iter().any(|asset| {
                asset.id == 0
                    || asset.name.is_empty()
                    || asset.name.len() > 255
                    || asset.name.contains(['/', '\\'])
                    || match asset.state {
                        RollingAssetState::Uploaded => {
                            asset.size == 0
                                || !asset
                                    .sha256
                                    .as_deref()
                                    .is_some_and(|digest| super::validation::lower_hex(digest, 64))
                        }
                        RollingAssetState::Starter => asset.size != 0 || asset.sha256.is_some(),
                    }
            })
        {
            return Err(ContractError::identity(
                "rolling_bootstrap",
                "invalid_snapshot",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> RollingBootstrapSnapshot {
        RollingBootstrapSnapshot {
            schema: "velnor.rolling-preview-bootstrap.v1".to_owned(),
            release_id: 123,
            tag: "preview".to_owned(),
            tag_source: "a".repeat(40),
            assets: vec![
                RollingAsset {
                    id: 1,
                    name: "legacy.tar.gz".to_owned(),
                    state: RollingAssetState::Uploaded,
                    size: 200,
                    sha256: Some("b".repeat(64)),
                },
                RollingAsset {
                    id: 2,
                    name: "legacy.zip".to_owned(),
                    state: RollingAssetState::Uploaded,
                    size: 300,
                    sha256: Some("c".repeat(64)),
                },
            ],
        }
    }
    #[test]
    fn order_independent_but_every_identity_is_bound() {
        let original = snapshot();
        let digest = original.sha256().expect("snapshot");
        let mut changed = original.clone();
        changed.assets.reverse();
        assert_eq!(changed.sha256().expect("permutation"), digest);
        let mut changed = original.clone();
        changed.release_id += 1;
        assert_ne!(changed.sha256().expect("release"), digest);
        let mut changed = original.clone();
        changed.tag_source = "d".repeat(40);
        assert_ne!(changed.sha256().expect("source"), digest);
        for index in 0..2 {
            let mut changed = original.clone();
            changed.assets[index].id += 10;
            assert_ne!(changed.sha256().expect("ID"), digest);
            let mut changed = original.clone();
            changed.assets[index].name.push_str(".old");
            assert_ne!(changed.sha256().expect("name"), digest);
            let mut changed = original.clone();
            changed.assets[index].size += 1;
            assert_ne!(changed.sha256().expect("size"), digest);
            let mut changed = original.clone();
            changed.assets[index].sha256 = Some("e".repeat(64));
            assert_ne!(changed.sha256().expect("digest"), digest);
        }
    }
    #[test]
    fn invalid_snapshots_cannot_authorize_deletion() {
        let mut changed = snapshot();
        changed.assets.push(changed.assets[0].clone());
        assert!(changed.sha256().is_err());
        let mut changed = snapshot();
        changed.tag = "v1.2.3".to_owned();
        assert!(changed.sha256().is_err());
        let mut changed = snapshot();
        changed.assets[0].sha256 = None;
        assert!(changed.sha256().is_err());
        let mut changed = snapshot();
        changed.assets[0].state = RollingAssetState::Starter;
        assert!(changed.sha256().is_err());
    }
}
