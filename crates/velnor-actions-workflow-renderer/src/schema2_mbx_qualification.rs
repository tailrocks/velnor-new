//! Hosted-only MBX objects-cache qualification experiment.
//!
//! Write and read are separate jobs in one dispatch. The writer is restricted
//! to protected main; the shared cache generation is bound to that run,
//! attempt, source SHA, action pin, and MBX release. The dependent reader
//! cannot publish a cache or pass on data from an earlier run.
//! The action ref is an explicit candidate input separate from the production
//! pin; emitting this probe does not assert that candidate qualification passed.
//!
//! Both jobs set `MBX_GC_AUTO=0` and `MBX_SHARE_OUT_DIR=0`. The candidate
//! action experiment remains separate from the production action pin and
//! typed hosted/Scale Set routes; passing it does not qualify those routes.

use super::features::{checkout_step, finish, gated, lane_base, run_step};
use super::{MbxQualificationPins, RunnerSpec};
use crate::cache_steps::{ACTION_MBX_BIN, MBX_ACTION_NAME, RESOLVE_ACTION_MBX_BIN};
use crate::yaml::Yaml;
use crate::{RenderError, steps::validate_uses};
use velnor_actions_contract::workflow::MBX_WORKSPACE_CLEAN_CONDITION;

const MAIN_REF: &str = "github.ref == 'refs/heads/main' && github.ref_protected == true";
const SMOKE_CRATE_SETUP: &str = r#"set -eu
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
"#;
const DISK_SAMPLE: &str = "df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"";

/// Emit roundtrip, corruption, and parallel jobs for the pinned MBX runtime.
///
/// # Errors
/// Invalid action, Mise, MBX, Rust, or hosted-runner inputs fail closed.
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

    Ok(vec![
        job(request, hosted, true)?,
        job(request, hosted, false)?,
    ])
}

fn job(request: &MbxQualificationPins, hosted: &RunnerSpec, writer: bool) -> (String, Yaml) {
    let (id, title, mode) = if writer {
        (
            "mbx-cache-write-hosted",
            "MBX objects cache / protected-main writer",
            Vec::new(),
        )
    } else {
        (
            "mbx-cache-read-hosted",
            "MBX objects cache / read-only reuse",
            vec!["mbx-cache-write-hosted".to_owned()],
        )
    };
    let mut fields = lane_base(title, hosted, 45);
    if !writer {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    fields.push(("permissions".to_owned(), permission_yaml(writer)));
    fields.push(("env".to_owned(), qualification_env(request)));
    Ok(gated(
        finish(id, fields, rendered_steps),
        QUALIFICATION_GATE,
    ))
}

fn qualification_steps(
    request: &MbxQualificationPins,
    writer: bool,
) -> Result<Vec<Step>, RenderError> {
    let mut steps = vec![
        checkout_step()?,
        mise_setup_step(request)?,
        mise_install_step(request)?,
        mbx_action_step(request)?,
        verify_action_step(request)?,
    ];
    if !writer {
        let probe = import_probe(request);
        steps.push(run_step("Require imported MBX objects", &probe));
    }
    steps.push(build_step(request));
    if !writer {
        let probe = reuse_probe(request);
        steps.push(run_step("Require reused compilation", &probe));
    }
    steps.push(clean_workspace_step(request));
    if writer {
        steps.push(run_step("Sample runner disk after cleanup", DISK_SAMPLE));
    }
    steps.push(clean_workspace_step(request));
    if writer {
        steps.push(run_step("Sample runner disk after cleanup", DISK_SAMPLE));
    }
    Ok(steps)
}

fn checkout_step() -> Result<Step, RenderError> {
    steps::checkout_step(super::features::CHECKOUT_USES)
}

fn mise_setup_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    steps::action_step(
        "Set up Mise",
        &request.mise_setup.uses,
        BTreeMap::from([
            ("version".to_owned(), request.mise_setup.version.clone()),
            ("sha256".to_owned(), request.mise_setup.sha256.clone()),
            ("install".to_owned(), "false".to_owned()),
            ("env".to_owned(), "false".to_owned()),
            ("cache".to_owned(), "false".to_owned()),
            ("cache_save".to_owned(), "false".to_owned()),
        ]),
    )
}

