// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{Fixture, oci};

#[derive(Clone, Copy)]
pub(crate) enum ExistingTag {
    SameDigest,
    ConflictingLeaf,
    OuterIndex,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum BlobFault {
    Missing,
    Short,
    Oversize,
    CorruptAt(u64),
    Nonzero,
}

pub(crate) struct MovedTag {
    pub(crate) digest: String,
    pub(crate) inventory: BTreeMap<String, Vec<u8>>,
}

pub(super) fn seed_existing_tag(fixture: &Fixture, tag: ExistingTag) -> Result<(), Box<dyn Error>> {
    let (digest, inventory) = match tag {
        ExistingTag::SameDigest => (
            fixture.expected_digest.clone(),
            fixture.expected_inventory.clone(),
        ),
        ExistingTag::ConflictingLeaf => {
            let source = "f".repeat(40);
            let image = oci::create(&source)?;
            (image.identity.platform_manifest_digest, image.inventory)
        }
        ExistingTag::OuterIndex => {
            let digest = digest(&fixture.archive_index);
            let mut inventory = fixture.expected_inventory.clone();
            inventory.insert(blob_path(&digest), fixture.archive_index.clone());
            (digest, inventory)
        }
    };
    let mut state: Value = serde_json::from_slice(&std::fs::read(&fixture.state)?)?;
    let repository = fixture.profile.repository();
    state["crane"]["tag_digest"] = json!(digest);
    state["crane"]["manifest_reference"] = json!(format!("{repository}@{digest}"));
    state["crane"]["remote"] = json!({
        "manifest_digest":digest,
        "inventory":inventory_value(&inventory)?
    });
    std::fs::write(&fixture.state, serde_json::to_vec(&state)?)?;
    Ok(())
}

pub(super) fn set_blob_fault(
    fixture: &Fixture,
    kind: &str,
    fault: BlobFault,
) -> Result<(), Box<dyn Error>> {
    let value = match fault {
        BlobFault::Missing => json!({"kind":kind,"mode":"missing"}),
        BlobFault::Short => json!({"kind":kind,"mode":"short"}),
        BlobFault::Oversize => json!({"kind":kind,"mode":"oversize"}),
        BlobFault::CorruptAt(offset) => json!({"kind":kind,"mode":"corrupt","offset":offset}),
        BlobFault::Nonzero => json!({"kind":kind,"mode":"nonzero"}),
    };
    let mut state: Value = serde_json::from_slice(&std::fs::read(&fixture.state)?)?;
    state["crane"]["corrupt_remote_blob"] = value;
    std::fs::write(&fixture.state, serde_json::to_vec(&state)?)?;
    Ok(())
}

pub(super) fn move_tag_after_readback(fixture: &Fixture) -> Result<MovedTag, Box<dyn Error>> {
    let source = "e".repeat(40);
    let image = oci::create(&source)?;
    let digest = image.identity.platform_manifest_digest.clone();
    let inventory = image.inventory;
    let target = json!({
        "digest":digest,
        "inventory":inventory_value(&inventory)?
    });
    let mut state: Value = serde_json::from_slice(&std::fs::read(&fixture.state)?)?;
    state["crane"]["tag_movement_target"] = target;
    state["crane"]["pending_tag_movement"] = Value::Null;
    std::fs::write(&fixture.state, serde_json::to_vec(&state)?)?;
    Ok(MovedTag { digest, inventory })
}

pub(super) fn corrupt_remote_manifest_response_at(
    fixture: &Fixture,
    offset: u64,
) -> Result<(), Box<dyn Error>> {
    let mut state: Value = serde_json::from_slice(&std::fs::read(&fixture.state)?)?;
    state["crane"]["remote_manifest_response_corruption"] = json!({"offset":offset});
    std::fs::write(&fixture.state, serde_json::to_vec(&state)?)?;
    Ok(())
}

fn inventory_value(inventory: &BTreeMap<String, Vec<u8>>) -> Result<Value, Box<dyn Error>> {
    let entries = inventory
        .iter()
        .map(|(path, bytes)| {
            let name = path.rsplit('/').next().ok_or("blob path has no digest")?;
            let blob_digest = format!("sha256:{name}");
            if blob_digest != digest(bytes) {
                return Err(format!("blob path does not match bytes: {path}").into());
            }
            Ok((
                path.clone(),
                json!({"digest":blob_digest,"size":bytes.len(),"bytes":bytes}),
            ))
        })
        .collect::<Result<Map<_, _>, Box<dyn Error>>>()?;
    Ok(Value::Object(entries))
}

fn blob_path(digest: &str) -> String {
    format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", super::hex(&Sha256::digest(bytes)))
}
