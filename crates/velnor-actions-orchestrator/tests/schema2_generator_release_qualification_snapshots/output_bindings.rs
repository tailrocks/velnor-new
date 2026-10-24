const CAPTURE_SCRIPT: &str = include_str!("../../../../scripts/capture-opentofu-goldens.sh");
const QUALIFICATION_HELPERS: &str =
    include_str!("../../../../scripts/generator-release/qualification-goldens.sh");
const CAPTURE_HARNESS: &str =
    include_str!("../../../../scripts/test-capture-opentofu-goldens-bin.sh");
const OUTPUT_BINDING_TESTS: &str =
    include_str!("../../../../scripts/generator-release/test-qualification-output-bindings.sh");
const MANIFEST_VALIDATOR: &str = include_str!(
    "../../../../crates/velnor-archive-guard/src/owned_archive_guard/qualification/manifest.rs"
);
const BINDING_VALIDATOR: &str = include_str!(
    "../../../../crates/velnor-archive-guard/src/owned_archive_guard/qualification/bindings.rs"
);
const STRICT_JSON: &str =
    include_str!("../../../../crates/velnor-archive-guard/src/owned_archive_guard/strict_json.rs");
const QUALIFICATION_TESTS: &str = include_str!(
    "../../../../crates/velnor-archive-guard/src/owned_archive_guard/qualification/tests.rs"
);
const STRICT_JSON_TESTS: &str = include_str!(
    "../../../../crates/velnor-archive-guard/src/owned_archive_guard/strict_json_tests.rs"
);

pub(super) fn assert_qualification_pipeline(
    qualification_helpers: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_checker_precedes_normalization(qualification_helpers)?;
    assert_checker_is_in_capture_path(qualification_helpers)?;
    assert_candidate_manifest_parent_is_physicalized()?;
    assert_native_checker_contract();
    assert_candidate_binding_faults_are_exercised();
    Ok(())
}

fn assert_checker_precedes_normalization(
    qualification_helpers: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(release_case) = qualification_helpers.find("capture_release_case()") else {
        return Err("release candidate case capture must exist".into());
    };
    let capture_window = &qualification_helpers[release_case..];
    let Some(raw_binding_check) = capture_window.find("validate_candidate_output_bindings") else {
        return Err("raw candidate output bindings must be validated".into());
    };
    let Some(normalization) = capture_window.find("normalize_candidate_digests") else {
        return Err("candidate output normalization must exist".into());
    };
    assert!(
        raw_binding_check < normalization,
        "raw candidate output bindings must be validated before normalization"
    );
    Ok(())
}

fn assert_checker_is_in_capture_path(
    qualification_helpers: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(
        qualification_helpers
            .contains("\"$QUALIFICATION_GUARD_BIN\" qualification-output-bindings")
            && qualification_helpers.contains("\"$CANDIDATE_MANIFEST\" \"$preview/.github\""),
        "trusted archive guard must receive the candidate manifest and generated workflow root"
    );
    let checker = qualification_helpers
        .find("validate_candidate_output_bindings()")
        .ok_or_else(|| std::io::Error::other("trusted output-binding checker must be defined"))?;
    let normalization = qualification_helpers[checker..]
        .find("normalize_candidate_digests()")
        .map(|offset| checker + offset)
        .ok_or_else(|| std::io::Error::other("candidate output normalization must be defined"))?;
    let checker_source = &qualification_helpers[checker..normalization];
    for handoff in [
        "prepare_qualification_guard || return 1",
        "verify_qualification_guard || return 1",
        "\"$QUALIFICATION_GUARD_BIN\" qualification-output-bindings",
        "\"$CANDIDATE_MANIFEST\" \"$preview/.github\"",
    ] {
        assert!(
            checker_source.contains(handoff),
            "trusted output-binding checker is missing {handoff}"
        );
    }
    assert!(
        !checker_source.contains("$BIN")
            && !checker_source.contains("VELNOR_REPO_POLICY_ACTION")
            && !checker_source.contains("VELNOR_INTERNAL_OP"),
        "candidate CLI must not be asked to validate its own output bindings"
    );
    assert!(
        CAPTURE_SCRIPT
            .contains("source \"$ROOT/scripts/generator-release/qualification-goldens.sh\""),
        "the workflow capture entrypoint must load the trusted qualification helper"
    );
    Ok(())
}

