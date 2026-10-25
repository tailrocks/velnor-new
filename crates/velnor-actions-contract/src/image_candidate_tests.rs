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

#[test]
fn candidate_record_round_trips_canonical_v1_and_rejects_duplicate_keys() {
    let record = valid_record();
    let bytes = serialize_image_candidate_record(&record).expect("serialize candidate record");
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(
        parse_image_candidate_record(&bytes).expect("parse record"),
        record
    );
    assert!(parse_image_candidate_record(br#"{"schema":"a","schema":"b"}"#).is_err());
}

#[test]
fn candidate_record_rejects_source_drift_and_unknown_fields() {
    let mut record = valid_record();
    record
        .source
        .inputs
        .remove("images/job/ubuntu-26.04/candidate/candidate.py");
    assert!(validate_image_candidate_record(&record).is_err());
    let mut record = valid_record();
    record
        .build
        .arguments
        .push("--build-arg=SECRET=x".to_owned());
    assert!(validate_image_candidate_record(&record).is_err());

    let mut bytes = serialize_image_candidate_record(&valid_record()).expect("record bytes");
    bytes.pop();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("record JSON");
    value["unexpected"] = serde_json::Value::Bool(true);
    assert!(
        parse_image_candidate_record(&serde_json::to_vec(&value).expect("JSON bytes")).is_err()
    );
}

#[test]
fn candidate_record_rejects_noncanonical_buildx_and_empty_workflow_event() {
    for builder in [
        "github.com/docker/buildx v0",
        "github.com/docker/buildx v0.38",
        "github.com/docker/buildx v0.38.0 ",
        "github.com/docker/buildx v0.38.0\nother",
    ] {
        let mut record = valid_record();
        record.build.builder = builder.to_owned();
        assert!(
            validate_image_candidate_record(&record).is_err(),
            "{builder:?}"
        );
    }
    let mut record = valid_record();
    record.workflow.event.clear();
    assert!(validate_image_candidate_record(&record).is_err());
}

#[test]
fn source_closure_binds_exact_native_implementation_paths() {
    let inputs = NATIVE_IMAGE_CANDIDATE_INPUTS
        .into_iter()
        .map(|path| (path.to_owned(), "a".repeat(64)))
        .collect::<BTreeMap<_, _>>();
    let closure = ImageCandidateSourceClosure::new("b".repeat(40), inputs)
        .expect("valid implementation closure");
    for path in [
        "crates/velnor-archive-guard/build.rs",
        "crates/velnor-archive-guard/build_support/archive_guard_inputs.rs",
        "crates/velnor-archive-guard/build_support/archive_guard_inputs_filesystem.rs",
    ] {
        assert!(NATIVE_IMAGE_CANDIDATE_INPUTS.contains(&path));
        let mut missing = closure.clone();
        missing.inputs.remove(path);
        assert!(
            validate_image_candidate_source_closure(&missing).is_err(),
            "{path}"
        );
    }
    let bytes = serialize_image_candidate_source_closure(&closure).expect("serialize closure");
    assert_eq!(
        parse_image_candidate_source_closure(&bytes).expect("parse closure"),
        closure
    );
    for path in NATIVE_IMAGE_CANDIDATE_INPUTS {
        let mut incomplete = closure.clone();
        incomplete.inputs.remove(path);
        assert!(
            validate_image_candidate_source_closure(&incomplete).is_err(),
            "{path}"
        );
    }
    let mut extra = closure;
    extra
        .inputs
        .insert("unreviewed.rs".to_owned(), "c".repeat(64));
    assert!(validate_image_candidate_source_closure(&extra).is_err());
}

#[test]
fn source_closure_covers_shared_command_runtime_modules() {
    for (directory, source) in [
        (
            "crates/velnor-actions-mise/src",
            include_str!("../../velnor-actions-mise/src/command.rs"),
        ),
        (
            "crates/velnor-actions-mise/src",
            include_str!("../../velnor-actions-mise/src/command_cancellable.rs"),
        ),
        (
            "crates/velnor-archive-guard/src/owned_archive_guard/oci_candidate",
            include_str!("../../velnor-archive-guard/src/owned_archive_guard/oci_candidate.rs"),
        ),
    ] {
        let lines = source.lines().collect::<Vec<_>>();
        for pair in lines.windows(2) {
            if pair[0] == "#[cfg(test)]" {
                continue;
            }
            let explicit = pair[1]
                .strip_prefix("#[path = \"")
                .and_then(|value| value.strip_suffix("\"]"))
                .map(str::to_owned);
            let implicit = (!pair[0].starts_with("#[path = "))
                .then(|| {
                    pair[1]
                        .strip_prefix("mod ")
                        .and_then(|value| value.strip_suffix(';'))
                })
                .flatten()
                .map(|module| format!("{module}.rs"));
            let Some(module) = explicit.or(implicit) else {
                continue;
            };
            let path = format!("{directory}/{module}");
            assert!(
                NATIVE_IMAGE_CANDIDATE_INPUTS.contains(&path.as_str()),
                "missing runtime module: {path}"
            );
        }
    }
}
