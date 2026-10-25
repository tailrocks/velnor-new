use super::features::{checkout_step, finish, gated, lane_base, run_step};
use super::{MbxQualificationPins, RunnerSpec};
use crate::cache_steps::MBX_ACTION_NAME;
use crate::yaml::Yaml;
use crate::{RenderError, steps::validate_uses};
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::workflow::MBX_WORKSPACE_CLEAN_CONDITION;

const MAIN_REF: &str = "github.ref == 'refs/heads/main' && github.ref_protected == true";
const SMOKE_CRATE: &str = r#"set -eu
root="$GITHUB_WORKSPACE/.velnor-mbx-cache-qualification"
mkdir -p "$root/src"
cat > "$root/Cargo.toml" <<'EOF'
[package]
name = "mbx-cache-qualification"
version = "0.1.0"
edition = "2024"

[workspace]
members = ["."]
resolver = "3"

[lib]
path = "src/lib.rs"
EOF
cat > "$root/src/lib.rs" <<'EOF'
pub fn cache_probe() -> u64 { 42 }
EOF
mbx build --manifest-path "$root/Cargo.toml"
"#;
const DISK_SAMPLE: &str = "df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"";
const IMPORT_PROBE: &str = "set -e -o pipefail; df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"; mbx cache stats --json | tee \"$RUNNER_TEMP/mbx-object-stats.json\"; jq -e '.objects > 0' \"$RUNNER_TEMP/mbx-object-stats.json\"";
const REUSE_PROBE: &str = "set -e -o pipefail; df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"; mbx stats --json | tee \"$RUNNER_TEMP/mbx-reuse-stats.json\"; jq -e '.savings.cached_compilations > 0' \"$RUNNER_TEMP/mbx-reuse-stats.json\"";

pub(super) fn jobs(
    request: &MbxQualificationPins,
    hosted: &RunnerSpec,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    request.mise_setup.validate()?;
    validate_uses(&request.candidate_action_uses)?;
    let action_prefix = format!("{MBX_ACTION_NAME}@");
    if !request.candidate_action_uses.starts_with(&action_prefix) {
        return Err(RenderError::BadActionRef(format!(
            "not_mbx_action:{}",
            request.candidate_action_uses
        )));
    }
    validate_exact_version(&request.mbx_version, "mbx")?;
    validate_exact_version(&request.rust_version, "rust")?;
    let verify_rust_step = crate::verifier_composite::verifier_shell_step(
        velnor_actions_contract::StepRole::VerifyRustManifest,
        crate::VERIFY_RUST_TOOLCHAIN_STEP,
        request.verify_rust_argv.clone(),
        std::collections::BTreeMap::default(),
    )?;
    let verify_rust = crate::document_steps::verifier_shell_step_to_yaml(&verify_rust_step)?;

    Ok(vec![
        job(request, hosted, true, &verify_rust),
        job(request, hosted, false, &verify_rust),
    ])
}

fn job(
    request: &MbxQualificationPins,
    hosted: &RunnerSpec,
    writer: bool,
    verify_rust: &Yaml,
) -> (String, Yaml) {
    let (id, title, mode) = if writer {
        (
            "mbx-cache-write-hosted",
            "MBX objects cache / protected-main writer",
            "mbx-cache-roundtrip",
        )
    } else {
        (
            "mbx-cache-read-hosted",
            "MBX objects cache / read-only reuse",
            "mbx-cache-roundtrip",
        )
    };
    let mut fields = lane_base(title, hosted, JobTimeout::DEFAULT);
    if !writer {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(vec![Yaml::str("mbx-cache-write-hosted")]),
        ));
    }
    fields.push((
        "permissions".to_owned(),
        mapping(&[
            ("contents", "read"),
            ("actions", if writer { "write" } else { "read" }),
        ]),
    ));
    fields.push(("env".to_owned(), qualification_env(request, writer)));

    let mut steps = vec![
        checkout_step(),
        mise_setup_step(request),
        mise_install_step(request),
        verify_rust.clone(),
        expose_rust_step(request),
        mbx_action_step(request, writer),
        verify_action_step(request, writer),
    ];
    if !writer {
        steps.push(run_step("Require imported MBX objects", IMPORT_PROBE));
    }
    steps.push(build_step());
    if !writer {
        steps.push(run_step("Require reused compilation", REUSE_PROBE));
    }
    steps.push(clean_workspace_step(request));
    if writer {
        steps.push(run_step("Sample runner disk after cleanup", DISK_SAMPLE));
    }

    let when = format!("inputs.mode == '{mode}' && {MAIN_REF}");
    gated(finish(id, fields, steps), &when)
}

