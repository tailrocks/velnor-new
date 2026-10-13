//! Read-only tool seed at the fixed container path `/opt/velnor/seed`.
//!
//! A previous step cannot choose this path. A missing seed or a different
//! key stays cold. The job copies into its private homes. It does not
//! delete or write the seed.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::RenderError;
use crate::yaml::Yaml;

/// Container path of the authorized seed. Not a job input.
pub(crate) const SEED_ROOT: &str = "/opt/velnor/seed";
/// Image provisioner marker required before any seed bytes can be consumed.
const SEED_PROVENANCE: &str = "velnor-host-seed-v1";
/// Display name of the copy step between V2 identity and restore.
pub(crate) const TOOL_SEED_NAME: &str = "Restore Velnor tool seed";
/// Workflow `uses` of the one local tool-seed composite.
pub(crate) const TOOL_SEED_USES: &str = "./.github/actions/velnor-tool-seed";
/// Repository path of that composite.
const TOOL_SEED_ACTION_PATH: &str = ".github/actions/velnor-tool-seed/action.yml";

/// Reject a seed root that could change the script's quoting.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] when `root` is not one absolute
/// path of ASCII letters, digits, `/`, `.`, `_`, and `-`.
pub(crate) fn require_seed_root(root: &str) -> Result<(), RenderError> {
    let bytes_ok = root
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
    if root.starts_with('/') && !root.contains("..") && !root.contains("//") && bytes_ok {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_seed_root:{root}")))
    }
}

/// Copy script. The composite sets `SEED_KEY` from `inputs.cache_key`.
///
/// The script does not embed a job-specific key, so one action file serves
/// every job. An empty key does not match. Rustup lands at
/// `$RUNNER_TEMP/velnor/rustup`, the same path as `MISE_RUSTUP_HOME`.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for a bad root.
pub(crate) fn tool_seed_action_script(seed_root: &str) -> Result<String, RenderError> {
    copy_script(seed_root, "\"$SEED_KEY\"")
}

fn copy_script(seed_root: &str, key_shell: &str) -> Result<String, RenderError> {
    let trust = trusted_seed_guard(seed_root)?;
    Ok(format!(
        r#"set -eu; {trust}; seed="$trusted_seed_root"; key={key_shell}; if [ -z "$key" ]; then echo "tool seed key mismatch"; exit 0; fi; if [ ! -d "$trusted_seed_root" ]; then echo "tool seed absent"; exit 0; fi; if ! seed_trusted "$trusted_seed_root"; then echo "tool seed untrusted; continuing cold"; exit 0; fi; if [ ! -f "$seed/mise/KEY" ]; then echo "tool seed absent"; exit 0; fi; IFS= read -r seed_key < "$seed/mise/KEY" || true; if [ "$seed_key" != "$key" ]; then echo "tool seed key mismatch"; exit 0; fi; if [ -d "$seed/mise/tree" ]; then mkdir -p "$HOME/.local/share/mise"; cp -R "$seed/mise/tree/." "$HOME/.local/share/mise/"; echo "tool seed restored share-dir"; fi; if [ -d "$seed/rustup/tree" ]; then mkdir -p "$RUNNER_TEMP/velnor/rustup"; cp -R "$seed/rustup/tree/." "$RUNNER_TEMP/velnor/rustup/"; echo "tool seed restored toolchain-dir"; fi"#
    ))
}

/// Shell preflight for a host-provisioned seed. Key agreement alone does
/// not establish who supplied the bytes or whether the shared source is
/// mutable during the job.
pub(crate) fn trusted_seed_guard(seed_root: &str) -> Result<String, RenderError> {
    require_seed_root(seed_root)?;
    Ok(format!(
        r#"trusted_seed_root="{seed_root}"; seed_owner() {{ if [ "$(uname -s)" = Darwin ]; then stat -f %u "$1"; else stat -c %u -- "$1"; fi; }}; seed_trusted() {{ [ -d "$1" ] && [ ! -L "$1" ] || return 1; [ -f "$1/PROVENANCE" ] && [ ! -L "$1/PROVENANCE" ] || return 1; [ "$(seed_owner "$1")" = 0 ] || return 1; [ "$(seed_owner "$1/PROVENANCE")" = 0 ] || return 1; IFS= read -r provenance < "$1/PROVENANCE" || return 1; [ "$provenance" = "{SEED_PROVENANCE}" ] || return 1; if [ "$(uname -s)" = Darwin ]; then mount | grep -F " on $1 (" | grep -E '(^|, )read-only(,|\))' >/dev/null 2>&1 || return 1; else options=$(findmnt -rn -T "$1" -o OPTIONS 2>/dev/null) || return 1; case ",$options," in *,ro,*) ;; *) return 1 ;; esac; fi; unsafe=$(find "$1" -xdev \( -type l -o ! -user 0 \) -print -quit 2>/dev/null) || return 1; [ -z "$unsafe" ]; }}"#
    ))
}

