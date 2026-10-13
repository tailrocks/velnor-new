//! Security assertions for the generated single-bundle MBX round trip.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

const QUALIFICATION_SCOPE: &str = "qualification-mbx-v1/single-bundle-roundtrip";
const MBX_VERSION: &str = "1.22.0";
const MBX_GENERATION: &str = "velnor-mbx-1.22.0";
const MBX_ACTION_PIN: &str = "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6";
const CACHE_RESTORE_PIN: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const CACHE_SAVE_PIN: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";

#[test]
fn protected_main_writer_and_fresh_reader_use_production_bundle_graph() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;
    assert!(
        qualification.contains("workflow_dispatch:"),
        "{qualification}"
    );
    assert!(qualification.contains("mode:"), "{qualification}");
    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;

    assert_writer(writer);
    assert_reader(reader);
    assert_shared_identity(writer, reader);
    Ok(())
}

fn assert_writer(writer: &str) {
    assert_qualification_gate(writer);
    assert!(writer.contains("runs-on: ubuntu-26.04"), "{writer}");
    assert!(writer.contains("actions: write"), "{writer}");
    assert!(writer.contains("MBX_GC_AUTO: \"1\""), "{writer}");
    assert!(writer.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{writer}");
    assert!(
        writer.contains("MBX_CACHE_SCOPE: qualification-mbx-v1/single-bundle-roundtrip"),
        "{writer}"
    );
    assert!(
        writer.contains("MBX_MATRIX_CONTEXT: ${{ toJSON(matrix) }}"),
        "{writer}"
    );
    assert!(writer.contains("rustc --print sysroot"), "{writer}");
    assert_compression_profile(writer);
    assert_mbx_action_pin(writer);
    assert_external_cache_actions(writer, true);
    assert_eq!(writer.matches("if: success() &&").count(), 2, "{writer}");
    assert!(
        writer.contains("success() && inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true && steps.mbx-bundle.outputs.cache-hit != 'true'"),
        "{writer}"
    );
    assert!(
        writer.contains("steps.mbx-export.outputs.ready == 'true'"),
        "{writer}"
    );
    assert_eq!(
        writer
            .matches("inputs.mode == 'mbx-cache-roundtrip'")
            .count(),
        3,
        "{writer}"
    );
    assert!(!writer.contains("github.event_name == 'push'"), "{writer}");
    assert!(
        !writer.contains("github.event_name == 'pull_request'"),
        "{writer}"
    );
    assert!(!writer.contains("continue-on-error"), "{writer}");
    assert!(!writer.contains("if: always()"), "{writer}");
    assert_no_implicit_cache(writer);
    assert!(
        step_body(writer, "Prepare private MBX store").contains("velnor-mbx-store.XXXXXXXXXX"),
        "{writer}"
    );
    assert!(
        step_body(writer, "Import MBX single bundle").contains("velnor-mbx-fallback.XXXXXXXXXX"),
        "{writer}"
    );
    assert_ordered(
        writer,
        &[
            "name: Verify cache compression support",
            "name: Prepare private MBX store",
            "name: Setup MBX",
            "name: Prepare MBX bundle key",
            "name: Restore MBX single bundle",
            "name: Import MBX single bundle",
            "name: Verify pinned MBX version",
            "name: Compile MBX cache probe",
            "name: Export MBX single bundle",
            "name: Save MBX single bundle",
        ],
    );
}

fn assert_reader(reader: &str) {
    assert_qualification_gate(reader);
    assert_eq!(
        reader
            .matches("inputs.mode == 'mbx-cache-roundtrip'")
            .count(),
        1,
        "{reader}"
    );
    assert!(
        reader.contains("needs:\n      - mbx-cache-write-hosted"),
        "{reader}"
    );
    assert!(reader.contains("runs-on: ubuntu-26.04"), "{reader}");
    assert!(reader.contains("actions: read"), "{reader}");
    assert!(!reader.contains("actions: write"), "{reader}");
    assert!(reader.contains("MBX_GC_AUTO: \"1\""), "{reader}");
    assert!(
        reader.contains("MBX_CACHE_SCOPE: qualification-mbx-v1/single-bundle-roundtrip"),
        "{reader}"
    );
    assert_compression_profile(reader);
    assert_mbx_action_pin(reader);
    assert_external_cache_actions(reader, false);
    assert!(reader.contains("mbx cache stats --json"), "{reader}");
    assert!(reader.contains(".objects > 0"), "{reader}");
    assert!(
        reader.contains("MBX bundle import failed; abandoning its private store"),
        "{reader}"
    );
    assert!(
        reader.contains(".savings.cached_compilations > 0"),
        "{reader}"
    );
    assert!(!reader.contains("continue-on-error"), "{reader}");
    assert_no_implicit_cache(reader);
    assert!(
        step_body(reader, "Prepare private MBX store").contains("velnor-mbx-store.XXXXXXXXXX"),
        "{reader}"
    );
    assert!(
        step_body(reader, "Import MBX single bundle").contains("velnor-mbx-fallback.XXXXXXXXXX"),
        "{reader}"
    );
    assert!(
        !reader.contains("name: Export MBX single bundle"),
        "{reader}"
    );
    assert!(!reader.contains("name: Save MBX single bundle"), "{reader}");
    assert_ordered(
        reader,
        &[
            "name: Verify cache compression support",
            "name: Prepare private MBX store",
            "name: Setup MBX",
            "name: Prepare MBX bundle key",
            "name: Restore MBX single bundle",
            "name: Import MBX single bundle",
            "name: Verify pinned MBX version",
            "name: Require imported MBX objects",
            "name: Compile MBX cache probe",
            "name: Require reused compilation",
        ],
    );
}

fn assert_shared_identity(writer: &str, reader: &str) {
    assert_eq!(mbx_action_version(writer), mbx_action_version(reader));
    assert_eq!(mbx_action_version(writer), Some(MBX_VERSION));
    for key in [
        "MBX_VERSION",
        "MBX_EXPECTED_VERSION",
        "MBX_GENERATION",
        "MBX_CACHE_SCOPE",
        "MBX_MATRIX_CONTEXT",
        "RUSTUP_TOOLCHAIN",
        "MISE_RUSTUP_HOME",
        "MISE_CARGO_HOME",
    ] {
        assert_same_step_value(writer, reader, "Prepare MBX bundle key", key);
    }
    assert_eq!(
        step_value(writer, "Prepare MBX bundle key", "MBX_CACHE_SCOPE"),
        Some(QUALIFICATION_SCOPE)
    );
    assert_eq!(
        step_value(writer, "Prepare MBX bundle key", "MBX_EXPECTED_VERSION"),
        Some(MBX_VERSION)
    );
    assert_eq!(
        step_value(writer, "Prepare MBX bundle key", "MBX_GENERATION"),
        Some(MBX_GENERATION)
    );
    assert_eq!(
        step_value(writer, "Prepare MBX bundle key", "MBX_MATRIX_CONTEXT"),
        Some("${{ toJSON(matrix) }}")
    );
    let writer_restore = step_body(writer, "Restore MBX single bundle");
    let reader_restore = step_body(reader, "Restore MBX single bundle");
    assert!(
        writer_restore.contains(CACHE_RESTORE_PIN),
        "{writer_restore}"
    );
    assert!(
        reader_restore.contains(CACHE_RESTORE_PIN),
        "{reader_restore}"
    );
    assert_same_step_value(writer, reader, "Restore MBX single bundle", "path");
    assert_same_step_value(writer, reader, "Restore MBX single bundle", "key");
    assert_eq!(
        step_value(writer, "Restore MBX single bundle", "restore-keys"),
        None,
        "qualification writer must not restore a previous run's cache"
    );
    assert_eq!(
        step_value(reader, "Restore MBX single bundle", "restore-keys"),
        None,
        "qualification reader must use only the designated writer's exact key"
    );
    assert_eq!(
        step_value(writer, "Restore MBX single bundle", "path"),
        Some(BUNDLE_PATH)
    );
    let writer_save = step_body(writer, "Save MBX single bundle");
    assert!(writer_save.contains(CACHE_SAVE_PIN), "{writer_save}");
    assert_eq!(
        step_value(writer, "Save MBX single bundle", "if"),
        Some(concat!(
            "success() && inputs.mode == 'mbx-cache-roundtrip' && ",
            "github.event_name == 'workflow_dispatch' && ",
            "github.ref == 'refs/heads/main' && github.ref_protected == true && ",
            "steps.mbx-bundle.outputs.cache-hit != 'true' && ",
            "steps.mbx-export.outputs.ready == 'true'"
        ))
    );
    assert!(
        writer_save.contains("key: ${{ steps.mbx-bundle-key.outputs.primary }}"),
        "{writer_save}"
    );
    assert!(
        writer_save.contains("path: ${{ runner.temp }}/mbx-single-bundle"),
        "{writer_save}"
    );
    assert_key_contract(step_body(writer, "Prepare MBX bundle key"));
    assert_key_contract(step_body(reader, "Prepare MBX bundle key"));
}

fn assert_external_cache_actions(job: &str, writer: bool) {
    let restore = step_body(job, "Restore MBX single bundle");
    assert!(restore.contains(CACHE_RESTORE_PIN), "{restore}");
    assert_eq!(action_input_names(restore), vec!["key", "path"]);
    assert_eq!(
        job.matches("uses: actions/cache/restore@").count(),
        1,
        "{job}"
    );
    assert_eq!(
        job.matches("uses: actions/cache/save@").count(),
        usize::from(writer),
        "{job}"
    );
    if writer {
        assert_eq!(
            action_input_names(step_body(job, "Save MBX single bundle")),
            vec!["key", "path"]
        );
    }
}

fn assert_compression_profile(job: &str) {
    let probe = step_body(job, "Verify cache compression support");
    assert!(probe.contains("command -v zstd"), "{probe}");
    assert!(probe.contains("zstd --version"), "{probe}");
    assert!(probe.contains("GNU tar"), "{probe}");
    assert!(!job.contains("enableCrossOsArchive: true"), "{job}");
    for name in ["Restore MBX single bundle", "Save MBX single bundle"] {
        if let Some(start) = job.find(&format!("name: {name}")) {
            assert!(
                !step_body(job, name).contains("enableCrossOsArchive:"),
                "{}",
                &job[start..]
            );
        }
    }
}

fn assert_key_contract(step: &str) {
    for required in [
        "RUNNER_OS:$RUNNER_ARCH",
        "Linux:X64",
        "arch=x64",
        "GITHUB_WORKFLOW_REF",
        "GITHUB_SHA",
        "MBX_BASE_SHA",
        "workflow_path",
        "MBX_CACHE_SCOPE",
        "MBX_MATRIX_CONTEXT",
        "mise --no-config --no-env --no-hooks exec",
        "rustc -vV",
        "sha256sum",
        "MBX_GENERATION",
        "dir-${toolchain}-scope-",
        "primary=%s%s",
        "prefix=%s",
    ] {
        assert!(step.contains(required), "missing `{required}` in {step}");
    }
}

fn assert_qualification_gate(job: &str) {
    assert!(
        job.contains("if: inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{job}"
    );
}

fn assert_no_implicit_cache(job: &str) {
    let action = step_body(job, "Setup MBX");
    assert!(action.contains("backend: local"), "{action}");
    assert!(!job.contains("ACTIONS_CACHE_MODE:"), "{job}");
    for input in [
        "github-cache-mode:",
        "save-on-workflow-dispatch:",
        "save-on-pull-request:",
        "save-on-protected-branch:",
        "cache-key-suffix:",
        "isolate-objects-cache:",
        "ACTIONS_CACHE_MODE:",
    ] {
        assert!(!action.contains(input), "{action}");
    }
}

fn assert_mbx_action_pin(job: &str) {
    let action = step_body(job, "Setup MBX");
    let Some(uses) = action
        .lines()
        .find_map(|line| line.trim().strip_prefix("uses: "))
    else {
        assert!(
            action.contains("uses: jdx/mr-boxington-action@"),
            "{action}"
        );
        return;
    };
    let Some(sha) = uses.strip_prefix("jdx/mr-boxington-action@") else {
        assert!(uses.starts_with("jdx/mr-boxington-action@"), "{uses}");
        return;
    };
    assert_eq!(uses, MBX_ACTION_PIN);
    assert_eq!(action_input_names(action), vec!["backend", "version"]);
    assert_eq!(sha.len(), 40, "{uses}");
    assert!(
        sha.bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "{uses}"
    );
}

fn mbx_action_version(job: &str) -> Option<&str> {
    step_body(job, "Setup MBX")
        .lines()
        .find_map(|line| line.trim().strip_prefix("version: "))
}

fn action_input_names(step: &str) -> Vec<&str> {
    let Some(with_index) = step.lines().position(|line| line.trim() == "with:") else {
        return Vec::new();
    };
    step.lines()
        .skip(with_index + 1)
        .take_while(|line| line.starts_with("          "))
        .filter_map(|line| line.trim().split_once(':').map(|(name, _)| name))
        .collect()
}

fn step_body<'a>(job: &'a str, name: &str) -> &'a str {
    let needle = format!("name: {name}");
    let Some(start) = job.find(&needle) else {
        assert!(job.contains(&needle), "missing {needle} in {job}");
        return job;
    };
    let remaining = &job[start..];
    let end = remaining[needle.len()..]
        .find("\n      - name:")
        .map_or(remaining.len(), |offset| needle.len() + offset);
    &remaining[..end]
}

fn step_value<'a>(job: &'a str, step: &str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: ");
    step_body(job, step)
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
}

fn assert_same_step_value(writer: &str, reader: &str, step: &str, key: &str) {
    let writer_value = step_value(writer, step, key);
    let reader_value = step_value(reader, step, key);
    assert!(writer_value.is_some(), "{key} missing from writer");
    assert_eq!(writer_value, reader_value, "{step} {key} differs");
}

fn assert_ordered(job: &str, names: &[&str]) {
    let mut previous = 0;
    for name in names {
        let position = job[previous..].find(name);
        assert!(
            position.is_some(),
            "missing or out of order `{name}` in {job}"
        );
        previous += position.unwrap_or_default() + name.len();
    }
}
