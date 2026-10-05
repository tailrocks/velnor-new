//! MBX object action setup and exact Rust/MBX PATH preflight.

use std::collections::BTreeMap;

use super::{CompileDriver, MBX_ACTION_NAME};
use crate::RenderError;
use crate::steps::{action_step_with_env, shell_step, validate_uses};
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::{Job, Step};

/// Display name for the strict MBX and Rust PATH preflight.
pub const MBX_PREFLIGHT_NAME: &str = "Verify MBX and Rust toolchains";
/// Stable workspace-relative directory archived by actions/cache.
pub(crate) const MBX_TRANSPORT_CACHE_PATH: &str = ".velnor-mbx-cache/mbx-single-bundle";

/// MBX preflight and pinned local action setup for MBX-selected profiles;
/// Cargo yields none. The stock bundle helper adds external restore/import
/// before producers and export/save after them.
///
/// The preflight verifies the exact Mise-installed MBX executable and
/// the exact Rustup shim invocation, then adds those installed paths to
/// `GITHUB_PATH`. The local action receives the same exact MBX version;
/// the preflight fails before setup if the verified binary does not match.
/// # Errors
pub fn mbx_steps_for_driver(
    uses: &str,
    driver: CompileDriver,
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Option<[Step; 2]>, RenderError> {
    match driver {
        CompileDriver::Cargo => Ok(None),
        CompileDriver::Mbx => {
            if !is_exact_rust_toolchain(rust_toolchain) {
                return Err(RenderError::BadCommand(format!(
                    "bad_rust_toolchain:{rust_toolchain}"
                )));
            }
            let mut action_env = with_rustup_process_homes(env)?;
            let preflight =
                mbx_path_preflight_step(mbx_version, rust_toolchain, action_env.clone())?;
            action_env.insert(
                "MBX_CACHE_DIR".to_owned(),
                "${{ steps.mbx_cache_preflight.outputs.cache_dir }}".to_owned(),
            );
            action_env.insert(
                "MBX_CACHE_EXPORT_GROUP".to_owned(),
                "${{ steps.mbx_cache_preflight.outputs.export_group }}".to_owned(),
            );
            let setup = mbx_local_action_step(uses, mbx_version, rust_toolchain, action_env)?;
            Ok(Some([preflight, setup]))
        }
    }
}

/// Gate MBX action/tool presence against per-job driver selections.
///
/// Jobs without a declared driver are skipped (plan/final/lint carry
/// none); declared Cargo jobs must be MBX-free while MBX jobs carry
/// exactly one objects-mode step.
/// # Errors
pub fn check_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
) -> Result<(), RenderError> {
    for (id, driver) in drivers {
        let Some(job) = jobs.get(id.as_str()) else {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_gating_unknown_job:{id}"
            )));
        };
        check_job_mbx(id, job, *driver)?;
    }
    Ok(())
}

/// Enforce one job's MBX presence against its declared driver.
fn check_job_mbx(id: &str, job: &Job, driver: CompileDriver) -> Result<(), RenderError> {
    let actions = job.steps.iter().filter(|step| is_mbx_action(step)).count();
    let tools = job
        .steps
        .iter()
        .any(|step| uses_mbx_tool(step) && !is_mbx_action(step));
    match driver {
        CompileDriver::Cargo => {
            if actions > 0 {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_action_without_selection:{id}"
                )));
            }
            if tools {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_tool_without_selection:{id}"
                )));
            }
        }
        CompileDriver::Mbx => {
            if actions == 0 {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_missing_for_selection:{id}"
                )));
            }
            if actions > 1 {
                return Err(RenderError::InvalidWorkflow(format!("mbx_duplicated:{id}")));
            }
        }
    }
    Ok(())
}

/// True for `jdx/mr-boxington-action` steps.
pub(crate) fn is_mbx_action(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Action { uses, .. } if uses.starts_with(&format!("{MBX_ACTION_NAME}@")))
}

