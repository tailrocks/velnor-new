use super::action_snapshots::{Actions, action};

pub(super) const CREDENTIAL_UNSET_PRELUDE: &str = "unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR;";

pub(super) fn assert_candidate_qualification(
    workflow: &str,
    actions: &Actions,
    job_id: &str,
    action_name: &str,
    build_job: &str,
    directory: &str,
    binary: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let action_body = action(actions, action_name)?;
    let archive_extract = action_body
        .find("Prevalidate and extract uploaded candidate archive")
        .ok_or("missing prevalidated candidate archive extraction")?;
    let file_modes = action_body
        .find("Verify extracted candidate file types and executable mode")
        .ok_or("missing extracted candidate file-type check")?;
    let checksum = action_body
        .find("Verify downloaded checksum sidecar")
        .ok_or("missing downloaded checksum verification")?;
    let provenance = action_body
        .find("Verify candidate provenance record")
        .ok_or("missing candidate provenance verification")?;
    let qualification = action_body
        .find("Qualify downloaded candidate")
        .ok_or("missing exact candidate qualification")?;
    let job = super::super::job_body(workflow, job_id)?;
    let caller_artifact_id =
        format!("artifact_id: ${{{{ needs.{build_job}.outputs.artifact_id }}}}");
    assert!(
        archive_extract < file_modes
            && file_modes < provenance
            && provenance < checksum
            && checksum < qualification,
        "{job}"
    );
    assert!(job.contains(&caller_artifact_id), "{job}");
    assert!(
        job.contains("manifest_sha256: ${{ needs.candidate-manifest.outputs.manifest_sha256 }}"),
        "{job}"
    );
    assert!(
        action_body.contains("artifact-ids: ${{ inputs.artifact_id }}"),
        "{action_body}"
    );
    assert!(
        action_body.contains("inputs:\n  artifact_id:"),
        "{action_body}"
    );
    assert!(
        action_body.contains("  manifest_artifact_id:")
            && action_body.contains("  manifest_sha256:")
            && action_body
                .contains("VELNOR_RELEASE_MANIFEST_SHA256: ${{ inputs.manifest_sha256 }}"),
        "{action_body}"
    );
    assert!(action_body.contains("required: true"), "{action_body}");
    assert!(!action_body.contains("${{ needs."), "{action_body}");
    assert_empty_credential_environment(action_body);
    assert!(
        !action_body.contains("Attest built artifacts"),
        "{action_body}"
    );
    assert!(
        !action_body.contains("actions/upload-artifact@"),
        "{action_body}"
    );
    assert_eq!(
        action_body.matches("Qualify downloaded candidate").count(),
        1
    );
    assert!(action_body.contains("test -x "), "{action_body}");
    assert!(action_body.contains("toolchain"), "{action_body}");
    assert!(action_body.contains("--version"), "{action_body}");
    assert!(
        action_body.contains(&format!(
            "scripts/capture-opentofu-goldens.sh check-release \\\"$GITHUB_WORKSPACE/{directory}/{binary}\\\" \\\"$GITHUB_WORKSPACE/manifest-assets/release-manifest.json\\\" \\\"$VELNOR_RELEASE_MANIFEST_SHA256\\\""
        )),
        "{action_body}"
    );
    assert_release_fixture_generation_requirements()?;
    Ok(())
}

pub(super) fn assert_empty_credential_environment(action_body: &str) {
    const CREDENTIAL_ENV_KEYS: &[&str] = &[
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "CARGO_REGISTRY_TOKEN",
        "GH_CONFIG_DIR",
        "GH_HOST",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_TOKEN",
        "NODE_AUTH_TOKEN",
        "NPM_TOKEN",
    ];
    for key in CREDENTIAL_ENV_KEYS {
        assert!(
            !contains_credential_assignment(action_body, key),
            "credential {key} must not be assigned in the payload"
        );
    }

    let mut github_token_entries = 0;
    for line in action_body.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        if CREDENTIAL_ENV_KEYS.contains(&key) {
            assert_eq!(value.trim(), "\"\"", "credential {key} must be empty");
            if key == "GH_TOKEN" {
                github_token_entries += 1;
            }
        }
    }
    assert_eq!(github_token_entries, 2, "{action_body}");
    assert_eq!(
        action_body.matches(CREDENTIAL_UNSET_PRELUDE).count(),
        2,
        "{action_body}"
    );
    assert!(!action_body.contains("secrets."), "{action_body}");
    assert!(!action_body.contains("github.token"), "{action_body}");
}

pub(super) fn contains_credential_assignment(action_body: &str, key: &str) -> bool {
    action_body.match_indices(key).any(|(start, _)| {
        let bytes = action_body.as_bytes();
        let starts_identifier =
            start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        !starts_identifier && bytes.get(start + key.len()) == Some(&b'=')
    })
}

