// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[allow(
    dead_code,
    unreachable_pub,
    reason = "The integration test imports the production error type."
)]
#[path = "../src/error.rs"]
mod error;
#[allow(dead_code, reason = "The integration test imports production hashing.")]
#[path = "../src/artifact_admission/hash.rs"]
mod hash;
#[allow(
    dead_code,
    reason = "The integration test exercises the private cache policy."
)]
#[path = "../src/artifact_admission/projection_cache.rs"]
mod projection_cache;

use projection_cache::{ProjectionCache, RegistryProjection};
use velnor_runner_image_manifest::{ImageEntry, ImageKind};

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hash::lowercase_hex(&hash::sha256(bytes)))
}

fn fixture(
    role: ImageKind,
) -> Result<(ImageEntry, String, Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let source = "a".repeat(40);
    let config = serde_json::to_vec(&serde_json::json!({
        "os": "linux", "architecture": "amd64",
        "rootfs": {"type": "layers", "diff_ids": [format!("sha256:{}", "b".repeat(64))]},
        "config": {"Labels": {"org.opencontainers.image.revision": source, "io.velnor.image.kind": match role { ImageKind::Runner => "runner", ImageKind::Dind => "dind", ImageKind::ResourceProbe => "resource_probe" }}}
    }))?;
    let manifest = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": digest(&config), "size": config.len()},
        "layers": [{"mediaType": "application/vnd.oci.image.layer.v1.tar+gzip", "digest": format!("sha256:{}", "c".repeat(64)), "size": 1}]
    }))?;
    let image: ImageEntry = serde_json::from_value(serde_json::json!({
        "repository": role.repository(), "tag": format!("sha-{source}"),
        "registry_digest": digest(&manifest), "platform_manifest_digest": digest(&manifest), "config_digest": digest(&config),
        "build_source": {"metadata_sha256": "d".repeat(64), "build_type": "test", "source_repository": "test", "source_commit": source, "context_path": "test", "dockerfile_path": "test", "builder_platform": "linux/amd64", "named_contexts": {}}
    }))?;
    Ok((image, source, manifest, config))
}

#[test]
fn validated_projection_is_reused_only_for_the_same_image_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let (image, source, manifest, config) = fixture(ImageKind::Runner)?;
    let projection =
        RegistryProjection::admit(&image, ImageKind::Runner, &source, &manifest, &config)?;
    let mut cache = ProjectionCache::default();
    cache.insert(&image, projection.clone());
    assert_eq!(
        cache.get(&image, ImageKind::Runner, &source),
        Some(projection)
    );
    assert!(cache.get(&image, ImageKind::Dind, &source).is_none());
    assert!(
        cache
            .get(&image, ImageKind::Runner, &"e".repeat(40))
            .is_none()
    );
    for field in [
        "repository",
        "registry_digest",
        "platform_manifest_digest",
        "config_digest",
        "tag",
    ] {
        let mut changed = serde_json::to_value(&image)?;
        changed[field] = serde_json::json!("changed");
        let changed = serde_json::from_value(changed)?;
        assert!(
            cache.get(&changed, ImageKind::Runner, &source).is_none(),
            "{field}"
        );
    }
    let (dind, dind_source, dind_manifest, dind_config) = fixture(ImageKind::Dind)?;
    let dind_projection = RegistryProjection::admit(
        &dind,
        ImageKind::Dind,
        &dind_source,
        &dind_manifest,
        &dind_config,
    )?;
    cache.insert(&dind, dind_projection.clone());
    assert_eq!(
        cache.get(&dind, ImageKind::Dind, &dind_source),
        Some(dind_projection)
    );
    assert!(cache.get(&image, ImageKind::Runner, &source).is_some());
    let mut corrupt_config = config;
    corrupt_config.push(b' ');
    assert!(
        RegistryProjection::admit(
            &image,
            ImageKind::Runner,
            &source,
            &manifest,
            &corrupt_config
        )
        .is_err()
    );
    assert!(cache.get(&image, ImageKind::Runner, &source).is_some());
    Ok(())
}
