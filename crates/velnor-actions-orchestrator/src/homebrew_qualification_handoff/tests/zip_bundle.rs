// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{
    ASSET_LIMIT,
    bundle::{open_bundle, read_admitted_files, read_entry},
};
use super::{TestResult, encode, fixture, sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};
use zip::{ZipWriter, write::SimpleFileOptions};

#[test]
fn selected_asset_handoffs_preserve_all_files_at_the_admission_limit() -> TestResult {
    let (base_plan, base_spec, _, _) = super::super::super::load_catalog()?;
    for additional in [14, 62] {
        let mut plan = base_plan.clone();
        let mut spec = base_spec.clone();
        for index in 0..additional {
            let name = format!("extra-{index:02}-{}", "x".repeat(91));
            assert_eq!(name.len(), 100);
            let mut product = plan.products[1].clone();
            product.id.clone_from(&name);
            product.package.clone_from(&name);
            product.bins = vec![name.clone()];
            plan.products.push(product);
            let mut auxiliary = spec.auxiliaries[0].clone();
            auxiliary.product.clone_from(&name);
            for file in &mut auxiliary.files {
                file.member.clone_from(&name);
                file.destination = format!("{name}/{}", file.target);
            }
            spec.auxiliaries.push(auxiliary);
        }
        plan.products.sort_by(|a, b| a.id.cmp(&b.id));
        spec.auxiliaries.sort_by(|a, b| a.product.cmp(&b.product));
        plan.validate()?;
        spec.validate()?;
        let assets = super::super::selected_assets(&spec, &plan)?;
        assert_eq!(assets.len(), 2 + additional);
        let (mut admission, context, catalog) = fixture();
        let mut files = BTreeMap::new();
        for label in ["A", "D"] {
            files.insert(format!("{label}/release.json"), b"receipt".to_vec());
            for asset in &assets {
                files.insert(format!("{label}/{asset}"), asset.as_bytes().to_vec());
            }
        }
        admission.files = files.iter().map(|(name, bytes)| (name.clone(), super::super::FileIdentity {
            size: bytes.len() as u64,
            sha256: sha256(bytes),
        })).collect();
        let json = encode(&admission)?;
        super::super::validate_admission(&json, &sha256(&json), &context, &catalog)?;
        let raw = super::super::bundle::encode_bundle(&json, files.clone())?;
        assert_eq!(read_admitted_files(&mut open_bundle(&raw)?, &admission)?, files);
        if additional == 62 {
            let mut extra = plan.products[0].clone();
            extra.id = "overflow".to_owned();
            extra.package.clone_from(&extra.id);
            extra.bins = vec![extra.id.clone()];
            plan.products.push(extra);
            plan.products.sort_by(|a, b| a.id.cmp(&b.id));
            assert!(plan.validate().is_err());
        }
    }
    Ok(())
}