fn assert_release_fixture_generation_requirements() -> Result<(), Box<dyn std::error::Error>> {
    let fixture_check = include_str!("../../../scripts/capture-opentofu-goldens.sh");
    assert!(
        fixture_check.contains("generate --output-dir"),
        "{fixture_check}"
    );
    assert!(fixture_check.contains("hostile-config"), "{fixture_check}");
    assert!(
        fixture_check.contains("unknown_config_field"),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains(r#"capture_release_case "$case" "$repo""#),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains(r#"stage_candidate_manifest "$repo" "$case""#),
        "{fixture_check}"
    );
    assert_schema_only_consumer_manifest_input(fixture_check);
    assert!(
        fixture_check.contains("capture_release_dogfood"),
        "{fixture_check}"
    );
    assert!(
        !fixture_check.contains("write_release_fixture_manifest"),
        "{fixture_check}"
    );
    let qualification_helpers =
        include_str!("../../../scripts/generator-release/qualification-goldens.sh");
    assert_candidate_output_binding_validation(qualification_helpers)?;
    assert!(
        qualification_helpers.contains(r#"stage_candidate_manifest "$repo" dogfood"#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers
            .contains(r#"cmp -s "$CANDIDATE_MANIFEST" "$repo/.velnor/release-manifest.json""#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("staged_sha\" != \"$CANDIDATE_MANIFEST_SHA256\""),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("GITHUB_SHA"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("host_target"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("candidate manifest digest does not match candidate CLI"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains(r#"commit_placeholder="$(printf '%040d' 0 | tr '0' 'b')""#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains(r#"jq -er '.commit | strings' "$CANDIDATE_MANIFEST""#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("candidate manifest contains a malformed source commit"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("could not normalize candidate manifest commit in $file"),
        "{qualification_helpers}"
    );
    assert_dogfood_cargo_warmup(qualification_helpers)?;
    Ok(())
}

fn assert_candidate_output_binding_validation(
    qualification_helpers: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let release_case = qualification_helpers
        .find("capture_release_case()")
        .ok_or("release candidate case capture must exist")?;
    let capture_window = &qualification_helpers[release_case..];
    let raw_binding_check = capture_window
        .find("validate_candidate_output_bindings")
        .ok_or("raw candidate output bindings must be validated")?;
    let normalization = capture_window
        .find("normalize_candidate_digests")
        .ok_or("candidate output normalization must exist")?;
    assert!(
        raw_binding_check < normalization,
        "raw candidate output bindings must be validated before normalization"
    );
    assert!(
        qualification_helpers.contains("target = targets.get(url)")
            && qualification_helpers.contains("if digest != expected_digest")
            && qualification_helpers.contains("if source != commit"),
        "every generated ConsumerV1 binding must match its candidate manifest target, digest, and source"
    );
    Ok(())
}

fn assert_schema_only_consumer_manifest_input(fixture_check: &str) {
    assert!(
        fixture_check.contains("write_schema_only_consumer_manifest()")
            && fixture_check.contains(r#"version="$("$BIN" --version | awk"#)
            && fixture_check.contains(
                "[[ ! \"$version\" =~ ^[0-9]+\\.[0-9]+\\.[0-9]+$ ]]",
            )
            && fixture_check.contains(
                "https://github.com/tailrocks/velnor-new/releases/download/v$version/velnor-actions-$version-$target",
            )
            && fixture_check.contains(r#"commit="$(printf '%040d' 0 | tr '0' 'b')""#)
            && fixture_check.contains(r#"digest="$(printf '%064d' 0 | tr '0' 'a')""#)
            && fixture_check.contains("write_schema_only_consumer_manifest \"$repo\""),
        "positive fixtures use the current-version schema-only marker contract: {fixture_check}"
    );
    assert!(
        !fixture_check.contains(r#"cp "$ROOT/fixtures/consumer-release-manifest.json""#),
        "positive fixture capture must not relabel the static schema placeholder: {fixture_check}"
    );
}

fn assert_dogfood_cargo_warmup(
    qualification_helpers: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        qualification_helpers.contains(
            "mise --no-config --no-env --no-hooks exec rust@1.99.0 mr-boxington@1.23.0 -- mbx fetch --locked"
        ),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains(
            r#"for manifest in "$repo/Cargo.toml" "$repo/crates/velnor-runner/Cargo.toml""#
        ),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("could not fetch dogfood cargo sources"),
        "{qualification_helpers}"
    );
    let dogfood = qualification_helpers
        .find("capture_release_dogfood()")
        .ok_or("dogfood capture must exist")?;
    let window = &qualification_helpers[dogfood..];
    let fetch = window
        .find("mbx fetch --locked")
        .ok_or("dogfood must warm the cargo cache")?;
    let generate = window
        .find("release candidate dogfood generate failed")
        .ok_or("dogfood must run the candidate generate")?;
    assert!(
        fetch < generate,
        "dogfood cargo fetch must precede the candidate generate"
    );
    Ok(())
}
