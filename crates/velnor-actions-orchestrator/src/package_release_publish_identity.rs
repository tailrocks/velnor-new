//! Canonical publication identity and draft/tag recovery-envelope helpers.

use std::{collections::BTreeMap, fs, path::Path};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use velnor_actions_contract::publication_retry::PublicationIdentity;

use crate::{
    OrchestratorError, internal::internal, package_release_signer::VerifiedAttestedSignedPackage,
};

use super::github::{GITHUB_ACTIONS_BOT_ID, ReleaseAuthor, sha256};

pub(super) const MAX_ASSET_BYTES: u64 = 1024 * 1024 * 1024;
pub(super) const MAX_RECOVERY_BODY_BYTES: usize = 110 * 1024;

pub(super) fn local_identity(
    signed: &VerifiedAttestedSignedPackage,
) -> Result<PublicationIdentity, OrchestratorError> {
    let receipt = signed.receipt();
    let mut assets: BTreeMap<String, String> = receipt
        .assets
        .iter()
        .map(|asset| (asset.name.clone(), asset.sha256.clone()))
        .collect();
    let receipt_bytes = read_regular(&signed.directory().join("release.json"))?;
    let canonical = receipt
        .canonical_bytes()
        .map_err(|_| internal("package_publish_receipt_encode"))?;
    if receipt_bytes != canonical {
        return Err(internal("package_publish_receipt_not_canonical"));
    }
    let receipt_digest = sha256(&receipt_bytes);
    assets.insert("release.json".to_owned(), receipt_digest.clone());
    let bundle = read_regular(&signed.directory().join("release.json.bundle"))?;
    assets.insert("release.json.bundle".to_owned(), sha256(&bundle));
    Ok(PublicationIdentity {
        source_sha: receipt.source.commit_sha.clone(),
        receipt_digest,
        assets,
    })
}

pub(super) fn recovery_body(
    signed: &VerifiedAttestedSignedPackage,
    receipt_digest: &str,
) -> Result<String, OrchestratorError> {
    let receipt = read_regular(&signed.directory().join("release.json"))?;
    let bundle = read_regular(&signed.directory().join("release.json.bundle"))?;
    make_recovery_body(&receipt, &bundle, receipt_digest)
}

pub(super) fn make_recovery_body(
    receipt: &[u8],
    bundle: &[u8],
    receipt_digest: &str,
) -> Result<String, OrchestratorError> {
    if sha256(receipt) != receipt_digest {
        return Err(internal("package_publish_recovery_receipt_digest"));
    }
    let body = format!(
        "velnor-package-release-recovery-v1\nreceipt_sha256={receipt_digest}\nreceipt={}\nbundle={}\n",
        URL_SAFE_NO_PAD.encode(receipt),
        URL_SAFE_NO_PAD.encode(bundle),
    );
    if body.len() > MAX_RECOVERY_BODY_BYTES {
        return Err(internal("package_publish_recovery_envelope_oversize"));
    }
    Ok(body)
}

pub(super) fn draft_receipt_digest(
    body: Option<&str>,
    author: Option<&ReleaseAuthor>,
    expected_body: &str,
    receipt_digest: &str,
) -> Option<String> {
    let (Some(body), Some(author)) = (body, author) else {
        return None;
    };
    if body == expected_body
        && author.login.as_deref() == Some("github-actions[bot]")
        && author.id == Some(GITHUB_ACTIONS_BOT_ID)
    {
        Some(receipt_digest.to_owned())
    } else {
        Some("untrusted-draft-envelope".to_owned())
    }
}

pub(super) fn tag_receipt_digest(
    message: Option<&str>,
    expected_message: &str,
    receipt_digest: &str,
) -> Option<String> {
    message.map(|message| {
        if message == expected_message {
            receipt_digest.to_owned()
        } else {
            "untrusted-tag-envelope".to_owned()
        }
    })
}

pub(super) fn combine_origin_identities(
    draft: Option<String>,
    tag: Option<String>,
) -> Option<String> {
    match (draft, tag) {
        (Some(draft), Some(tag)) if draft == tag => Some(draft),
        (None, None) => None,
        _ => Some("conflicting-origin-envelope".to_owned()),
    }
}

pub(super) fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn read_regular(path: &Path) -> Result<Vec<u8>, OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("package_publish_asset_missing"))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_ASSET_BYTES {
        return Err(internal("package_publish_asset_not_regular"));
    }
    fs::read(path).map_err(|_| internal("package_publish_asset_read"))
}
