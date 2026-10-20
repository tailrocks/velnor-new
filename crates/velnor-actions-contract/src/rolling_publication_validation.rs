//! Closed structural checks for rolling observations.

use super::{PublicationIdentity, RollingAsset, RollingAssetState};
use std::collections::BTreeSet;

pub(super) fn valid_observation(expected: &PublicationIdentity, assets: &[RollingAsset]) -> bool {
    let ids: BTreeSet<_> = assets.iter().map(|item| item.id).collect();
    let names: BTreeSet<_> = assets.iter().map(|item| &item.name).collect();
    expected.assets.get("release.json") == Some(&expected.receipt_digest)
        && expected.assets.contains_key("release.json.bundle")
        && lower_hex(&expected.source_sha, 40)
        && lower_hex(&expected.receipt_digest, 64)
        && expected.assets.iter().all(|(name, digest)| {
            crate::package_release::valid_asset_name(name)
                && !name.starts_with("vlnr-")
                && lower_hex(digest, 64)
        })
        && ids.len() == assets.len()
        && names.len() == assets.len()
        && assets.iter().all(|item| {
            item.id > 0
                && match item.state {
                    RollingAssetState::Uploaded => {
                        item.size > 0
                            && item
                                .sha256
                                .as_deref()
                                .is_some_and(|value| lower_hex(value, 64))
                    }
                    RollingAssetState::Starter => item.size == 0 && item.sha256.is_none(),
                }
        })
}

pub(super) fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
