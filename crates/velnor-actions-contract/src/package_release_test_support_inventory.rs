use super::super::{
    PackageReleaseAsset, PackageReleaseAssetKind, PackageReleaseChannel, PackageReleasePayload,
    PackageReleaseProduct, PackageReleaseReceipt,
};
use super::asset;
use super::fixture;
use crate::config::{
    RustBinaryPreviewArchive, RustBinaryPreviewCompanion, RustBinaryPreviewConfig,
    RustBinaryPreviewProducer, RustBinaryPreviewProduct,
};

pub(crate) fn jackin_preview() -> RustBinaryPreviewConfig {
    RustBinaryPreviewConfig {
        enabled: true,
        version_env: "JACKIN_VERSION_OVERRIDE".to_owned(),
        producer: RustBinaryPreviewProducer {
            task: "release:preview-package".to_owned(),
            tools: vec!["rust".to_owned()],
            inputs: vec!["Cargo.lock".to_owned(), "mise.toml".to_owned()],
            ..RustBinaryPreviewProducer::default()
        },
        products: vec![
            preview_product(
                "jackin",
                "jackin",
                &["jackin", "jackin-role", "jackin-usage-broker"],
                &[
                    "aarch64-apple-darwin",
                    "x86_64-apple-darwin",
                    "aarch64-unknown-linux-gnu",
                    "x86_64-unknown-linux-gnu",
                ],
            ),
            preview_product(
                "jackin-capsule",
                "jackin-capsule",
                &["jackin-capsule"],
                &["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"],
            ),
        ],
        companions: vec![RustBinaryPreviewCompanion {
            id: "capsule-manifest".to_owned(),
            path: "capsule-manifest.json".to_owned(),
            asset_name: "capsule-manifest.json".to_owned(),
        }],
        ..RustBinaryPreviewConfig::default()
    }
}

pub(crate) fn jackin_receipt(preview: &RustBinaryPreviewConfig) -> PackageReleaseReceipt {
    let mut receipt = fixture(PackageReleaseChannel::Preview);
    receipt.products = jackin_products(preview, &receipt.version.base);
    receipt.assets = jackin_assets(&receipt.products);
    receipt.evidence.attested_subject_names = jackin_subjects(&receipt.products);
    receipt
}

pub(crate) fn jackin_products(
    preview: &RustBinaryPreviewConfig,
    base: &str,
) -> Vec<PackageReleaseProduct> {
    let mut products: Vec<_> = preview
        .products
        .iter()
        .map(|product| PackageReleaseProduct {
            id: product.id.clone(),
            package: product.package.clone(),
            package_version: base.to_owned(),
            bins: sorted_strings(&product.bins),
            payloads: {
                let mut payloads: Vec<_> = product
                    .targets
                    .iter()
                    .map(|target| PackageReleasePayload {
                        format: crate::package_release::PackageReleasePayloadFormat::TarGz,
                        target: target.clone(),
                        name: format!("{}-{target}.tar.gz", product.id),
                        members: product.bins.clone(),
                    })
                    .collect();
                payloads.sort_by(|left, right| left.target.cmp(&right.target));
                payloads
            },
        })
        .collect();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    products
}

pub(crate) fn jackin_assets(products: &[PackageReleaseProduct]) -> Vec<PackageReleaseAsset> {
    let mut assets = Vec::new();
    for product in products {
        for archive in &product.payloads {
            assets.extend(archive_assets(product, archive));
        }
    }
    assets.extend([
        asset(
            "capsule-manifest.json",
            PackageReleaseAssetKind::Companion,
            None,
            None,
            Some("capsule-manifest.json"),
        ),
        asset(
            "capsule-manifest.json.bundle",
            PackageReleaseAssetKind::CompanionSignatureBundle,
            None,
            None,
            Some("capsule-manifest.json"),
        ),
        asset(
            "SHA256SUMS",
            PackageReleaseAssetKind::ChecksumList,
            None,
            None,
            None,
        ),
    ]);
    assets.sort_by(|left, right| left.name.cmp(&right.name));
    assets
}

pub(crate) fn archive_assets(
    product: &PackageReleaseProduct,
    archive: &PackageReleasePayload,
) -> [PackageReleaseAsset; 4] {
    let owner = Some(product.id.as_str());
    let target = Some(archive.target.as_str());
    [
        asset(
            &archive.name,
            PackageReleaseAssetKind::Payload,
            owner,
            target,
            Some(&archive.name),
        ),
        asset(
            &format!("{}.sha256", archive.name),
            PackageReleaseAssetKind::Sha256,
            owner,
            target,
            Some(&archive.name),
        ),
        asset(
            &format!("{}.bundle", archive.name),
            PackageReleaseAssetKind::SignatureBundle,
            owner,
            target,
            Some(&archive.name),
        ),
        asset(
            &format!("{}.sbom.json", archive.name),
            PackageReleaseAssetKind::Sbom,
            owner,
            target,
            Some(&archive.name),
        ),
    ]
}

pub(crate) fn jackin_subjects(products: &[PackageReleaseProduct]) -> Vec<String> {
    let mut subjects = products
        .iter()
        .flat_map(|product| product.payloads.iter().map(|archive| archive.name.clone()))
        .collect::<Vec<_>>();
    subjects.extend([
        "capsule-manifest.json".to_owned(),
        "release.json".to_owned(),
    ]);
    subjects.sort();
    subjects
}

pub(crate) fn archive_count(receipt: &PackageReleaseReceipt) -> usize {
    receipt
        .products
        .iter()
        .map(|product| product.payloads.len())
        .sum()
}

fn sorted_strings(values: &[String]) -> Vec<String> {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted
}

pub(crate) fn preview_product(
    id: &str,
    package: &str,
    bins: &[&str],
    targets: &[&str],
) -> RustBinaryPreviewProduct {
    RustBinaryPreviewProduct {
        id: id.to_owned(),
        package: package.to_owned(),
        bins: bins.iter().map(|bin| (*bin).to_owned()).collect(),
        targets: targets.iter().map(|target| (*target).to_owned()).collect(),
        archive: RustBinaryPreviewArchive::TarGz,
    }
}
