use super::{check_composite_qualification_cache_env_value, check_with_value};

#[test]
fn qualification_cache_composite_accepts_only_typed_output_maps() {
    assert!(super::super::qualification_cache::is_composite_output_map(
        "before_outputs",
        "toJSON(steps.qualification-cache-before-save-cargo_sources-0123456789abcdef.outputs)"
    ));
    assert!(
        super::super::check_composite_with_value(
            "prepare_outputs",
            "${{ toJSON(steps.qualification-cache-prepare-0123456789abcdef.outputs) }}"
        )
        .is_ok()
    );
    assert!(super::super::check_composite_with_value(
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
            super::super::check_composite_with_value(key, value).is_err(),
            "{key}: {value}"
        );
    }
}

#[test]
fn qualification_cache_composite_accepts_typed_input_projection() {
    assert!(super::super::qualification_cache::is_composite_input(
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
