//! Hosted-only MBX objects-cache qualification.
//!
//! Write and read are separate jobs in one dispatch. The writer is restricted
//! to protected main; the shared cache generation is bound to that run,
//! attempt, source SHA, action pin, and MBX release. The dependent reader
//! cannot publish a cache or pass on data from an earlier run.
//!
//! Both jobs set `MBX_GC_AUTO=1` to exercise the hosted policy emitted for
//! production MBX jobs, not the action's hosted default.

use super::features::{checkout_step, finish, gated, lane_base, run_step};
use super::{MbxQualificationPins, RunnerSpec};
use crate::cache_steps::MBX_ACTION_NAME;
use crate::yaml::Yaml;
use crate::{RenderError, steps::validate_uses};

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
const IMPORT_PROBE: &str = "mbx cache stats --json | jq -e '.objects > 0' >/dev/null";
const REUSE_PROBE: &str = "mbx stats --json | jq -e '.savings.cached_compilations > 0' >/dev/null";

/// Emit isolated writer and reader jobs for the pinned MBX runtime.
///
/// # Errors
/// Invalid action, Mise, MBX, or Rust pins fail closed.
pub(super) fn jobs(
    request: &MbxQualificationPins,
    hosted: &RunnerSpec,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    request.mise_setup.validate()?;
    validate_uses(&request.mbx_action_uses)?;
    let action_prefix = format!("{MBX_ACTION_NAME}@");
    if !request.mbx_action_uses.starts_with(&action_prefix) {
        return Err(RenderError::BadActionRef(format!(
            "not_mbx_action:{}",
            request.mbx_action_uses
        )));
    }
    validate_exact_version(&request.mbx_version, "mbx")?;
    validate_exact_version(&request.rust_version, "rust")?;

    Ok(vec![
        job(request, hosted, true),
        job(request, hosted, false),
    ])
}

fn job(request: &MbxQualificationPins, hosted: &RunnerSpec, writer: bool) -> (String, Yaml) {
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
    let mut fields = lane_base(title, hosted, 45);
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
            Yaml::str(format!(
                "mise install rust@{rust} && printf '%s/bin\\n' \"$(mise exec rust@{rust} -- rustc --print sysroot)\" >> \"$GITHUB_PATH\""
            )),
        ),
    ])
}

fn mbx_action_step(request: &MbxQualificationPins, writer: bool) -> Yaml {
    let generation = format!(
        "velnor-qualification-mbx-{}-action-{}-run-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-${{{{ github.sha }}}}",
        request.mbx_version,
        &request.mbx_action_uses[format!("{MBX_ACTION_NAME}@").len()..]
    );
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Restore MBX objects")),
        (
            "uses".to_owned(),
            Yaml::str(request.mbx_action_uses.clone()),
        ),
        ("id".to_owned(), Yaml::str("mbx_cache")),
        (
            "with".to_owned(),
            mapping(&[
                ("github-cache-mode", "objects"),
                ("version", &request.mbx_version),
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
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify MBX action identity and write policy"),
        ),
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
                "test \"$MBX_VERSION\" = '{}' && test \"$CACHE_SAVE_ELIGIBLE\" = '{}' && test \"$CACHE_SAVE_REASON\" = '{}'{}",
                request.mbx_version,
                save_eligible,
                save_reason,
                if writer {
                    ""
                } else {
                    r#" && test "$CACHE_HIT" = 'false'"#
                }
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

fn qualification_env(request: &MbxQualificationPins, writer: bool) -> Yaml {
    let home = "${{ github.workspace }}/.velnor-mbx-cache-qualification";
    mapping(&[
        ("MBX_GC_AUTO", "1"),
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
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!(
            "bad_{tool}_version:{value}"
        )))
    }
}
