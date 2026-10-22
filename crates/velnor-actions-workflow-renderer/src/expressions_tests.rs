use super::{
    check_composite_env_value, check_composite_qualification_cache_env_value, check_env_value,
    check_with_action_value, check_with_value,
};

#[test]
fn event_sha_is_allowed_only_for_checkout_ref() {
    assert!(
        check_with_action_value(
            "actions/checkout@0123456789012345678901234567890123456789",
            "ref",
            "${{ github.sha }}"
        )
        .is_ok()
    );
    assert!(
        check_with_action_value(
            "actions/upload-artifact@0123456789012345678901234567890123456789",
            "ref",
            "${{ github.sha }}"
        )
        .is_err()
    );
    assert!(
        check_with_action_value(
            "actions/checkout@0123456789012345678901234567890123456789",
            "ref",
            "prefix-${{ github.sha }}"
        )
        .is_err()
    );
}

#[test]
fn qualification_cache_runtime_paths_accept_only_their_typed_step_output() {
    assert!(check_env_value(
            "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE",
            "${{ steps.qualification-cache-roots-mbx_objects-0123456789abcdef.outputs.root_observation_path }}"
        )
        .is_ok());
    assert!(check_env_value(
            "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE",
            "${{ steps.qualification-cache-runtime-identity-task_result-0123456789abcdef.outputs.runtime_identity_path }}"
        )
        .is_ok());
    for (key, expression) in [
        (
            "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE",
            "${{ steps.qualification-cache-roots-mbx_objects-0123456789abcdef.outputs.runtime_identity_path }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE",
            "${{ steps.untrusted.outputs.root_observation_path }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE",
            "${{ steps.qualification-cache-bind-mbx_objects-0123456789abcdef.outputs.root_observation_path }}",
        ),
        (
            "UNRELATED_PATH",
            "${{ steps.qualification-cache-roots-mbx_objects-0123456789abcdef.outputs.root_observation_path }}",
        ),
        (
            "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE",
            "${{ steps.qualification-cache-runtime-identity-mbx_objects-0123456789abcg.outputs.runtime_identity_path }}",
        ),
    ] {
        assert!(
            check_env_value(key, expression).is_err(),
            "{key}: {expression}"
        );
    }
}

#[test]
fn check_with_value_accepts_glob_hash_files() {
    assert!(
        check_with_value(
            "cache-key",
            "prefix-${{ hashFiles('**/Cargo.lock','**/Cargo.toml') }}"
        )
        .is_ok()
    );
}

#[test]
fn cache_tar_launcher_accepts_only_its_fixed_digest_inputs() {
    assert!(
        check_with_value(
            "helper-sha256",
            "${{ steps.preseed-stage.outputs.helper-sha256 }}"
        )
        .is_ok()
    );
    assert!(
        check_composite_env_value("VELNOR_CACHE_TAR_SHA256", "${{ inputs.helper-sha256 }}").is_ok()
    );
    assert!(check_composite_env_value("VELNOR_CACHE_TAR_SHA256", "${{ inputs.other }}").is_err());
}

#[test]
fn check_with_value_accepts_quoted_hash_file_exclusions() {
    assert!(
        check_with_value(
            "cache-key",
            "prefix-${{ hashFiles('**/Cargo.toml','!target/**') }}"
        )
        .is_ok()
    );
}

#[test]
fn check_with_value_rejects_nonliteral_hash_file_arguments() {
    for arguments in [
        "",
        "''",
        "'!'",
        "'!!target/**'",
        "!'Cargo.toml'",
        "'Cargo.toml',",
        "'Cargo.toml'||'other'",
        "github.token",
    ] {
        let value = format!("prefix-${{{{ hashFiles({arguments}) }}}}");
        assert!(check_with_value("cache-key", &value).is_err(), "{value}");
    }
}

#[test]
fn attestation_subject_digest_accepts_only_fixed_image_assembly_outputs() {
    for output in ["digest", "digest_amd64", "digest_arm64"] {
        let expression = format!("${{{{ steps.image.outputs.{output} }}}}");
        assert!(
            check_with_value("subject-digest", &expression).is_ok(),
            "{expression}"
        );
    }

    for (key, expression) in [
        ("subject-name", "${{ steps.image.outputs.digest }}"),
        ("subject-digest", "${{ steps.other.outputs.digest }}"),
        (
            "subject-digest",
            "${{ steps.image.outputs.digest_linux_amd64 }}",
        ),
    ] {
        assert!(
            check_with_value(key, expression).is_err(),
            "{key}: {expression}"
        );
    }
}