pub(super) fn fixture_bundle(
    files: &[(&str, &[u8])],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o644);
    for (name, bytes) in files {
        writer.start_file(*name, options)?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

#[test]
fn raw_bundle_admits_exact_files_without_filesystem_extraction() -> TestResult {
    let (admission, _, _) = fixture();
    let json = encode(&admission)?;
    let raw = fixture_bundle(&[("admission.json", &json), ("A/release.json", b"receipt")])?;
    let mut archive = open_bundle(&raw)?;
    assert_eq!(
        read_entry(&mut archive, "admission.json", 1024 * 1024)?,
        json
    );
    assert_eq!(
        read_admitted_files(&mut archive, &admission)?,
        BTreeMap::from([("A/release.json".to_owned(), b"receipt".to_vec())])
    );
    let raw = fixture_bundle(&[("admission.json", &json), ("A/release.json", b"forgery")])?;
    assert!(read_admitted_files(&mut open_bundle(&raw)?, &admission).is_err());
    let raw = fixture_bundle(&[
        ("admission.json", &json),
        ("A/release.json", b"receipt"),
        ("D/extra", b"extra"),
    ])?;
    assert!(read_admitted_files(&mut open_bundle(&raw)?, &admission).is_err());
    let raw = fixture_bundle(&[("admission.json", &json)])?;
    assert!(read_admitted_files(&mut open_bundle(&raw)?, &admission).is_err());
    Ok(())
}

fn duplicate_directory(raw: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let eocd = raw.len() - 22;
    let offset = usize::try_from(u32::from_le_bytes(raw[eocd + 16..eocd + 20].try_into()?))?;
    let directory = &raw[offset..eocd];
    let mut duplicate = raw[..eocd].to_vec();
    duplicate.extend_from_slice(directory);
    duplicate.extend_from_slice(&raw[eocd..]);
    let eocd = duplicate.len() - 22;
    duplicate[eocd + 8..eocd + 10].copy_from_slice(&2_u16.to_le_bytes());
    duplicate[eocd + 10..eocd + 12].copy_from_slice(&2_u16.to_le_bytes());
    duplicate[eocd + 12..eocd + 16]
        .copy_from_slice(&u32::try_from(directory.len() * 2)?.to_le_bytes());
    Ok(duplicate)
}

#[test]
fn raw_bundle_refuses_duplicate_traversal_symlink_and_compressed_entries() -> TestResult {
    for name in [
        "../outside",
        "/absolute",
        "A/../outside",
        "A/\\outside",
        "A/nested/file",
        "A/.",
        "A/..",
    ] {
        assert!(
            open_bundle(&fixture_bundle(&[(name, b"payload")])?).is_err(),
            "accepted {name}"
        );
    }
    let raw = fixture_bundle(&[("admission.json", b"json")])?;
    assert!(open_bundle(&duplicate_directory(&raw)?).is_err());
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.add_symlink("A/link", "/etc/passwd", SimpleFileOptions::default())?;
    assert!(open_bundle(&writer.finish()?.into_inner()).is_err());
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file(
        "A/bomb",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
    )?;
    writer.write_all(&vec![b'x'; 65_536])?;
    assert!(open_bundle(&writer.finish()?.into_inner()).is_err());
    Ok(())
}

#[test]
fn raw_bundle_limits_directory_count_declared_sizes_and_actual_bytes() -> TestResult {
    let names = (0..132).map(|n| format!("A/file-{n}")).collect::<Vec<_>>();
    let entries = names.iter().map(|name| (name.as_str(), &b"payload"[..])).collect::<Vec<_>>();
    assert!(open_bundle(&fixture_bundle(&entries[..131])?).is_ok());
    assert!(open_bundle(&fixture_bundle(&entries)?).is_err());
    let raw = fixture_bundle(&[("A/payload", b"payload")])?;
    let eocd = raw.len() - 22;
    let mut count = raw.clone();
    count[eocd + 8..eocd + 10].copy_from_slice(&u16::MAX.to_le_bytes());
    count[eocd + 10..eocd + 12].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(open_bundle(&count).is_err());
    let offset = usize::try_from(u32::from_le_bytes(raw[eocd + 16..eocd + 20].try_into()?))?;
    let mut oversize = raw.clone();
    oversize[offset + 24..offset + 28]
        .copy_from_slice(&u32::try_from(ASSET_LIMIT + 1)?.to_le_bytes());
    assert!(open_bundle(&oversize).is_err());
    let mut corrupt = raw.clone();
    let payload = usize::try_from(
        zip::ZipArchive::new(Cursor::new(&raw))?
            .by_name("A/payload")?
            .data_start()
            .ok_or("payload fixture data")?,
    )?;
    corrupt[payload] = b'X';
    assert!(read_entry(&mut open_bundle(&corrupt)?, "A/payload", 7).is_err());
    assert!(open_bundle(&raw[..raw.len() - 1]).is_err());
    assert_ne!(sha256(&raw), sha256(&corrupt));
    Ok(())
}
