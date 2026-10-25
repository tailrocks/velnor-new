use std::collections::BTreeMap;

use super::*;

fn valid_record() -> ImageCandidateRecord {
    let inputs = IMAGE_CANDIDATE_SOURCE_INPUTS
        .into_iter()
        .map(|path| (path.to_owned(), "a".repeat(64)))
        .collect();
    ImageCandidateRecord {
        schema: IMAGE_CANDIDATE_SCHEMA.to_owned(),
        source: ImageCandidateSource {
            commit: "b".repeat(40),
            source_ref: "refs/heads/main".to_owned(),
            inputs,
        },
        workflow: ImageCandidateWorkflow {
            repository: "tailrocks/velnor-new".to_owned(),
            repository_id: 12,
            event: "push".to_owned(),
            workflow_ref:
                "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml@refs/heads/main"
                    .to_owned(),
            authority_commit: "c".repeat(40),
            run_id: 42,
            run_attempt: 1,
        },
        platform: "linux/amd64".to_owned(),
        build: ImageCandidateBuild {
            builder: "github.com/docker/buildx v0.31.0".to_owned(),
            platform: "linux/amd64".to_owned(),
            arguments: Vec::new(),
            exporter: "oci".to_owned(),
            provenance: "disabled".to_owned(),
            compression: "gzip".to_owned(),
            force_compression: true,
        },
        archive: ImageCandidateArchive {
            sha256: "d".repeat(64),
            size_bytes: 1,
        },
        oci: ImageCandidateOci {
            archive_index_digest: "e".repeat(64),
            archive_index_size_bytes: 1,
            platform_manifest_digest: format!("sha256:{}", "f".repeat(64)),
            platform_manifest_size_bytes: 1,
            image_configuration_digest: format!("sha256:{}", "0".repeat(64)),
            image_configuration_size_bytes: 1,
            layer_digests: vec![format!("sha256:{}", "1".repeat(64))],
        },
    }
}

