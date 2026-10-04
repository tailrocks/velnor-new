//! Shared assertion helpers for the single-bundle MBX qualification test.

pub(super) const QUALIFICATION_SCOPE: &str = "qualification-mbx-v1/single-bundle-roundtrip";
pub(super) const MBX_VERSION: &str = "1.22.0";
pub(super) const MBX_GENERATION: &str = "velnor-mbx-1.22.0";
const MBX_ACTION_PIN: &str = "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6";
pub(super) const CACHE_RESTORE_PIN: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
pub(super) const CACHE_SAVE_PIN: &str =
    "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
pub(super) const BUNDLE_PATH: &str = "${{ runner.temp }}/mbx-single-bundle";

pub(super) fn assert_external_cache_actions(job: &str, writer: bool) {
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

pub(super) fn assert_compression_profile(job: &str) {
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

pub(super) fn assert_key_contract(step: &str) {
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

pub(super) fn assert_qualification_gate(job: &str) {
    assert!(
        job.contains("if: inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{job}"
    );
}

pub(super) fn assert_mbx_bootstrap(job: &str) {
    let step = step_body(job, "Install pinned Rust and MBX toolchains");
    assert_eq!(
        job.matches("name: Install pinned Rust and MBX toolchains")
            .count(),
        1,
        "{job}"
    );
    assert!(
        step.contains(
            "mise --no-config --no-env --no-hooks install rust@1.98.1 mr-boxington@1.22.0"
        ),
        "{step}"
    );
}

pub(super) fn assert_mbx_preflight(job: &str) {
    assert_eq!(
        job.matches("name: Verify MBX and Rust toolchains").count(),
        1,
        "exactly one central preflight per MBX job: {job}"
    );
    let step = step_body(job, "Verify MBX and Rust toolchains");
    let command = normalized_shell_run(step);
    assert!(
        !command.is_empty(),
        "preflight shell step has a run command: {step}"
    );
    assert!(
        command.contains("mise --no-config --no-env --no-hooks where 'mr-boxington@1.22.0'"),
        "{step}"
    );
    assert!(
        command.contains("mise --no-config --no-env --no-hooks where 'rust@1.98.1'"),
        "{step}"
    );
    assert!(step.contains("GITHUB_PATH"), "{step}");
    assert!(
        step.contains("rust_root") && step.contains("mbx_root"),
        "{step}"
    );
    assert!(
        command.contains("\"$rustc_bin\" '+1.98.1' -vV")
            && command.contains("grep -Fqx 'release: 1.98.1'"),
        "preflight verifies the selected Rustup shim and exact release: {step}"
    );
    let rustup_home = step_value(job, "Verify MBX and Rust toolchains", "RUSTUP_HOME");
    let mise_rustup_home = step_value(job, "Verify MBX and Rust toolchains", "MISE_RUSTUP_HOME");
    assert!(
        rustup_home.is_some(),
        "preflight must set RUSTUP_HOME: {step}"
    );
    assert_eq!(rustup_home, mise_rustup_home);
    let cargo_home = step_value(job, "Verify MBX and Rust toolchains", "CARGO_HOME");
    let mise_cargo_home = step_value(job, "Verify MBX and Rust toolchains", "MISE_CARGO_HOME");
    assert!(
        cargo_home.is_some(),
        "preflight must set CARGO_HOME: {step}"
    );
    assert_eq!(cargo_home, mise_cargo_home);
    assert!(!job.contains("mbx-rust-sysroot"), "{job}");
    assert!(!job.contains("sysroot/bin"), "{job}");
    assert!(!job.contains("printf '%s/bin\\n'"), "{job}");
}

fn normalized_shell_run(step: &str) -> String {
    let run = step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .unwrap_or_default();
    run.replace("\\\\''", "").replace("\\\"", "\"")
}

pub(super) fn assert_no_implicit_cache(job: &str) {
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

pub(super) fn assert_mbx_action_pin(job: &str) {
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

pub(super) fn mbx_action_version(job: &str) -> Option<&str> {
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

pub(super) fn step_body<'a>(job: &'a str, name: &str) -> &'a str {
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

pub(super) fn step_value<'a>(job: &'a str, step: &str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: ");
    step_body(job, step)
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
}

pub(super) fn assert_same_step_value(writer: &str, reader: &str, step: &str, key: &str) {
    let writer_value = step_value(writer, step, key);
    let reader_value = step_value(reader, step, key);
    assert!(writer_value.is_some(), "{key} missing from writer");
    assert_eq!(writer_value, reader_value, "{step} {key} differs");
}

pub(super) fn assert_shared_identity(writer: &str, reader: &str) {
    assert_shared_cache_identity(writer, reader);
    assert_shared_bundle_identity(writer, reader);
}

fn assert_shared_cache_identity(writer: &str, reader: &str) {
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
    assert_key_contract(step_body(writer, "Prepare MBX bundle key"));
    assert_key_contract(step_body(reader, "Prepare MBX bundle key"));
}

fn assert_shared_bundle_identity(writer: &str, reader: &str) {
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
}

pub(super) fn assert_ordered(job: &str, names: &[&str]) {
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