/// True when shell argv invokes the `mbx` program or tool spec.
fn uses_mbx_tool(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mbx" || arg.contains("mr-boxington")))
}

/// Env key the cache backend reads for its restore/save mode.
pub const MBX_CACHE_MODE_ENV: &str = "ACTIONS_CACHE_MODE";
/// Display name of the local MBX installation/setup step.
pub const MBX_LOCAL_SETUP_NAME: &str = "Prepare MBX local cache store";
/// Hosted Linux jobs disable asynchronous collection through action export.
pub const MBX_GC_AUTO_ENV: &str = "MBX_GC_AUTO";
/// MBX 1.21.1+ honors this value. Hosted Linux jobs keep collection off.
pub const MBX_GC_AUTO_VALUE: &str = "0";
/// Generated MBX jobs must not materialize a shared read-only `OUT_DIR`.
pub const MBX_SHARE_OUT_DIR_ENV: &str = "MBX_SHARE_OUT_DIR";
/// Disable shared `OUT_DIR` materialization on every generated MBX job.
pub const MBX_SHARE_OUT_DIR_VALUE: &str = "0";
/// Mode that skips the action post. `read` does not permit writes.
pub(crate) const MBX_ACTION_CACHE_MODE: &str = "read";

/// Exact MBX and Rust toolchain preflight before the action's PATH lookup.
///
/// `mise where` resolves both exact catalog installs without loading
/// consumer config. The exact MBX version and the Rustup `+toolchain`
/// probe must succeed before either directory is added to `GITHUB_PATH`.
/// Paths are quoted as single `printf` arguments so spaces remain intact.
/// # Errors
fn mbx_path_preflight_step(
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if !is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    let script = mbx_preflight_script(mbx_version, rust_toolchain);
    shell_step(
        MBX_PREFLIGHT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )
}

fn mbx_preflight_script(mbx_version: &str, rust_toolchain: &str) -> String {
    let mut lines = vec!["set -eu".to_owned(), "umask 077".to_owned()];
    lines.extend(preflight_authority_lines());
    lines.extend(preflight_reservation_lines());
    lines.extend(preflight_tool_lines(mbx_version, rust_toolchain));
    lines.extend(preflight_store_lines());
    lines.extend(preflight_transport_lines());
    lines.extend(preflight_output_lines());
    lines.join("; ")
}