fn mise_install_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    let rust = &request.rust_version;
    shell_step(
        "Install pinned Rust toolchain",
        &format!(
            "mise install rust@{rust} && mise exec rust@{rust} -- rustc --print sysroot > \"$RUNNER_TEMP/mbx-rust-sysroot\" && IFS= read -r sysroot < \"$RUNNER_TEMP/mbx-rust-sysroot\" && test -n \"$sysroot\" && printf '%s/bin\\n' \"$sysroot\" >> \"$GITHUB_PATH\""
        ),
    )
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
    // A dispatch writer saves under a run/attempt suffix. The dependent
    // read-only job restores via the run-bound prefix, so exact equality is
    // false even when the import and compilation-reuse probes pass.
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
                "set -eu; {RESOLVE_ACTION_MBX_BIN}; test \"$MBX_VERSION\" = '{}' && test \"$CACHE_SAVE_ELIGIBLE\" = '{}' && test \"$CACHE_SAVE_REASON\" = '{}'{} && actual_version=$(mise --no-config --no-env --no-hooks exec rust@{} -- \"{ACTION_MBX_BIN}\" --version) && test \"$actual_version\" = 'mbx {}'",
                request.mbx_version,
                save_eligible,
                save_reason,
                exact_cache_hit,
                request.rust_version,
                request.mbx_version
            )),
        ),
    ])
}

fn import_probe(request: &MbxQualificationPins) -> String {
    format!(
        "set -e -o pipefail; df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"; {RESOLVE_ACTION_MBX_BIN}; mise --no-config --no-env --no-hooks exec rust@{} -- \"{ACTION_MBX_BIN}\" cache stats --json | tee \"$RUNNER_TEMP/mbx-object-stats.json\"; jq -e '.objects > 0' \"$RUNNER_TEMP/mbx-object-stats.json\"",
        request.rust_version
    )
}

fn reuse_probe(request: &MbxQualificationPins) -> String {
    format!(
        "set -e -o pipefail; df -B1 -P \"$RUNNER_TEMP\"; df -i -P \"$RUNNER_TEMP\"; {RESOLVE_ACTION_MBX_BIN}; mise --no-config --no-env --no-hooks exec rust@{} -- \"{ACTION_MBX_BIN}\" stats --json | tee \"$RUNNER_TEMP/mbx-reuse-stats.json\"; jq -e '.savings.cached_compilations > 0' \"$RUNNER_TEMP/mbx-reuse-stats.json\"",
        request.rust_version
    )
}

fn build_step(request: &MbxQualificationPins) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Compile MBX cache probe")),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "{SMOKE_CRATE_SETUP}{RESOLVE_ACTION_MBX_BIN}\nmise --no-config --no-env --no-hooks exec rust@{} -- \"{ACTION_MBX_BIN}\" +{} build --manifest-path \"$root/Cargo.toml\"",
                request.rust_version, request.rust_version
            )),
        ),
    ])
}

fn clean_workspace_step(request: &MbxQualificationPins) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Clean MBX workspace outputs")),
        ("if".to_owned(), Yaml::str(MBX_WORKSPACE_CLEAN_CONDITION)),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "set -eu; {RESOLVE_ACTION_MBX_BIN}; mise --no-config --no-env --no-hooks exec rust@{} -- \"{ACTION_MBX_BIN}\" clean",
                request.rust_version
            )),
        ),
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
        ("MBX_CACHE_DIR", &format!("{home}/mbx-store")),
        ("MBX_CACHE_EXPORT_GROUP", &export_group),
        ("MBX_GC_AUTO", "1"),
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

fn step_context() -> RenderContext {
    RenderContext {
        generator_version: "0.0.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.0.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor".to_owned(),
        checkout_uses: super::features::CHECKOUT_USES.to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn validate_exact_version(version: &str, kind: &str) -> Result<(), RenderError> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!(
            "bad_{kind}_version:{version}"
        )))
    }
}
