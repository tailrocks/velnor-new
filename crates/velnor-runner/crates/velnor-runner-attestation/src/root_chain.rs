use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tough::schema::{Role, Root, Signed};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootIdentity {
    pub(crate) version: u64,
    pub(crate) signed_sha256: String,
}

impl RootIdentity {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let root: Signed<Root> =
            serde_json::from_slice(bytes).map_err(|_| "root metadata parse failed")?;
        Self::from_signed(&root)
    }

    pub(crate) fn from_signed(root: &Signed<Root>) -> Result<Self, String> {
        let canonical = root
            .signed
            .canonical_form()
            .map_err(|_| "root canonicalization failed")?;
        Ok(Self {
            version: root.signed.version.get(),
            signed_sha256: lower_hex(&Sha256::digest(canonical)),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedRootChain {
    pub(crate) identities: Vec<RootIdentity>,
    pub(crate) final_identity: RootIdentity,
}

pub(crate) fn verify_composite_chain(
    starting_root: &RootIdentity,
    captured_root_responses: &[Vec<u8>],
    repository_root: &Signed<Root>,
) -> Result<VerifiedRootChain, String> {
    let mut identities = vec![starting_root.clone()];
    let mut seen = BTreeMap::from([(starting_root.version, starting_root.signed_sha256.clone())]);

    for response in captured_root_responses {
        let identity = RootIdentity::from_bytes(response)?;
        if let Some(previous) = seen.get(&identity.version) {
            if previous != &identity.signed_sha256 {
                return Err("root version equivocation".to_owned());
            }
            return Err("duplicate root version response".to_owned());
        }
        let expected = identities
            .last()
            .and_then(|last| last.version.checked_add(1))
            .ok_or_else(|| "root version overflow".to_owned())?;
        if identity.version != expected {
            return Err("root chain is not contiguous".to_owned());
        }
        seen.insert(identity.version, identity.signed_sha256.clone());
        identities.push(identity);
    }

    let final_identity = RootIdentity::from_signed(repository_root)
        .map_err(|_| "repository root identity failed")?;
    if identities.last() != Some(&final_identity) {
        return Err("Tough final root does not match captured chain".to_owned());
    }

    Ok(VerifiedRootChain {
        identities,
        final_identity,
    })
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}