fn preflight_authority_lines() -> Vec<String> {
    [
        "[ -n \"$RUNNER_TEMP\" ] && [ -n \"$GITHUB_WORKSPACE\" ] && [ -n \"$GITHUB_PATH\" ] && [ -n \"$GITHUB_OUTPUT\" ] && [ -n \"$GITHUB_ENV\" ] || { printf '%s\\n' 'GitHub runner paths are required' >&2; exit 1; }",
        "case \"${GITHUB_REPOSITORY_ID:-}\" in ''|*[!0-9]*) printf '%s\\n' 'invalid repository ID' >&2; exit 1 ;; esac",
        "case \"${GITHUB_RUN_ID:-}\" in ''|*[!0-9]*) printf '%s\\n' 'invalid run ID' >&2; exit 1 ;; esac",
        "case \"${GITHUB_RUN_ATTEMPT:-}\" in ''|*[!0-9]*) printf '%s\\n' 'invalid run attempt' >&2; exit 1 ;; esac",
        "case \"${GITHUB_JOB:-}\" in ''|*[!a-zA-Z0-9_-]*) printf '%s\\n' 'invalid job ID' >&2; exit 1 ;; esac",
        "case \"$RUNNER_OS\" in Linux|macOS) ;; *) printf '%s\\n' 'unsupported MBX runner OS' >&2; exit 1 ;; esac",
        "case \"$RUNNER_TEMP:$GITHUB_WORKSPACE\" in /*:/*) ;; *) printf '%s\\n' 'runner paths must be absolute' >&2; exit 1 ;; esac",
        "canonical_dir() { [ -d \"$1\" ] && [ ! -L \"$1\" ] || return 1; (cd \"$1\" && pwd -P) | grep -Fqx \"$1\"; }",
        "stat_owner() { if [ \"$RUNNER_OS\" = macOS ]; then stat -f '%u' \"$1\"; else stat -c '%u' -- \"$1\"; fi; }",
        "directory_identity() { if [ \"$RUNNER_OS\" = macOS ]; then stat -f '%d:%i' \"$1\"; else stat -c '%d:%i' -- \"$1\"; fi; }",
        "identity_matches() { directory_identity \"$1\" | grep -Fqx \"$2\"; }",
        "private_dir() { [ -d \"$1\" ] && [ ! -L \"$1\" ] && find -P \"$1\" -prune -perm 700 -print -quit | grep -Fqx \"$1\"; }",
        "secure_parent() { canonical_dir \"$1\" || return 1; { id -u || exit 1; stat_owner \"$1\" || exit 1; } | awk 'NR == 1 { runner = $0; next } NR == 2 { owner = $0; next } END { exit !(NR == 2 && runner == owner) }' || return 1; { find -P \"$1\" -prune -perm -022 -print -quit || exit 1; printf '__MBX_FIND_OK__\\n'; } | awk '$0 == \"__MBX_FIND_OK__\" { complete = 1; next } END { exit !(complete && NR == 1) }'; }",
        "secure_parent \"$RUNNER_TEMP\" && secure_parent \"$GITHUB_WORKSPACE\" || { printf '%s\\n' 'runner directories must be canonical, runner-owned, and not group/world writable' >&2; exit 1; }",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn preflight_reservation_lines() -> Vec<String> {
    [
        "parent=\"$RUNNER_TEMP/velnor\"; if [ ! -e \"$parent\" ] && [ ! -L \"$parent\" ]; then mkdir -m 700 \"$parent\" || { printf '%s\\n' 'cannot create private MBX parent' >&2; exit 1; }; fi; secure_parent \"$parent\" || { printf '%s\\n' 'MBX parent is not runner-owned and canonical' >&2; exit 1; }",
        "root=\"$parent/mbx-job-$GITHUB_REPOSITORY_ID-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$GITHUB_JOB\"; [ ! -e \"$root\" ] && [ ! -L \"$root\" ] || { printf '%s\\n' 'MBX job root already exists' >&2; exit 1; }; mkdir -m 700 \"$root\" || { printf '%s\\n' 'cannot reserve exclusive MBX job root' >&2; exit 1; }",
        "root_identity=; preflight_complete=false; probe=\"$root/preflight-probe\"; mbx_probe=\"$root/mbx-probe\"; rust_probe=\"$root/rust-probe\"; mbx_output=\"$root/mbx-version\"; rust_output=\"$root/rustc-version\"; primary=; cold=; import_cwd=; transport_parent=; transport_lease=; bundle=; primary_identity=; cold_identity=; import_identity=; transport_parent_identity=; transport_lease_identity=; bundle_identity=; transport_parent_created=false",
        "cleanup_empty() { path=\"$1\"; identity=\"$2\"; [ -n \"$path\" ] && [ -n \"$identity\" ] || return 0; if [ -e \"$path\" ] || [ -L \"$path\" ]; then [ ! -L \"$path\" ] && private_dir \"$path\" && identity_matches \"$path\" \"$identity\" && rmdir \"$path\" || return 1; fi; }",
        "cleanup_preflight() { status=$?; trap - 0 HUP INT TERM; if [ -n \"$root_identity\" ] && private_dir \"$root\" && identity_matches \"$root\" \"$root_identity\"; then rm -f \"$probe\" \"$mbx_probe\" \"$rust_probe\" \"$mbx_output\" \"$rust_output\" || status=1; if [ \"$preflight_complete\" != true ]; then cleanup_empty \"$bundle\" \"$bundle_identity\" || status=1; if [ \"$transport_parent_created\" = true ]; then cleanup_empty \"$transport_parent\" \"$transport_parent_identity\" || status=1; fi; cleanup_empty \"$transport_lease\" \"$transport_lease_identity\" || status=1; cleanup_empty \"$primary\" \"$primary_identity\" || status=1; cleanup_empty \"$cold\" \"$cold_identity\" || status=1; cleanup_empty \"$import_cwd\" \"$import_identity\" || status=1; cleanup_empty \"$root\" \"$root_identity\" || status=1; fi; else [ \"$status\" -ne 0 ] || status=1; fi; exit \"$status\"; }",
        "trap 'cleanup_preflight' 0; trap 'exit 129' HUP; trap 'exit 130' INT; trap 'exit 143' TERM",
        "(set -C; : > \"$probe\") || { printf '%s\\n' 'cannot reserve owned MBX probe' >&2; exit 1; }; directory_identity \"$root\" > \"$probe\"; IFS= read -r root_identity < \"$probe\" || exit 1; case \"$root_identity\" in ''|*[!0-9:]*) exit 1 ;; esac",
        "for probe_path in \"$mbx_probe\" \"$rust_probe\" \"$mbx_output\" \"$rust_output\"; do (set -C; : > \"$probe_path\") || { printf '%s\\n' 'cannot reserve owned MBX probe' >&2; exit 1; }; done",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn preflight_tool_lines(mbx_version: &str, rust_toolchain: &str) -> Vec<String> {
    let mut lines = vec![
        format!(
            "mise --no-config --no-env --no-hooks where 'mr-boxington@{mbx_version}' > \"$mbx_probe\""
        ),
        format!(
            "mise --no-config --no-env --no-hooks where 'rust@{rust_toolchain}' > \"$rust_probe\""
        ),
    ];
    lines.extend(
        [
            "IFS= read -r mbx_root < \"$mbx_probe\" && IFS= read -r rust_root < \"$rust_probe\" || { printf '%s\\n' 'Mise returned no install path' >&2; exit 1; }".to_owned(),
            "case \"$mbx_root:$rust_root\" in /*:/*) ;; *) printf '%s\\n' 'Mise returned a non-absolute install path' >&2; exit 1 ;; esac".to_owned(),
            "mbx_bin=\"$mbx_root/mbx\"; rustc_bin=\"$rust_root/rustc\"; [ -x \"$mbx_bin\" ] && [ -x \"$rustc_bin\" ] || { printf '%s\\n' 'expected MBX and Rustup executables' >&2; exit 1; }".to_owned(),
            format!("\"$mbx_bin\" --version > \"$mbx_output\" && \"$rustc_bin\" '+{rust_toolchain}' -vV > \"$rust_output\""),
            format!("IFS= read -r mbx_version_output < \"$mbx_output\" && [ \"$mbx_version_output\" = 'mbx {mbx_version}' ] || {{ printf '%s\\n' 'MBX version mismatch' >&2; exit 1; }}"),
            format!("grep -Fqx 'release: {rust_toolchain}' \"$rust_output\" || {{ printf '%s\\n' 'Rust toolchain mismatch' >&2; exit 1; }}"),
        ],
    );
    lines
}

fn preflight_store_lines() -> Vec<String> {
    [
        "job_identity=\"$GITHUB_REPOSITORY_ID:$GITHUB_RUN_ID:$GITHUB_RUN_ATTEMPT:$GITHUB_JOB\"; export_group=\"velnor-$GITHUB_REPOSITORY_ID-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$GITHUB_JOB\"",
        "primary=\"$root/store\"; cold=\"$root/store-cold\"; import_cwd=\"$root/import\"; mkdir -m 700 \"$primary\" \"$cold\" \"$import_cwd\" || { printf '%s\\n' 'cannot create private MBX store children' >&2; exit 1; }",
        "private_dir \"$root\" && private_dir \"$primary\" && private_dir \"$cold\" && private_dir \"$import_cwd\" || { printf '%s\\n' 'MBX private directory mode check failed' >&2; exit 1; }",
        "directory_identity \"$primary\" > \"$probe\"; IFS= read -r primary_identity < \"$probe\"; directory_identity \"$cold\" > \"$probe\"; IFS= read -r cold_identity < \"$probe\"; directory_identity \"$import_cwd\" > \"$probe\"; IFS= read -r import_identity < \"$probe\"",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn preflight_transport_lines() -> Vec<String> {
    [
        "workspace_root=\"$GITHUB_WORKSPACE\"; directory_identity \"$workspace_root\" > \"$probe\"; IFS= read -r workspace_identity < \"$probe\"",
        "[ \"$workspace_root\" = \"$GITHUB_WORKSPACE\" ] && secure_parent \"$workspace_root\" || { printf '%s\\n' 'workspace authority changed before lease acquisition' >&2; exit 1; }; transport_lease=\"$workspace_root/.velnor-mbx-cache.lock\"; [ ! -e \"$transport_lease\" ] && [ ! -L \"$transport_lease\" ] || { printf '%s\\n' 'MBX workspace transport is already leased' >&2; exit 1; }; mkdir -m 700 \"$transport_lease\" || { printf '%s\\n' 'cannot acquire exclusive MBX workspace lease' >&2; exit 1; }; private_dir \"$transport_lease\" || { printf '%s\\n' 'MBX workspace lease is not private' >&2; exit 1; }; directory_identity \"$transport_lease\" > \"$probe\"; IFS= read -r transport_lease_identity < \"$probe\"",
        "transport_parent=\"$workspace_root/.velnor-mbx-cache\"; if [ ! -e \"$transport_parent\" ] && [ ! -L \"$transport_parent\" ]; then mkdir -m 700 \"$transport_parent\" || { printf '%s\\n' 'cannot create MBX transport parent' >&2; exit 1; }; transport_parent_created=true; fi; secure_parent \"$transport_parent\" && private_dir \"$transport_parent\" || { printf '%s\\n' 'MBX transport parent is not private and runner-owned' >&2; exit 1; }; directory_identity \"$transport_parent\" > \"$probe\"; IFS= read -r transport_parent_identity < \"$probe\"",
        "bundle=\"$transport_parent/mbx-single-bundle\"; private_dir \"$transport_parent\" && identity_matches \"$transport_parent\" \"$transport_parent_identity\" && private_dir \"$transport_lease\" && identity_matches \"$transport_lease\" \"$transport_lease_identity\" || { printf '%s\\n' 'MBX transport authority changed' >&2; exit 1; }; [ ! -e \"$bundle\" ] && [ ! -L \"$bundle\" ] || { printf '%s\\n' 'stale MBX workspace bundle requires safe cleanup' >&2; exit 1; }; mkdir -m 700 \"$bundle\" || { printf '%s\\n' 'cannot reserve MBX workspace bundle path' >&2; exit 1; }; private_dir \"$bundle\" || { printf '%s\\n' 'MBX workspace bundle is not private' >&2; exit 1; }; directory_identity \"$bundle\" > \"$probe\"; IFS= read -r bundle_identity < \"$probe\"",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn preflight_output_lines() -> Vec<String> {
    [
        "printf 'MBX_CACHE_DIR=%s\\nMBX_CACHE_EXPORT_GROUP=%s\\n' \"$primary\" \"$export_group\" >> \"$GITHUB_ENV\"",
        "printf '%s\\n' \"$rust_root\" \"$mbx_root\" >> \"$GITHUB_PATH\"",
        "printf 'workspace_root=%s\\nworkspace_identity=%s\\ntransport_parent=%s\\ntransport_parent_identity=%s\\ntransport_lease=%s\\ntransport_lease_identity=%s\\njob_root=%s\\njob_identity=%s\\njob_root_identity=%s\\ncache_dir=%s\\ncache_dir_identity=%s\\ncold_cache_dir=%s\\ncold_cache_dir_identity=%s\\nbundle_path=%s\\nbundle_path_identity=%s\\nexport_group=%s\\nimport_cwd=%s\\nimport_cwd_identity=%s\\n' \"$workspace_root\" \"$workspace_identity\" \"$transport_parent\" \"$transport_parent_identity\" \"$transport_lease\" \"$transport_lease_identity\" \"$root\" \"$job_identity\" \"$root_identity\" \"$primary\" \"$primary_identity\" \"$cold\" \"$cold_identity\" \"$bundle\" \"$bundle_identity\" \"$export_group\" \"$import_cwd\" \"$import_identity\" >> \"$GITHUB_OUTPUT\"",
        "preflight_complete=true",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn mbx_local_action_step(
    uses: &str,
    mbx_version: &str,
    rust_toolchain: &str,
    mut env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
        return Err(RenderError::BadActionRef(format!("not_mbx_action:{uses}")));
    }
    if !is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    if !is_exact_rust_toolchain(rust_toolchain) {
        return Err(RenderError::BadCommand(format!(
            "bad_rust_toolchain:{rust_toolchain}"
        )));
    }
    let action_sha = uses
        .strip_prefix(&format!("{MBX_ACTION_NAME}@"))
        .ok_or_else(|| RenderError::BadActionRef(format!("not_mbx_action:{uses}")))?;
    let with = BTreeMap::from([
        ("backend".to_owned(), "local".to_owned()),
        ("github-cache-mode".to_owned(), "objects".to_owned()),
        ("version".to_owned(), mbx_version.to_owned()),
        ("toolchain".to_owned(), rust_toolchain.to_owned()),
        (
            "cache-generation".to_owned(),
            cache_generation(mbx_version, action_sha),
        ),
    ]);
    env.insert(
        MBX_CACHE_MODE_ENV.to_owned(),
        MBX_ACTION_CACHE_MODE.to_owned(),
    );
    env.insert(
        MBX_SHARE_OUT_DIR_ENV.to_owned(),
        MBX_SHARE_OUT_DIR_VALUE.to_owned(),
    );
    env.insert("VELNOR_MBX_VERSION".to_owned(), mbx_version.to_owned());
    action_step_with_env(MBX_LOCAL_SETUP_NAME, uses, with, env)
}

/// Generation binds the runner-independent `OUT_DIR` policy and action SHA.
fn cache_generation(mbx_version: &str, action_sha: &str) -> String {
    format!(
        "{}-share-out-dir-disabled-v1-action-{action_sha}",
        mbx_cache_generation(mbx_version)
    )
}

/// Expose native Rustup paths as well as Mise's selected tool homes.
fn with_rustup_process_homes(
    mut env: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, RenderError> {
    crate::toolchain_env::check_toolchain_homes(&env)?;
    let rustup_home = env
        .get("MISE_RUSTUP_HOME")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| RenderError::BadCommand("missing_mise_rustup_home".to_owned()))?;
    let cargo_home = env
        .get("MISE_CARGO_HOME")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| RenderError::BadCommand("missing_mise_cargo_home".to_owned()))?;
    env.insert("RUSTUP_HOME".to_owned(), rustup_home);
    env.insert("CARGO_HOME".to_owned(), cargo_home);
    Ok(env)
}

/// Exact MBX versions: three nonempty numeric dot parts, nothing else.
fn is_exact_mbx_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Exact Rust toolchain selectors in the compiled catalog.
fn is_exact_rust_toolchain(toolchain: &str) -> bool {
    let parts: Vec<&str> = toolchain.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}