/// Build the seed restore from the same typed payload as the V2 archive.
///
/// # Errors
///
/// Returns [`RenderError`] if the local action call is malformed.
pub(crate) fn seed_step(
    payload: &crate::cache_p08::ToolsCachePayload,
) -> Result<Step, RenderError> {
    let cache_key = payload.key_expression();
    if !crate::cache_p08::is_v2_cache_key_expression(&cache_key) {
        return Err(RenderError::InvalidWorkflow(
            "bad_tools_seed_key_expression".to_owned(),
        ));
    }
    crate::steps::action_step(
        TOOL_SEED_NAME,
        TOOL_SEED_USES,
        BTreeMap::from([("cache_key".to_owned(), cache_key)]),
    )
}

/// Validate the renderer-owned local action call, not just its path.
/// # Errors
pub(crate) fn validate_action_call(
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let valid_key = with
        .get("cache_key")
        .is_some_and(|key| crate::cache_p08::is_v2_cache_key_expression(key));
    if step.name != TOOL_SEED_NAME
        || step.condition.is_some()
        || uses != TOOL_SEED_USES
        || with.len() != 1
        || !valid_key
        || !env.is_empty()
    {
        return Err(RenderError::InvalidWorkflow(
            "malformed_tools_seed_action".to_owned(),
        ));
    }
    Ok(())
}

/// True when any job renders the tool-seed step.
pub(crate) fn any_job_has_seed(
    jobs: &std::collections::BTreeMap<String, velnor_actions_contract::Job>,
) -> bool {
    jobs.values().any(|job| {
        job.steps.iter().any(|step| {
            let StepKind::Action { uses, with, env } = &step.kind else {
                return false;
            };
            validate_action_call(step, uses, with, env).is_ok()
        })
    })
}

/// One composite action for every tool-seed step.
///
/// The workflow step stays a short `uses` plus the cache key. The copy
/// script lives here, outside the 500 KB workflow cap.
///
/// # Errors
///
/// Returns [`RenderError`] when the version or the script is invalid.
pub(crate) fn action_file(version: &str) -> Result<crate::tree::RenderedFile, RenderError> {
    let script = tool_seed_action_script(SEED_ROOT)?;
    let step = crate::steps::shell_step(
        "Copy matching tool seed",
        vec!["bash".to_owned(), "-c".to_owned(), script],
        BTreeMap::from([("SEED_KEY".to_owned(), "${{ inputs.cache_key }}".to_owned())]),
    )?;
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow("tool_seed_step".to_owned()));
    };
    let body = action_yaml(&step.name, env, &crate::commands::join_argv_for_run(run)?);
    let quoted = crate::yaml::quote_run_values_in_yaml(body);
    let bytes = crate::marker::with_marker(version, &crate::yaml::render_yaml(&quoted))?;
    crate::steps::scan_for_private_subcommands(&bytes)?;
    Ok(crate::tree::RenderedFile {
        path: TOOL_SEED_ACTION_PATH.to_owned(),
        bytes,
    })
}

fn action_yaml(step_name: &str, env: &BTreeMap<String, String>, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(TOOL_SEED_NAME.to_owned())),
        (
            "description".to_owned(),
            Yaml::str("Copy a matching host tool seed into this job.".to_owned()),
        ),
        (
            "inputs".to_owned(),
            Yaml::Map(vec![(
                "cache_key".to_owned(),
                Yaml::Map(vec![
                    (
                        "description".to_owned(),
                    Yaml::str("Exact V2 tools-cache identity.".to_owned()),
                    ),
                    ("required".to_owned(), Yaml::Bool(true)),
                ]),
            )]),
        ),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite".to_owned())),
                (
                    "steps".to_owned(),
                    Yaml::Seq(vec![Yaml::Map(vec![
                        ("name".to_owned(), Yaml::str(step_name.to_owned())),
                        (
                            "env".to_owned(),
                            crate::document_steps::string_map_yaml(env),
                        ),
                        ("shell".to_owned(), Yaml::str("bash".to_owned())),
                        ("run".to_owned(), Yaml::str(run.to_owned())),
                    ])]),
                ),
            ]),
        ),
    ])
}