fn assert_candidate_manifest_parent_is_physicalized() -> Result<(), Box<dyn std::error::Error>> {
    let leaf_check = QUALIFICATION_HELPERS
        .find("[ ! -f \"$CANDIDATE_MANIFEST\" ] || [ -L \"$CANDIDATE_MANIFEST\" ]")
        .ok_or_else(|| {
            std::io::Error::other(
                "candidate manifest must reject nonregular and leaf-symlink paths",
            )
        })?;
    let parent_resolution = QUALIFICATION_HELPERS
        .find("cd -- \"$manifest_parent\" 2>/dev/null && pwd -P")
        .ok_or_else(|| {
            std::io::Error::other(
                "candidate manifest parent must be resolved before native no-follow checks",
            )
        })?;
    let normalized_path = QUALIFICATION_HELPERS
        .find("CANDIDATE_MANIFEST=\"$physical_manifest_parent/$manifest_name\"")
        .ok_or_else(|| {
            std::io::Error::other("physical parent must retain the original manifest basename")
        })?;
    assert!(
        leaf_check < parent_resolution && parent_resolution < normalized_path,
        "manifest leaf validation must precede parent-only physicalization"
    );
    assert!(
        CAPTURE_HARNESS.contains("$WORK/tmp symlink/valid manifest.json"),
        "the capture harness must accept a regular manifest through an aliased parent"
    );
    assert!(
        CAPTURE_HARNESS.contains("symlink-manifest")
            && CAPTURE_HARNESS.contains("candidate manifest must be a regular non-symlink file"),
        "the capture harness must continue rejecting a symlinked manifest leaf"
    );
    Ok(())
}

fn assert_native_checker_contract() {
    for contract in [
        "value.get(\"schema\").and_then(Value::as_u64) != Some(1)",
        "Some(\"tailrocks/velnor-new\")",
        "filter(|value| is_release_version(value))",
        "filter(|value| is_lower_hex(value, 40))",
        "targets.len() == EXPECTED_TARGETS.len()",
        "target != Some(expected)",
        "artifact != Some(expected_url.as_str())",
        "is_lower_hex(value, 64)",
        "by_url.insert(artifact.to_owned(), binding).is_some()",
    ] {
        assert!(
            MANIFEST_VALIDATOR.contains(contract),
            "native candidate-manifest validator is missing {contract}"
        );
    }
    for contract in [
        "\"VELNOR_ASSET_SHA256\"",
        "\"VELNOR_ASSET_URL\"",
        "\"VELNOR_RELEASE_COMMIT\"",
        "parsed.iter().map(|entry| entry.1).ne(expected)",
        "parsed.iter().any(|entry| entry.0 != parsed[0].0)",
        "manifest.by_url.get(parsed[1].2)",
        "wrong digest for target",
        "wrong source commit",
        "no ConsumerV1 download bindings were emitted",
    ] {
        assert!(
            BINDING_VALIDATOR.contains(contract),
            "native output-binding validator is missing {contract}"
        );
    }
    for contract in [
        "strict_json::parse(&bytes)",
        "malformed or duplicate-key JSON",
        "manifest_symlink_is_not_followed",
        "rejects_duplicate_keys_and_manifest_identity_inventory_faults",
        "rejects_binding_digest_commit_url_scope_and_missing_field_faults",
        "exact_manifest_and_adjacent_binding_are_accepted",
    ] {
        assert!(
            MANIFEST_VALIDATOR.contains(contract)
                || BINDING_VALIDATOR.contains(contract)
                || QUALIFICATION_TESTS.contains(contract),
            "native qualification validation is missing {contract}"
        );
    }
    assert!(STRICT_JSON.contains("duplicate object key"));
    let compact_strict_json = STRICT_JSON.split_whitespace().collect::<String>();
    assert!(compact_strict_json.contains("deserializer.end()"));
    assert!(
        STRICT_JSON_TESTS.contains(
            "strict_json_rejects_duplicate_escaped_keys_non_finite_numbers_and_bad_framing"
        )
    );
    let guard_api = include_str!("../../../../crates/velnor-archive-guard/src/lib.rs");
    assert!(
        guard_api.contains("validate_qualification_manifest")
            && guard_api.contains("validate_qualification_output_bindings"),
        "trusted archive guard must own and expose candidate qualification validation"
    );
}

fn assert_candidate_binding_faults_are_exercised() {
    assert!(
        CAPTURE_HARNESS.contains(
            "source \"$ROOT/scripts/generator-release/test-qualification-output-bindings.sh\""
        ) && CAPTURE_HARNESS.contains("run_candidate_output_binding_tests || exit $?"),
        "independent binding fault tests must run through the capture harness"
    );
    for fault in [
        "wrong-digest",
        "wrong-commit",
        "both",
        "missing-digest",
        "missing-url",
        "missing-commit",
        "wrong-url",
        "wrong-target",
    ] {
        assert!(
            OUTPUT_BINDING_TESTS.contains(fault),
            "candidate output-binding tests are missing {fault}"
        );
    }
    for contract in [
        "expect_output_binding_fault_rejected \"$fault\"",
        "expect_duplicate_manifest_keys_rejected",
        "[ \"$status\" -ne 2 ]",
        "candidate output binding $fault reached normalization",
        "could not normalize candidate asset digests",
        "release qualification asked candidate repo-policy to validate output bindings",
        "release candidate output bindings do not match candidate manifest",
    ] {
        assert!(
            OUTPUT_BINDING_TESTS.contains(contract),
            "candidate output-binding tests are missing {contract}"
        );
    }
}
