// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{
    IMAGE_CANDIDATE_SOURCE_INPUTS, IMAGE_NATIVE_INVENTORY_PATH, ImageCandidateSourceClosure,
    NativeImageSourceInventory, parse_image_candidate_source_closure,
    serialize_image_candidate_source_closure,
};

const SIGNED_CLOSURE: &[u8] =
    include_bytes!("fixtures/image-job/job-image-native-source-closure.json");

#[test]
fn historical_signed_source_inventory_remains_verifiable_after_generator_changes()
-> Result<(), Box<dyn Error>> {
    let historical = parse_image_candidate_source_closure(SIGNED_CLOSURE)?;
    assert_eq!(
        historical.source_commit,
        "a3417b150ac2f477498cadc329cdf4e23aa5b54b"
    );
    assert_eq!(
        serialize_image_candidate_source_closure(&historical)?,
        SIGNED_CLOSURE
    );
    assert!(
        ImageCandidateSourceClosure::new(
            historical.source_commit.clone(),
            historical.inputs.clone()
        )
        .is_err()
    );
    let inventory_bytes = historical
        .inputs
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let inventory = NativeImageSourceInventory::parse(inventory_bytes.as_bytes())?;
    assert_eq!(
        ImageCandidateSourceClosure::from_source_inventory(
            historical.source_commit.clone(),
            historical.inputs.clone(),
            &inventory
        )?,
        historical
    );
    let mut omitted = historical.inputs.clone();
    omitted.remove("Cargo.toml");
    assert!(
        ImageCandidateSourceClosure::from_source_inventory(
            historical.source_commit,
            omitted,
            &inventory
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn source_inventory_rejects_ambiguous_paths_and_missing_trust_anchors() -> Result<(), Box<dyn Error>>
{
    let mut paths = IMAGE_CANDIDATE_SOURCE_INPUTS
        .into_iter()
        .chain([IMAGE_NATIVE_INVENTORY_PATH])
        .map(str::to_owned)
        .collect::<Vec<_>>();
    paths.sort();
    let encode = |paths: &[String]| paths.join("\n") + "\n";
    NativeImageSourceInventory::parse(encode(&paths).as_bytes())?;
    for omitted in 0..paths.len() {
        let mut changed = paths.clone();
        changed.remove(omitted);
        assert!(NativeImageSourceInventory::parse(encode(&changed).as_bytes()).is_err());
    }
    for unsafe_path in [
        "../outside",
        "/absolute",
        "a//b",
        "a/./b",
        "a/../b",
        "a\\b",
        "a\rb",
        "a b",
    ] {
        let mut changed = paths.clone();
        changed.push(unsafe_path.to_owned());
        changed.sort();
        assert!(
            NativeImageSourceInventory::parse(encode(&changed).as_bytes()).is_err(),
            "accepted {unsafe_path:?}"
        );
    }
    let mut duplicate = paths.clone();
    duplicate.insert(0, paths[0].clone());
    assert!(NativeImageSourceInventory::parse(encode(&duplicate).as_bytes()).is_err());
    paths.reverse();
    assert!(NativeImageSourceInventory::parse(encode(&paths).as_bytes()).is_err());
    assert!(NativeImageSourceInventory::parse(&vec![b'x'; 1024 * 1024 + 1]).is_err());
    Ok(())
}
