use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, StepRole};

use crate::{RenderError, cache_p08, marker, steps, tree::RenderedFile};

pub(crate) const MBX_RUST_CHECKS_SCRIPT_PATH: &str = ".github/scripts/velnor-mbx-rust-checks.sh";

const MBX_RUST_CHECKS_SCRIPT_BODY: &str = r#"set -eu

if [ "$#" -eq 0 ]; then
    printf '%s\n' 'MBX check mode is required' >&2
    exit 2
fi
mode=$1
shift

case "$mode" in
    preflight)
        [ "$#" -eq 1 ] || { printf '%s\n' 'preflight requires one Rust toolchain' >&2; exit 2; }
        rust_toolchain=$1
        case "$RUNNER_TEMP" in /*) ;; *) printf '%s\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac
        [ -d "$RUNNER_TEMP" ] || { printf '%s\n' 'RUNNER_TEMP must exist' >&2; exit 1; }
        check_no_symlink() { path=$1; while [ "$path" != / ]; do [ ! -L "$path" ] || return 1; path=${path%/*}; [ -n "$path" ] || path=/; done; return 0; }
        check_no_symlink "$RUNNER_TEMP" || { printf '%s\n' 'RUNNER_TEMP path contains a symlink' >&2; exit 1; }
        mbx_cache_dir="$RUNNER_TEMP/velnor/mbx"
        [ "${MBX_CACHE_DIR:-}" = "$mbx_cache_dir" ] || { printf '%s\n' 'MBX_CACHE_DIR must use the stable runner-temp path' >&2; exit 1; }
        [ ! -L "$RUNNER_TEMP/velnor" ] || { printf '%s\n' 'Velnor temp root must not be a symlink' >&2; exit 1; }
        if [ ! -e "$RUNNER_TEMP/velnor" ]; then (umask 077 && mkdir -m 700 "$RUNNER_TEMP/velnor"); fi
        [ -d "$RUNNER_TEMP/velnor" ] && check_no_symlink "$RUNNER_TEMP/velnor" || { printf '%s\n' 'Velnor temp root must be a real directory' >&2; exit 1; }
        [ ! -e "$mbx_cache_dir" ] && [ ! -L "$mbx_cache_dir" ] || { printf '%s\n' 'MBX store root already exists in this private runner namespace' >&2; exit 1; }
        (umask 077 && mkdir -m 700 "$mbx_cache_dir") || { printf '%s\n' 'could not reserve the MBX store root' >&2; exit 1; }
        [ -n "${GITHUB_ENV:-}" ] && [ -f "$GITHUB_ENV" ] && [ ! -L "$GITHUB_ENV" ] || { printf '%s\n' 'GITHUB_ENV must be a regular runner file' >&2; exit 1; }
        check_no_symlink "$GITHUB_ENV" || { printf '%s\n' 'GITHUB_ENV path contains a symlink' >&2; exit 1; }
        printf 'MBX_CACHE_DIR=%s\n' "$mbx_cache_dir" >> "$GITHUB_ENV"
        [ -n "$GITHUB_PATH" ] || { printf '%s\n' 'GITHUB_PATH is required' >&2; exit 1; }
        case "$GITHUB_RUN_ID" in ''|*[!0-9]*) printf '%s\n' 'run id must be numeric' >&2; exit 1 ;; esac
        case "$GITHUB_RUN_ATTEMPT" in ''|*[!0-9]*) printf '%s\n' 'run attempt must be numeric' >&2; exit 1 ;; esac
        probe_dir="$RUNNER_TEMP/velnor-mbx-preflight-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT"
        [ ! -e "$probe_dir" ] && [ ! -L "$probe_dir" ] || { printf '%s\n' 'preflight scratch already exists' >&2; exit 1; }
        (umask 077 && mkdir -m 700 "$probe_dir") || { printf '%s\n' 'could not reserve preflight scratch' >&2; exit 1; }
        cleanup() { rm -f "$probe_dir/rust-root" "$probe_dir/rustc-output"; rmdir "$probe_dir"; }
        trap cleanup EXIT
        trap 'exit 1' HUP INT TERM
        mise --no-config --no-env --no-hooks where "rust@$rust_toolchain" > "$probe_dir/rust-root"
        exec 3< "$probe_dir/rust-root"
        IFS= read -r rust_root <&3 || { printf '%s\n' 'Mise returned no Rust install path' >&2; exit 1; }
        extra=; if IFS= read -r extra <&3 || [ -n "$extra" ]; then printf '%s\n' 'Mise returned multiple Rust install paths' >&2; exit 1; fi
        exec 3<&-
        [ -n "$rust_root" ] || { printf '%s\n' 'Mise returned an empty Rust install path' >&2; exit 1; }
        case "$rust_root" in /*) ;; *) printf '%s\n' 'Rust install path must be absolute' >&2; exit 1 ;; esac
        rustc_bin="$rust_root/rustc"
        [ -x "$rustc_bin" ] || { printf 'expected Rustup shim at %s\n' "$rustc_bin" >&2; exit 1; }
        "$rustc_bin" "+$rust_toolchain" -vV > "$probe_dir/rustc-output"
        grep -Fqx "release: $rust_toolchain" "$probe_dir/rustc-output" || { printf 'Rustup did not report exact toolchain %s\n' "$rust_toolchain" >&2; exit 1; }
        printf '%s\n' "$rust_root" >> "$GITHUB_PATH"
        ;;
    verify)
        [ "$#" -eq 2 ] || { printf '%s\n' 'verify requires an MBX version and Rust toolchain' >&2; exit 2; }
        mbx_version=$1
        rust_toolchain=$2
        case "$RUNNER_TEMP" in /*) ;; *) printf '%s\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac
        check_no_symlink() { path=$1; while [ "$path" != / ]; do [ ! -L "$path" ] || return 1; path=${path%/*}; [ -n "$path" ] || path=/; done; return 0; }
        [ -d "$RUNNER_TEMP" ] && check_no_symlink "$RUNNER_TEMP" || { printf '%s\n' 'RUNNER_TEMP must be a real directory' >&2; exit 1; }
        [ "${MBX_CACHE_DIR:-}" = "$RUNNER_TEMP/velnor/mbx" ] || { printf '%s\n' 'MBX_CACHE_DIR changed from its stable runner-temp path' >&2; exit 1; }
        [ -d "$RUNNER_TEMP/velnor" ] && [ -d "$MBX_CACHE_DIR" ] && [ -d "$MBX_CACHE_DIR/actions" ] || { printf '%s\n' 'MBX store directories are missing' >&2; exit 1; }
        check_no_symlink "$RUNNER_TEMP/velnor" && check_no_symlink "$MBX_CACHE_DIR/actions" || { printf '%s\n' 'MBX store path contains a symlink' >&2; exit 1; }
        case "$GITHUB_RUN_ID" in ''|*[!0-9]*) printf '%s\n' 'run id must be numeric' >&2; exit 1 ;; esac
        case "$GITHUB_RUN_ATTEMPT" in ''|*[!0-9]*) printf '%s\n' 'run attempt must be numeric' >&2; exit 1 ;; esac
        probe_dir="$RUNNER_TEMP/velnor-mbx-verify-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT"
        [ ! -e "$probe_dir" ] && [ ! -L "$probe_dir" ] || { printf '%s\n' 'verification scratch already exists' >&2; exit 1; }
        (umask 077 && mkdir -m 700 "$probe_dir") || { printf '%s\n' 'could not reserve verification scratch' >&2; exit 1; }
        cleanup() { rm -f "$probe_dir/version" "$probe_dir/cache-dir"; rmdir "$probe_dir"; }
        trap cleanup EXIT
        trap 'exit 1' HUP INT TERM
        mise --no-config --no-env --no-hooks exec "rust@$rust_toolchain" -- mbx --version > "$probe_dir/version"
        exec 3< "$probe_dir/version"
        IFS= read -r version_line <&3 || { printf '%s\n' 'MBX version output is empty or unterminated' >&2; exit 1; }
        extra=; if IFS= read -r extra <&3 || [ -n "$extra" ]; then printf '%s\n' 'MBX version output has extra data' >&2; exit 1; fi
        exec 3<&-
        [ "$version_line" = "mbx $mbx_version" ] || { printf '%s\n' 'MBX on PATH does not match the pinned action version' >&2; exit 1; }
        mise --no-config --no-env --no-hooks exec "rust@$rust_toolchain" -- mbx cache dir > "$probe_dir/cache-dir"
        exec 3< "$probe_dir/cache-dir"
        IFS= read -r cache_dir <&3 || { printf '%s\n' 'MBX cache dir output is empty or unterminated' >&2; exit 1; }
        extra=; if IFS= read -r extra <&3 || [ -n "$extra" ]; then printf '%s\n' 'MBX cache dir output has extra data' >&2; exit 1; fi
        exec 3<&-
        [ "$cache_dir" = "$MBX_CACHE_DIR/actions" ] || { printf '%s\n' 'MBX reported an unexpected object store path' >&2; exit 1; }
        ;;
    *)
        printf '%s\n' 'unsupported MBX check mode' >&2
        exit 2
        ;;
esac
"#;

pub(crate) fn with_runtime_identity_files(
    mut files: Vec<RenderedFile>,
    jobs: &BTreeMap<String, Job>,
    version: &str,
) -> Result<Vec<RenderedFile>, RenderError> {
    let lanes: BTreeSet<String> = jobs
        .values()
        .filter(|job| {
            job.steps.iter().any(|step| {
                step.role == Some(velnor_actions_contract::StepRole::ToolsCacheIdentity)
            })
        })
        .map(|job| job.runs_on.clone())
        .collect();
    for runs_on in &lanes {
        files.push(cache_p08::runtime_identity_action_file(runs_on, version)?);
        files.push(cache_p08::runtime_prelude_action_file(runs_on, version)?);
    }
    if !lanes.is_empty() {
        files.push(cache_p08::runtime_identity_script_file(version)?);
        files.push(crate::cache_steps::tools_restore_action_file(version)?);
        files.push(crate::tool_seed::action_file(version)?);
    }
    let has_mbx_steps = jobs.values().any(|job| {
        job.steps.iter().any(|step| {
            matches!(
                step.role,
                Some(StepRole::MbxPreflight | StepRole::MbxVersionCheck)
            )
        })
    });
    if has_mbx_steps {
        for (id, job) in jobs {
            let has_preflight = job
                .steps
                .iter()
                .any(|step| step.role == Some(StepRole::MbxPreflight));
            let has_version_check = job
                .steps
                .iter()
                .any(|step| step.role == Some(StepRole::MbxVersionCheck));
            if has_preflight != has_version_check {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mbx_script_step_pair_mismatch:{id}"
                )));
            }
        }
        files.push(mbx_rust_checks_script_file(version)?);
    }
    Ok(files)
}

fn mbx_rust_checks_script_file(version: &str) -> Result<RenderedFile, RenderError> {
    let bytes = marker::with_marker(version, MBX_RUST_CHECKS_SCRIPT_BODY)?;
    steps::scan_for_private_subcommands(&bytes)?;
    crate::guard::validate_tree_path(MBX_RUST_CHECKS_SCRIPT_PATH)?;
    Ok(RenderedFile {
        path: MBX_RUST_CHECKS_SCRIPT_PATH.to_owned(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{Job, JobTimeout, StepRole};

    use super::{
        MBX_RUST_CHECKS_SCRIPT_PATH, mbx_rust_checks_script_file, with_runtime_identity_files,
    };
    use crate::{RenderError, marker};

    fn job_with_mbx_roles(preflight: bool, version_check: bool) -> Result<Job, RenderError> {
        let mut steps = Vec::new();
        for (name, role) in [
            ("preflight", StepRole::MbxPreflight),
            ("version check", StepRole::MbxVersionCheck),
        ] {
            let included = match role {
                StepRole::MbxPreflight => preflight,
                StepRole::MbxVersionCheck => version_check,
                _ => false,
            };
            if included {
                let mut step =
                    crate::steps::shell_step(name, vec!["true".to_owned()], BTreeMap::new())?;
                step.role = Some(role);
                steps.push(step);
            }
        }
        Ok(Job {
            display_name: "fixture".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps,
        })
    }

    #[test]
    fn mbx_support_script_is_marked_and_has_a_closed_two_mode_dispatch() -> Result<(), RenderError>
    {
        let file = mbx_rust_checks_script_file("0.1.0")?;
        marker::check_first_line(&file.bytes, "0.1.0")?;
        assert_eq!(file.path, MBX_RUST_CHECKS_SCRIPT_PATH);
        assert!(file.bytes.contains("case \"$mode\" in"));
        assert!(file.bytes.contains("preflight)"));
        assert!(file.bytes.contains("verify)"));
        assert!(file.bytes.contains("unsupported MBX check mode"));
        assert!(file.bytes.contains("GITHUB_PATH"));
        assert!(
            file.bytes
                .contains("mise --no-config --no-env --no-hooks where \"rust@$rust_toolchain\"")
        );
        assert!(file.bytes.contains("Rustup did not report exact toolchain"));
        assert!(file.bytes.contains(
            "mise --no-config --no-env --no-hooks exec \"rust@$rust_toolchain\" -- mbx --version"
        ));
        assert!(
            file.bytes
                .contains("MBX version output is empty or unterminated")
        );
        assert!(file.bytes.contains("mbx $mbx_version"));
        assert!(file.bytes.contains("MBX store directories are missing"));
        assert!(file.bytes.contains("mbx cache dir"));
        Ok(())
    }

    #[test]
    fn mbx_support_file_is_conditional_and_requires_each_jobs_pair() -> Result<(), RenderError> {
        let no_mbx = with_runtime_identity_files(Vec::new(), &BTreeMap::new(), "0.1.0")?;
        assert!(no_mbx.is_empty());

        let paired = BTreeMap::from([("fixture".to_owned(), job_with_mbx_roles(true, true)?)]);
        let files = with_runtime_identity_files(Vec::new(), &paired, "0.1.0")?;
        assert_eq!(
            files
                .iter()
                .filter(|file| file.path == MBX_RUST_CHECKS_SCRIPT_PATH)
                .count(),
            1
        );

        for (preflight, version_check) in [(true, false), (false, true)] {
            let unpaired = BTreeMap::from([(
                "fixture".to_owned(),
                job_with_mbx_roles(preflight, version_check)?,
            )]);
            assert!(matches!(
                with_runtime_identity_files(Vec::new(), &unpaired, "0.1.0"),
                Err(RenderError::InvalidWorkflow(message))
                    if message == "mbx_script_step_pair_mismatch:fixture"
            ));
        }
        Ok(())
    }
}
