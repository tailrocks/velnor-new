// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn assert_publication_records(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    for (role, record_name, archive_name, envelope_name) in [
        (
            "runner",
            "runner-image-publication.json",
            "velnor-runner-linux-amd64.tar",
            "runner-image-provenance.json",
        ),
        (
            "dind",
            "dind-image-publication.json",
            "velnor-dind-linux-amd64.tar",
            "dind-image-provenance.json",
        ),
        (
            "resource_probe",
            "resource-probe-image-publication.json",
            "velnor-resource-probe-linux-amd64.tar",
            "resource-probe-image-provenance.json",
        ),
    ] {
        let record = velnor_actions_contract::parse_runner_image_publication_record(&fs::read(
            fixture.output.join(record_name),
        )?)?;
        assert_eq!(record.image, role);
        assert_eq!(record.source.commit, fixture.sha);
        let archive = fs::read(fixture.output.join(archive_name))?;
        assert_eq!(record.archive.sha256, hex(&Sha256::digest(&archive)));
        assert_eq!(record.archive.size_bytes, archive.len() as u64);
        assert_graph(&record.oci, &archive)?;
        let envelope = fs::read(fixture.output.join(envelope_name))?;
        assert_eq!(
            record.provenance.envelope.sha256,
            hex(&Sha256::digest(&envelope))
        );
        assert_eq!(record.provenance.envelope.size_bytes, envelope.len() as u64);
        let lock = fs::read(fixture.root.join(".velnor/runner.lock"))?;
        assert_eq!(record.lock.sha256, hex(&Sha256::digest(&lock)));
        assert_eq!(record.source.source_ref, "refs/heads/main");
        for path in velnor_actions_contract::runner_publication_source_inputs()? {
            assert_eq!(
                record.source.inputs[*path],
                hex(&Sha256::digest(fs::read(fixture.root.join(path))?))
            );
        }
        assert_eq!(
            (
                &record.lock.runner_version,
                &record.lock.docker_version,
                &record.lock.buildx_version,
                &record.lock.compose_version,
                &record.lock.node_version
            ),
            (
                &"2.338.0".to_owned(),
                &"29.9.1".to_owned(),
                &"0.38.0".to_owned(),
                &"5.6.0".to_owned(),
                &"26.12.0".to_owned()
            )
        );
        assert_eq!(record.workflow.repository, "tailrocks/velnor-new");
        assert_eq!(record.workflow.repository_id, 1_390_620_900);
        assert_eq!(record.workflow.event, "push");
        assert_eq!(
            record.workflow.workflow_ref,
            "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main"
        );
        assert_eq!(
            (record.workflow.run_id, record.workflow.run_attempt),
            (31, 2)
        );
        assert_eq!(record.workflow.authority_commit, fixture.sha);
        for (key, context) in &record.provenance.named_contexts {
            let bytes = fs::read(fixture.contexts.join(key))?;
            assert_eq!(context.sha256, hex(&Sha256::digest(&bytes)));
            assert_eq!(context.size_bytes, bytes.len() as u64);
        }
    }
    Ok(())
}

fn assert_graph(
    oci: &velnor_actions_contract::ImageCandidateOci,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    use std::{collections::BTreeMap, io::Read};
    let mut files = BTreeMap::new();
    for entry in tar::Archive::new(archive).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry.path()?.to_string_lossy().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.insert(name, bytes);
    }
    let index = files.get("index.json").ok_or("missing index")?;
    assert_eq!(oci.archive_index_digest, hex(&Sha256::digest(index)));
    assert_eq!(oci.archive_index_size_bytes, index.len() as u64);
    let manifest = files
        .get(&format!(
            "blobs/sha256/{}",
            oci.platform_manifest_digest
                .strip_prefix("sha256:")
                .ok_or("manifest digest")?
        ))
        .ok_or("missing manifest")?;
    assert_eq!(
        oci.platform_manifest_digest,
        format!("sha256:{}", hex(&Sha256::digest(manifest)))
    );
    assert_eq!(oci.platform_manifest_size_bytes, manifest.len() as u64);
    let config = files
        .get(&format!(
            "blobs/sha256/{}",
            oci.image_configuration_digest
                .strip_prefix("sha256:")
                .ok_or("config digest")?
        ))
        .ok_or("missing config")?;
    assert_eq!(
        oci.image_configuration_digest,
        format!("sha256:{}", hex(&Sha256::digest(config)))
    );
    assert_eq!(oci.image_configuration_size_bytes, config.len() as u64);
    let value: serde_json::Value = serde_json::from_slice(manifest)?;
    let layers = value["layers"].as_array().ok_or("layers")?;
    let mut digests = Vec::new();
    for layer in layers {
        let digest = layer["digest"].as_str().ok_or("layer digest")?;
        let bytes = files
            .get(&format!(
                "blobs/sha256/{}",
                digest.strip_prefix("sha256:").ok_or("digest")?
            ))
            .ok_or("layer blob")?;
        assert_eq!(digest, format!("sha256:{}", hex(&Sha256::digest(bytes))));
        digests.push(digest.to_owned());
    }
    assert_eq!(oci.layer_digests, digests);
    Ok(())
}