fn cache_key_at(job: &Job, index: usize) -> Result<String, RenderError> {
    let Some(step) = job.steps.get(index) else {
        return Err(RenderError::InvalidWorkflow("setup_missing".to_owned()));
    };
    let StepKind::Action { with, .. } = &step.kind else {
        return Err(RenderError::InvalidWorkflow("setup_missing".to_owned()));
    };
    with.get("cache_key")
        .filter(|key| crate::cache_p08::is_cache_key(key))
        .cloned()
        .ok_or_else(|| RenderError::InvalidWorkflow("setup_missing_key".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn payload() -> crate::cache_p08::ToolsCachePayload {
        let mise = crate::setup::MiseSetup {
            uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
            version: "2026.9.18".to_owned(),
            sha256: "a".repeat(64),
        };
        crate::cache_p08::ToolsCachePayload::new(crate::cache_p08::ToolsCacheInputs {
            runs_on: "ubuntu-26.04",
            target: "x86_64-unknown-linux-gnu",
            mise_setup: &mise,
            tool_specs: &["rust@1.98.1".to_owned()],
            rustup_toolchain: Some("1.98.1"),
            rustup_components: &[],
        })
        .expect("payload")
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("velnor-tool-seed-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("scratch");
        path
    }

    fn trusted_test_path(home: &std::path::Path) -> String {
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        for (name, body) in [
            ("uname", "#!/bin/sh\necho Linux\n"),
            ("stat", "#!/bin/sh\necho \"${SEED_OWNER_UID:-0}\"\n"),
            (
                "findmnt",
                "#!/bin/sh\necho \"${SEED_MOUNT_OPTIONS:-ro,relatime}\"\n",
            ),
            (
                "find",
                "#!/bin/sh\nif [ \"${SEED_UNSAFE:-0}\" = 1 ]; then echo \"$1/untrusted\"; fi\n",
            ),
        ] {
            let path = bin.join(name);
            std::fs::write(&path, body).expect("fake command");
            let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).expect("executable");
        }
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    fn write_provenance(seed: &std::path::Path) {
        std::fs::create_dir_all(seed).expect("seed root");
        std::fs::write(seed.join("PROVENANCE"), SEED_PROVENANCE).expect("provenance");
    }

    fn run_key(script: &str, home: &std::path::Path, seed_key: Option<&str>) -> String {
        run_key_with_provenance(script, home, seed_key, "0", "ro,relatime", "0")
    }

    fn run_key_with_provenance(
        script: &str,
        home: &std::path::Path,
        seed_key: Option<&str>,
        owner_uid: &str,
        mount_options: &str,
        unsafe_tree: &str,
    ) -> String {
        let mut command = std::process::Command::new("bash");
        command
            .arg("-c")
            .arg(script)
            .env("HOME", home)
            .env("RUNNER_TEMP", home.join("rt"));
        let path = trusted_test_path(home);
        if let Some(seed_key) = seed_key {
            command.env("SEED_KEY", seed_key);
        }
        command
            .env("PATH", path)
            .env("SEED_OWNER_UID", owner_uid)
            .env("SEED_MOUNT_OPTIONS", mount_options)
            .env("SEED_UNSAFE", unsafe_tree);
        let output = command.output().expect("bash");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[test]
    fn matching_seed_copies_mise_and_rustup_and_keeps_the_seed() {
        let root = scratch("hit");
        let home = root.join("home");
        std::fs::create_dir_all(home.join("keep")).expect("home");
        let seed = root.join("seed");
        let cache_key = payload().key_expression();
        write_provenance(&seed);
        std::fs::create_dir_all(seed.join("mise/tree/installs")).expect("tree");
        std::fs::create_dir_all(seed.join("rustup/tree/toolchains")).expect("rustup");
        std::fs::write(seed.join("mise/KEY"), &cache_key).expect("key file");
        std::fs::write(seed.join("mise/tree/installs/marker"), "mise-bytes").expect("marker");
        std::fs::write(seed.join("rustup/tree/toolchains/marker"), "rustup-bytes").expect("marker");
        let script = tool_seed_action_script(seed.to_str().expect("utf8")).expect("script");
        let text = run_key(&script, &home, Some(&cache_key));
        assert!(text.contains("tool seed restored share-dir"), "{text}");
        assert!(text.contains("tool seed restored toolchain-dir"), "{text}");
        assert_eq!(
            std::fs::read_to_string(home.join(".local/share/mise/installs/marker")).expect("copy"),
            "mise-bytes"
        );
        assert_eq!(
            std::fs::read_to_string(home.join("rt/velnor/rustup/toolchains/marker")).expect("copy"),
            "rustup-bytes"
        );
        assert_eq!(
            std::fs::read_to_string(seed.join("mise/tree/installs/marker")).expect("seed"),
            "mise-bytes"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn wrong_key_and_absent_seed_do_not_copy() {
        let root = scratch("miss");
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let seed = root.join("seed");
        let cache_key = payload().key_expression();
        write_provenance(&seed);
        std::fs::create_dir_all(seed.join("mise/tree")).expect("tree");
        std::fs::write(seed.join("mise/KEY"), "not-the-v2-key").expect("key");
        std::fs::write(seed.join("mise/tree/marker"), "secret").expect("marker");
        let wrong = tool_seed_action_script(seed.to_str().expect("utf8")).expect("script");
        let text = run_key(&wrong, &home, Some(&cache_key));
        assert!(text.contains("tool seed key mismatch"), "{text}");
        assert!(!home.join(".local/share/mise/marker").exists());
        assert_eq!(
            std::fs::read_to_string(seed.join("mise/tree/marker")).expect("seed"),
            "secret"
        );
        let absent =
            tool_seed_action_script(root.join("missing").to_str().expect("utf8")).expect("script");
        let text = run_key(&absent, &home, Some(&cache_key));
        assert!(text.contains("tool seed absent"), "{text}");
        assert!(tool_seed_action_script("relative").is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn seed_restore_requires_root_owned_read_only_provisioned_tree() {
        let root = scratch("untrusted");
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let seed = root.join("seed");
        let cache_key = payload().key_expression();
        write_provenance(&seed);
        std::fs::create_dir_all(seed.join("mise/tree")).expect("tree");
        std::fs::write(seed.join("mise/KEY"), &cache_key).expect("key");
        std::fs::write(seed.join("mise/tree/marker"), "secret").expect("marker");
        let script = tool_seed_action_script(seed.to_str().expect("utf8")).expect("script");

        for (owner, mount, unsafe_tree) in [
            ("1000", "ro,relatime", "0"),
            ("0", "rw,relatime", "0"),
            ("0", "ro,relatime", "1"),
        ] {
            let output = run_key_with_provenance(
                &script,
                &home,
                Some(&cache_key),
                owner,
                mount,
                unsafe_tree,
            );
            assert!(output.contains("tool seed untrusted; continuing cold"), "{output}");
            assert!(!home.join(".local/share/mise/marker").exists());
        }
        assert_eq!(
            std::fs::read_to_string(seed.join("mise/tree/marker")).expect("seed remains"),
            "secret"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn action_script_reads_seed_key_and_the_action_file_keeps_the_seed() {
        let root = scratch("action");
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let seed = root.join("seed");
        let cache_key = payload().key_expression();
        write_provenance(&seed);
        std::fs::create_dir_all(seed.join("mise/tree")).expect("tree");
        std::fs::write(seed.join("mise/KEY"), &cache_key).expect("key");
        std::fs::write(seed.join("mise/tree/marker"), "kept").expect("marker");
        let script = tool_seed_action_script(seed.to_str().expect("utf8")).expect("script");
        let text = run_key(&script, &home, Some(&cache_key));
        assert!(text.contains("tool seed restored share-dir"), "{text}");
        assert_eq!(
            std::fs::read_to_string(home.join(".local/share/mise/marker")).expect("copy"),
            "kept"
        );
        assert_eq!(
            std::fs::read_to_string(seed.join("mise/tree/marker")).expect("seed"),
            "kept"
        );
        let wrong = run_key(&script, &home, Some("mise-tools-v2-other-key"));
        assert!(wrong.contains("tool seed key mismatch"), "{wrong}");
        let file = action_file("0.1.0").expect("action");
        assert_eq!(file.path, ".github/actions/velnor-tool-seed/action.yml");
        assert!(file.bytes.contains("/opt/velnor/seed"), "{}", file.bytes);
        assert!(file.bytes.contains("$SEED_KEY"), "{}", file.bytes);
        assert!(file.bytes.contains("inputs.cache_key"), "{}", file.bytes);
        assert!(file.bytes.contains("unset "), "{}", file.bytes);
        assert!(!file.bytes.contains("rm "), "{}", file.bytes);
        std::fs::remove_dir_all(&root).ok();
    }

}