fn mise_setup_step(request: &MbxQualificationPins) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Set up Mise")),
        (
            "uses".to_owned(),
            Yaml::str(request.mise_setup.uses.clone()),
        ),
        (
            "with".to_owned(),
            mapping(&[
                ("version", &request.mise_setup.version),
                ("sha256", &request.mise_setup.sha256),
                ("install", "false"),
                ("env", "false"),
                ("cache", "false"),
                ("cache_save", "false"),
            ]),
        ),
    ])
}

fn mise_install_step(request: &MbxQualificationPins) -> Yaml {
    let rust = &request.rust_version;
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Install pinned Rust toolchain"),
        ),
        (
            "run".to_owned(),
            Yaml::str(format!("mise install rust@{rust}")),
        ),
    ])
}

fn expose_rust_step(request: &MbxQualificationPins) -> Yaml {
    let rust = &request.rust_version;
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Put the verified Rust toolchain on PATH"),
        ),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "printf '%s/bin\\n' \"$(mise exec rust@{rust} -- rustc --print sysroot)\" >> \"$GITHUB_PATH\""
            )),
        ),
    ])
}

fn mbx_action_step(request: &MbxQualificationPins, writer: bool) -> Yaml {
    let generation = format!(
        "velnor-qualification-mbx-{}-share-out-dir-disabled-v1-gc-auto-off-final-clean-v1-action-{}-run-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-${{{{ github.sha }}}}",
        request.mbx_version,
        &request.candidate_action_uses[format!("{MBX_ACTION_NAME}@").len()..]
    );
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Restore MBX objects")),
        (
            "uses".to_owned(),
            Yaml::str(request.candidate_action_uses.clone()),
        ),
        ("id".to_owned(), Yaml::str("mbx_cache")),
        (
            "with".to_owned(),
            mapping(&[
                ("github-cache-mode", "objects"),
                ("version", &request.mbx_version),
                ("toolchain", &request.rust_version),
                ("isolate-objects-cache", "true"),
                ("cache-generation", &generation),
                (
                    "save-on-workflow-dispatch",
                    if writer { "true" } else { "false" },
                ),
            ]),
        ),
    ])
}

fn verify_action_step(request: &MbxQualificationPins, writer: bool) -> Yaml {
    let save_eligible = if writer { "true" } else { "false" };
    let save_reason = if writer {
        "workflow_dispatch"
    } else {
        "workflow_dispatch; save-on-workflow-dispatch is off"
    };
    let exact_cache_hit = if writer {
        ""
    } else {
        r#" && test "$CACHE_HIT" = 'false'"#
    };
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify MBX action identity and write policy"),
        ),
        ("id".to_owned(), Yaml::str("mbx-ready")),
        (
            "env".to_owned(),
            mapping(&[
                ("MBX_VERSION", "${{ steps.mbx_cache.outputs.mbx-version }}"),
                (
                    "CACHE_SAVE_ELIGIBLE",
                    "${{ steps.mbx_cache.outputs.cache-save-eligible }}",
                ),
                (
                    "CACHE_SAVE_REASON",
                    "${{ steps.mbx_cache.outputs.cache-save-reason }}",
                ),
                ("CACHE_HIT", "${{ steps.mbx_cache.outputs.cache-hit }}"),
            ]),
        ),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "test \"$MBX_VERSION\" = \"{}\" && test \"$CACHE_SAVE_ELIGIBLE\" = '{}' && test \"$CACHE_SAVE_REASON\" = '{}'{}",
                request.mbx_version, save_eligible, save_reason, exact_cache_hit
            )),
        ),
    ])
}

fn build_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Compile MBX cache probe")),
        ("run".to_owned(), Yaml::str(SMOKE_CRATE)),
    ])
}

fn clean_workspace_step(request: &MbxQualificationPins) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Clean MBX workspace outputs")),
        ("if".to_owned(), Yaml::str(MBX_WORKSPACE_CLEAN_CONDITION)),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "mise --no-config --no-env --no-hooks exec rust@{} -- mbx clean",
                request.rust_version
            )),
        ),
    ])
}

fn qualification_env(request: &MbxQualificationPins, writer: bool) -> Yaml {
    let home = "${{ github.workspace }}/.velnor-mbx-cache-qualification";
    mapping(&[
        ("MBX_GC_AUTO", "0"),
        ("MBX_SHARE_OUT_DIR", "0"),
        ("ACTIONS_CACHE_MODE", if writer { "write" } else { "read" }),
        ("CARGO_HOME", &format!("{home}/cargo")),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_CARGO_HOME", &format!("{home}/cargo")),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_RUSTUP_HOME", &format!("{home}/rustup")),
        ("RUSTUP_HOME", &format!("{home}/rustup")),
        ("RUSTUP_TOOLCHAIN", &request.rust_version),
    ])
}

fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn validate_exact_version(value: &str, tool: &str) -> Result<(), RenderError> {
    if crate::commands::is_exact_or_project_version(value) {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!(
            "bad_{tool}_version:{value}"
        )))
    }
}
