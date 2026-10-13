//! Structural checks for the exact-key provider-cache composite.

use super::{TOFU_PROVIDER_ADMISSION_SCRIPT, TOFU_PROVIDERS_SAVE_USES, provider_admission_file};

#[test]
fn composite_checks_the_actual_restore_before_exposing_provider_use() {
    let file = provider_admission_file("1.2.3").expect("provider composite renders");
    assert_eq!(
        file.path,
        ".github/actions/tofu-provider-admission/action.yml"
    );
    let yaml = file.bytes;
    let restore = yaml
        .find("actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9")
        .expect("restore uses the save action's exact SHA");
    let admission = yaml
        .find("Discard provider bytes unless the exact restore key matched")
        .expect("composite has an admission step");
    assert!(restore < admission, "admission follows the actual restore");
    for required in [
        "id: restore",
        "key: ${{ inputs.cache-key }}",
        "path: ${{ inputs.cache-path }}",
        "steps.restore.outputs.cache-hit",
        "steps.restore.outputs.cache-matched-key",
        "TOFU_EXPECTED_KEY",
        "TOFU_PROVIDER_CACHE_PATH",
        "${{ inputs.cache-key }}",
        "${{ inputs.cache-path }}",
    ] {
        assert!(yaml.contains(required), "composite includes {required:?}");
    }
    assert!(
        !yaml.contains("restore-keys"),
        "the restore has no fallback keys"
    );
    assert!(
        !yaml.contains("TF_DATA_DIR"),
        "admission never touches OpenTofu's data directory"
    );
    assert!(
        TOFU_PROVIDERS_SAVE_USES.starts_with("actions/cache/save@"),
        "the paired save remains pinned to the same cache release"
    );
}

#[test]
fn admission_script_discards_non_exact_or_missing_provider_entries() {
    for condition in [
        "[ \"$TOFU_CACHE_HIT\" = true ]",
        "[ -n \"$TOFU_EXPECTED_KEY\" ]",
        "[ \"$TOFU_MATCHED_KEY\" = \"$TOFU_EXPECTED_KEY\" ]",
        "[ -d \"$d\" ]",
        "[ ! -L \"$d\" ]",
    ] {
        assert!(
            TOFU_PROVIDER_ADMISSION_SCRIPT.contains(condition),
            "admission requires {condition:?}"
        );
    }
    assert!(TOFU_PROVIDER_ADMISSION_SCRIPT.contains("rm -rf \"$d\""));
    assert!(TOFU_PROVIDER_ADMISSION_SCRIPT.contains("mkdir -m 700 \"$d\""));
}
