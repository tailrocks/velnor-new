use super::{
    check_composite_env_value, check_composite_qualification_cache_env_value, check_env_value,
    check_with_value,
};

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
fn qualification_cache_operation_expressions_are_typed_and_closed() {
    let check = super::check_qualification_cache_env_value;
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_EXPORT_STATE_DIGEST",
        "${{ steps.qualification-cache-export-mbx_objects-0123456789abcdef.outputs.state_digest }}"
    )
    .is_ok());
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_RESTORE_CENSUS_CACHE_ID",
        "${{ steps.qualification-cache-restore-census-mbx_objects-0123456789abcdef.outputs.backend_cache_id }}"
    )
    .is_ok());
    assert!(check(
        "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
        "${{ steps.qualification-cache-export-mbx_objects-0123456789abcdef.outputs.save_requested == 'true' && steps.qualification-cache-save-mbx_objects-0123456789abcdef.outcome || '' }}"
    )
    .is_ok());
    for (key, expression) in [
        (
            "VELNOR_QUALIFICATION_CACHE_EXPORT_STATE_DIGEST",
            "${{ steps.qualification-cache-export-mbx_objects-0123456789abcdef.outputs.save_requested }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_EXPORT_STATE_DIGEST",
            "${{ steps.untrusted.outputs.state_digest }}",
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
fn qualification_cache_composite_accepts_only_typed_output_maps() {
    assert!(super::qualification_cache::is_composite_output_map(
        "before_outputs",
        "toJSON(steps.qualification-cache-before-save-cargo_sources-0123456789abcdef.outputs)"
    ));
    assert!(
        super::check_composite_with_value(
            "prepare_outputs",
            "${{ toJSON(steps.qualification-cache-prepare-0123456789abcdef.outputs) }}"
        )
        .is_ok()
    );
    assert!(super::check_composite_with_value(
        "before_outputs",
        "${{ toJSON(steps.qualification-cache-before-save-mbx_objects-0123456789abcdef.outputs) }}"
    )
    .is_ok());
    for (key, value) in [
        (
            "prepare_outputs",
            "${{ toJSON(steps.qualification-cache-before-save-mbx_objects-0123456789abcdef.outputs) }}",
        ),
        (
            "before_outputs",
            "${{ toJSON(steps.qualification-cache-prepare-0123456789abcdef.outputs) }}",
        ),
        (
            "arbitrary",
            "${{ toJSON(steps.qualification-cache-prepare-0123456789abcdef.outputs) }}",
        ),
        ("prepare_outputs", "${{ toJSON(steps.other-step.outputs) }}"),
    ] {
        assert!(
            super::check_composite_with_value(key, value).is_err(),
            "{key}: {value}"
        );
    }
}

#[test]
fn qualification_cache_composite_accepts_typed_input_projection() {
    assert!(super::qualification_cache::is_composite_input(
        "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
        "fromJSON(inputs.before_outputs).cache_cargo_sources_save_requested == 'true' && inputs.cache_cargo_sources_save_action_outcome || ''"
    ));
    assert!(check_with_value(
        "key",
        "${{ steps.qualification-cache-prepare-0123456789abcdef.outputs.cache_mbx_objects_save_key }}"
    )
    .is_ok());
    assert!(check_with_value(
        "path",
        "${{ steps.qualification-cache-prepare-0123456789abcdef.outputs.cache_mbx_objects_save_stage_path }}"
    )
    .is_ok());
    assert!(
        check_with_value(
            "cache_mbx_objects_save_action_outcome",
            "${{ steps.qualification-cache-save-mbx_objects-0123456789abcdef.outcome }}"
        )
        .is_ok()
    );
    assert!(check_composite_qualification_cache_env_value(
        "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
        "${{ fromJSON(inputs.before_outputs).cache_mbx_objects_save_requested == 'true' && inputs.cache_mbx_objects_save_action_outcome || '' }}"
    )
    .is_ok());
    assert!(check_composite_qualification_cache_env_value(
        "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
        "${{ fromJSON(inputs.before_outputs).cache_cargo_sources_save_requested == 'true' && inputs.cache_cargo_sources_save_action_outcome || '' }}"
    )
    .is_ok());
    assert!(check_composite_qualification_cache_env_value(
        "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
        "${{ inputs.cache_mbx_objects_save_requested == 'true' && inputs.cache_mbx_objects_save_action_outcome || '' }}"
    )
    .is_err());
    assert!(
        check_composite_qualification_cache_env_value(
            "VELNOR_QUALIFICATION_CACHE_ACTIVE",
            "${{ fromJSON(inputs.prepare_outputs).cache_mbx_objects_active }}"
        )
        .is_ok()
    );
    assert!(check_with_value(
        "cache_mbx_objects_export_result",
        "${{ steps.qualification-cache-before-save-mbx_objects-0123456789abcdef.outputs.cache_mbx_objects_export_result }}"
    )
    .is_ok());
    assert!(check_composite_qualification_cache_env_value(
        "VELNOR_QUALIFICATION_CACHE_LAYER_MBX_OBJECTS_EVIDENCE",
        "${{ steps.qualification-cache-after-save-mbx_objects-0123456789abcdef.outputs.cache_mbx_objects_owner_evidence_path }}"
    )
    .is_ok());
}

#[test]
fn qualification_cache_composite_rejects_wrong_owner_layer_and_projection() {
    for (key, value) in [
        (
            "key",
            "${{ steps.qualification-cache-prepare-0123456789abcdef.outputs.cache_mbx_objects_runtime_before }}",
        ),
        (
            "path",
            "${{ steps.qualification-cache-prepare-0123456789abcdef.outputs.cache_mbx_objects_save_key }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
            "${{ fromJSON(inputs.before_outputs).cache_cargo_sources_save_requested == 'true' && inputs.cache_mbx_objects_save_action_outcome || '' }}",
        ),
        (
            "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION",
            "${{ inputs.cache_mbx_objects_save_action_outcome }}",
        ),
        (
            "cache_mbx_objects_export_result",
            "${{ steps.qualification-cache-prepare-0123456789abcdef.outputs.cache_mbx_objects_export_result }}",
        ),
        (
            "cache_mbx_objects_export_result",
            "${{ steps.qualification-cache-before-save-cargo_sources-0123456789abcdef.outputs.cache_mbx_objects_export_result }}",
        ),
        (
            "cache_mbx_objects_save_action_outcome",
            "${{ steps.qualification-cache-save-cargo_sources-0123456789abcdef.outcome }}",
        ),
    ] {
        let result = if key.starts_with("VELNOR_") {
            check_composite_qualification_cache_env_value(key, value)
        } else {
            check_with_value(key, value)
        };
        assert!(result.is_err(), "{key}: {value}");
    }
}
