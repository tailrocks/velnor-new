const CHECKER: &str =
    include_str!("../../../../scripts/generator-release/qualification-checker.py");
const CAPTURE_SCRIPT: &str = include_str!("../../../../scripts/capture-opentofu-goldens.sh");
const CAPTURE_HARNESS: &str =
    include_str!("../../../../scripts/test-capture-opentofu-goldens-bin.sh");
const OUTPUT_BINDING_TESTS: &str =
    include_str!("../../../../scripts/generator-release/test-qualification-output-bindings.sh");

pub(super) fn assert_qualification_pipeline(
    qualification_helpers: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_checker_precedes_normalization(qualification_helpers)?;
    assert_checker_is_in_capture_path(qualification_helpers);
    assert_checker_contract();
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

fn assert_checker_is_in_capture_path(qualification_helpers: &str) {
    assert!(
        qualification_helpers.contains("qualification-checker.py")
            && qualification_helpers
                .contains("python3 \"$ROOT/scripts/generator-release/qualification-checker.py\"")
            && qualification_helpers
                .contains("output-bindings \"$CANDIDATE_MANIFEST\" \"$preview/.github\""),
        "trusted qualification checker must receive the candidate manifest and generated workflow root"
    );
    assert!(
        CAPTURE_SCRIPT
            .contains("source \"$ROOT/scripts/generator-release/qualification-goldens.sh\""),
        "the workflow capture entrypoint must load the checker-owning helper"
    );
}

fn assert_checker_contract() {
    for contract in [
        "def validate_candidate_manifest(",
        "manifest[\"schema\"] != 1",
        "manifest.get(\"repository\") != \"tailrocks/velnor-new\"",
        "version = manifest.get(\"version\")",
        "commit = manifest.get(\"commit\")",
        "not re.fullmatch(r\"[0-9a-f]{40}\", commit)",
        "len(targets) != len(EXPECTED_TARGETS)",
        "not re.fullmatch(r\"[0-9a-f]{64}\", digest)",
        "artifact != expected_url",
        "target != expected_target",
        "def validate_file_bindings(",
        "tuple(item[1] for item in parsed) != BOUND_FIELDS",
        "any(item[0] != parsed[0][0] for item in parsed)",
        "target = by_url.get(url)",
        "if digest != expected_digest:",
        "if source != commit:",
        "if bindings == 0:",
    ] {
        assert!(CHECKER.contains(contract), "checker is missing {contract}");
    }
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
