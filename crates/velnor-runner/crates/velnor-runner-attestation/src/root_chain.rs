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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::num::NonZeroU64;
    use std::path::Path;

    use tough::schema::{Root, Signed};

    use super::{RootIdentity, verify_composite_chain};

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/tough/rotated-root")
                .join(name),
        )
        .expect("read Tough root fixture")
    }

    fn signed(bytes: &[u8]) -> Signed<Root> {
        serde_json::from_slice(bytes).expect("parse signed root fixture")
    }

    #[test]
    fn empty_fetched_chain_requires_final_root_equal_saved_start() {
        let bytes = fixture("1.root.json");
        let root = signed(&bytes);
        let start = RootIdentity::from_signed(&root).expect("canonical root identity");
        let chain =
            verify_composite_chain(&start, &[], &root).expect("no-rotation chain uses saved root");
        assert_eq!(chain.identities, vec![start.clone()]);
        assert_eq!(chain.final_identity, start);

        let newer = signed(&fixture("2.root.json"));
        assert!(
            verify_composite_chain(
                &RootIdentity::from_signed(&root).expect("canonical root identity"),
                &[],
                &newer,
            )
            .is_err()
        );
    }

    #[test]
    fn valid_rotation_is_saved_root_plus_exact_next_response() {
        let first_bytes = fixture("1.root.json");
        let second_bytes = fixture("2.root.json");
        let first = signed(&first_bytes);
        let second = signed(&second_bytes);
        let start = RootIdentity::from_signed(&first).expect("canonical root identity");
        let chain = verify_composite_chain(&start, &[second_bytes], &second)
            .expect("contiguous signed rotation chain");
        assert_eq!(chain.identities.len(), 2);
        assert_eq!(chain.final_identity.version, 2);
    }

    #[test]
    fn gap_and_lower_root_are_rejected() {
        let first = signed(&fixture("1.root.json"));
        let start = RootIdentity::from_signed(&first).expect("canonical root identity");
        let mut third_value: serde_json::Value =
            serde_json::from_slice(&fixture("2.root.json")).expect("parse second root");
        third_value["signed"]["version"] = serde_json::Value::Number(3.into());
        let third_bytes = serde_json::to_vec(&third_value).expect("serialize third root");
        let third: Signed<Root> =
            serde_json::from_slice(&third_bytes).expect("parse synthetic third root");
        assert!(verify_composite_chain(&start, &[third_bytes], &third).is_err());
        assert!(verify_composite_chain(&start, &[fixture("1.root.json")], &first).is_err());
    }

    #[test]
    fn duplicate_version_and_equal_version_changed_body_are_rejected() {
        let second_bytes = fixture("2.root.json");
        let second = signed(&second_bytes);
        let start = RootIdentity::from_signed(&second).expect("canonical root identity");
        assert_eq!(
            verify_composite_chain(&start, std::slice::from_ref(&second_bytes), &second)
                .expect_err("duplicate version response"),
            "duplicate root version response"
        );

        let mut changed: serde_json::Value =
            serde_json::from_slice(&second_bytes).expect("parse signed root");
        changed["signed"]["consistent_snapshot"] = serde_json::Value::Bool(false);
        let changed_bytes = serde_json::to_vec(&changed).expect("serialize changed body");
        assert_eq!(
            verify_composite_chain(&start, &[changed_bytes], &second)
                .expect_err("equivocating same-version response"),
            "root version equivocation"
        );
    }

    #[test]
    fn bootstrap_membership_uses_exact_version_and_canonical_body() {
        let first = signed(&fixture("1.root.json"));
        let second = signed(&fixture("2.root.json"));
        let first_id = RootIdentity::from_signed(&first).expect("canonical root identity");
        let second_id = RootIdentity::from_signed(&second).expect("canonical root identity");
        let chain = verify_composite_chain(&first_id, &[fixture("2.root.json")], &second)
            .expect("valid composite chain");
        assert!(chain.identities.contains(&second_id));
        assert!(chain.identities.contains(&first_id));

        let mut changed: serde_json::Value =
            serde_json::from_slice(&fixture("2.root.json")).expect("parse root");
        changed["signed"]["consistent_snapshot"] = serde_json::Value::Bool(false);
        let changed_root: Signed<Root> =
            serde_json::from_value(changed).expect("parse modified root body");
        let changed_id = RootIdentity::from_signed(&changed_root).expect("canonical identity");
        assert_eq!(changed_id.version, second_id.version);
        assert_ne!(changed_id.signed_sha256, second_id.signed_sha256);
        assert!(!chain.identities.contains(&changed_id));
    }

    #[test]
    fn version_overflow_is_rejected() {
        let mut root: serde_json::Value =
            serde_json::from_slice(&fixture("2.root.json")).expect("parse root");
        root["signed"]["version"] = serde_json::Value::Number(u64::MAX.into());
        let max_root: Signed<Root> = serde_json::from_value(root).expect("parse max root");
        let mut next: serde_json::Value =
            serde_json::from_slice(&fixture("2.root.json")).expect("parse root");
        next["signed"]["version"] = serde_json::Value::Number(1.into());
        let lower_root: Signed<Root> = serde_json::from_value(next).expect("parse lower root");
        assert_eq!(
            max_root.signed.version,
            NonZeroU64::new(u64::MAX).expect("nonzero")
        );
        let start = RootIdentity::from_signed(&max_root).expect("canonical max identity");
        assert!(
            verify_composite_chain(
                &start,
                &[serde_json::to_vec(&lower_root).expect("serialize")],
                &max_root
            )
            .is_err()
        );
    }
}