#[test]
fn qualification_cache_operation_expressions_are_typed_and_closed() {
    let check = super::check_qualification_cache_env_value;
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_EXPORT_STATE_DIGEST",
        "${{ steps.qualification-cache-export-mbx_objects-0123456789abcdef.outputs.state_digest }}"
    )
    .is_ok());
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
        "${{ steps.qualification-cache-export-mbx_objects-0123456789abcdef.outputs.save_requested == 'true' && steps.qualification-cache-save-mbx_objects-0123456789abcdef.outcome || '' }}"
    )
    .is_ok());
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_HIT",
        "${{ steps.qualification-cache-import-mbx_objects-0123456789abcdef.outputs.restore_action_hit }}"
    )
    .is_ok());
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY",
        "${{ steps.qualification-cache-import-mbx_objects-0123456789abcdef.outputs.restore_action_matched_key }}"
    )
    .is_ok());
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_HIT",
        "${{ steps.qualification-cache-restore-mbx_objects-0123456789abcdef.outputs.cache-hit }}"
    )
    .is_ok());
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY",
        "${{ steps.qualification-cache-restore-mbx_objects-0123456789abcdef.outputs.cache-matched-key }}"
    )
    .is_ok());
    for (key, expression) in [
        (
            "VELNOR_QUALIFICATION_CACHE_EXPORT_STATE_DIGEST",
            "${{ steps.qualification-cache-export-mbx_objects-0123456789abcdef.outputs.save_requested }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY",
            "${{ steps.qualification-cache-restore-mbx_objects-0123456789abcdef.outputs.cache-hit }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_EXPORT_STATE_DIGEST",
            "${{ steps.untrusted.outputs.state_digest }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_RESTORE_CENSUS_CACHE_ID",
            "${{ steps.qualification-cache-restore-census-mbx_objects-0123456789abcdef.outputs.backend_cache_id }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_RESTORE_CENSUS_CACHE_ID",
            "${{ steps.qualification-cache-save-census-mbx_objects-0123456789abcdef.outputs.backend_cache_id }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
            "${{ steps.qualification-cache-export-mbx_objects-0123456789abcdef.outputs.save_requested == 'true' && steps.qualification-cache-save-cargo_sources-0123456789abcdef.outcome || '' }}",
        ),
        ("VELNOR_QUALIFICATION_CACHE_KEY", "${{ github.sha }}"),
    ] {
        assert!(check(key, expression).is_err(), "{key}: {expression}");
    }
}

#[test]
fn restore_action_facts_use_the_exact_layered_prepare_boundary() {
    assert!(check_composite_qualification_cache_env_value(
        "VELNOR_QUALIFICATION_CACHE_HIT",
        "${{ steps.qualification-cache-restore-mbx_objects-0123456789abcdef.outputs.cache-hit }}"
    )
    .is_ok());
    assert!(check_composite_qualification_cache_env_value(
        "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY",
        "${{ steps.qualification-cache-restore-mbx_objects-0123456789abcdef.outputs.cache-matched-key }}"
    )
    .is_ok());
    for (key, suffix) in [
        ("VELNOR_QUALIFICATION_CACHE_HIT", "action_hit"),
        (
            "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY",
            "action_matched_key",
        ),
    ] {
        let input = format!(
            "${{{{ fromJSON(inputs.prepare_outputs).cache_mbx_objects_restore_census_{suffix} }}}}"
        );
        assert!(check_composite_qualification_cache_env_value(key, &input).is_ok());
        let boundary = format!(
            "${{{{ steps.qualification-cache-prepare-0123456789abcdef.outputs.cache_mbx_objects_restore_census_{suffix} }}}}"
        );
        assert!(check_composite_qualification_cache_env_value(key, &boundary).is_ok());
    }

    assert!(check_composite_qualification_cache_env_value(
        "VELNOR_QUALIFICATION_CACHE_RESTORE_CENSUS_CACHE_ID",
        "${{ fromJSON(inputs.prepare_outputs).cache_mbx_objects_restore_census_backend_cache_id }}"
    )
    .is_err());

    for (key, expression) in [
        (
            "VELNOR_QUALIFICATION_CACHE_HIT",
            "${{ fromJSON(inputs.prepare_outputs).cache_mbx_objects_restore_census_backend_status }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_HIT",
            "${{ steps.qualification-cache-import-mbx_objects-0123456789abcdef.outputs.cache-hit }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY",
            "${{ fromJSON(inputs.before_outputs).cache_mbx_objects_restore_census_action_matched_key }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_HIT",
            "${{ steps.qualification-cache-prepare-0123456789abcdef.outputs.cache_mbx_objects_restore_census_backend_status }}",
        ),
    ] {
        assert!(
            check_composite_qualification_cache_env_value(key, expression).is_err(),
            "{key}: {expression}"
        );
    }
}

#[path = "expressions_qualification_cache_tests.rs"]
mod qualification_cache_tests;
