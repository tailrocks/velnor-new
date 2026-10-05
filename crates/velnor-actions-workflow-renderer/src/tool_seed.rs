//! Read-only tool seed at the fixed container path `/opt/velnor/seed`.
//!
//! A previous step cannot choose this path. A missing seed or a different
//! key stays cold. The job copies into its private homes. It does not
//! delete or write the seed.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::yaml::Yaml;

/// Container path of the authorized seed. Not a job input.
pub(crate) const SEED_ROOT: &str = "/opt/velnor/seed";
/// Display name of the copy step ahead of `Setup Mise`.
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
    require_seed_root(seed_root)?;
    Ok(copy_script(seed_root, "\"$SEED_KEY\""))
}

fn copy_script(seed_root: &str, key_shell: &str) -> String {
    format!(
        r#"set -eu; seed="{seed_root}"; key={key_shell}; if [ -z "$key" ]; then echo "tool seed key mismatch"; exit 0; fi; if [ ! -f "$seed/mise/KEY" ]; then echo "tool seed absent"; exit 0; fi; IFS= read -r seed_key < "$seed/mise/KEY" || true; if [ "$seed_key" != "$key" ]; then echo "tool seed key mismatch"; exit 0; fi; if [ -d "$seed/mise/tree" ]; then mkdir -p "$HOME/.local/share/mise"; cp -R "$seed/mise/tree/." "$HOME/.local/share/mise/"; echo "tool seed restored share-dir"; fi; if [ -d "$seed/rustup/tree" ]; then mkdir -p "$RUNNER_TEMP/velnor/rustup"; cp -R "$seed/rustup/tree/." "$RUNNER_TEMP/velnor/rustup/"; echo "tool seed restored toolchain-dir"; fi"#
    )
}

/// Insert the seed step immediately before the setup at `setup_index`.
///
/// A second call leaves the existing seed step in place. A job with no
/// checkout before the setup stays without the local action. GitHub
/// cannot load `./.github/actions/velnor-tool-seed` until checkout runs.
///
/// # Errors
///
/// Returns [`RenderError`] when the setup step has no qualified cache key.
pub(crate) fn insert_before_setup(job: &mut Job, setup_index: usize) -> Result<usize, RenderError> {
    if setup_index > 0 && job.steps[setup_index - 1].name == TOOL_SEED_NAME {
        return Ok(setup_index);
    }
    if !checkout_before(job, setup_index) {
        return Ok(setup_index);
    }
    let key = cache_key_at(job, setup_index)?;
    job.steps.insert(setup_index, seed_step(&key)?);
    Ok(setup_index + 1)
}

fn checkout_before(job: &Job, setup_index: usize) -> bool {
    job.steps[..setup_index].iter().any(|step| {
        step.name == "Checkout"
            || matches!(
                &step.kind,
                StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")
            )
    })
}

fn seed_step(cache_key: &str) -> Result<Step, RenderError> {
    if !crate::cache_p08::is_cache_key(cache_key) {
        return Err(RenderError::BadCommand(format!(
            "bad_cache_key:{cache_key}"
        )));
    }
    crate::steps::action_step(
        TOOL_SEED_NAME,
        TOOL_SEED_USES,
        BTreeMap::from([("cache_key".to_owned(), cache_key.to_owned())]),
    )
}

/// True when any job renders the tool-seed step.
pub(crate) fn any_job_has_seed(jobs: &std::collections::BTreeMap<String, Job>) -> bool {
    jobs.values()
        .any(|job| job.steps.iter().any(|step| step.name == TOOL_SEED_NAME))
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
                        Yaml::str("Exact Mise cache key.".to_owned()),
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

    fn key() -> String {
        crate::cache_p08::mise_cache_key_for_tools(
            "x86_64-unknown-linux-gnu",
            "2026.9.18",
            &["rust@1.98.1".to_owned()],
        )
        .expect("key")
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("velnor-tool-seed-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("scratch");
        path
    }

    fn run_key(script: &str, home: &std::path::Path, seed_key: Option<&str>) -> String {
        let mut command = std::process::Command::new("bash");
        command
            .arg("-c")
            .arg(script)
            .env("HOME", home)
            .env("RUNNER_TEMP", home.join("rt"));
        if let Some(seed_key) = seed_key {
            command.env("SEED_KEY", seed_key);
        }
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
        let cache_key = key();
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
        let cache_key = key();
        std::fs::create_dir_all(seed.join("mise/tree")).expect("tree");
        std::fs::write(seed.join("mise/KEY"), "mise-v1-other-key-0123456789abcdef").expect("key");
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
    fn action_script_reads_seed_key_and_the_action_file_keeps_the_seed() {
        let root = scratch("action");
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        let seed = root.join("seed");
        let cache_key = key();
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
        let wrong = run_key(&script, &home, Some("mise-v1-other-key-0123456789abcdef"));
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

    fn job(steps: Vec<Step>) -> Job {
        Job {
            display_name: "Required".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: velnor_actions_contract::JobTimeout::PLAN,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            check_runner: None,
            steps,
        }
    }

    fn setup(cache_key: &str) -> Step {
        crate::steps::action_step(
            "Setup Mise",
            "jdx/mise-action@0123456789abcdef0123456789abcdef01234567",
            BTreeMap::from([("cache_key".to_owned(), cache_key.to_owned())]),
        )
        .expect("setup")
    }

    #[test]
    fn local_seed_requires_a_prior_checkout() {
        let cache_key = key();
        let mut bare = job(vec![setup(&cache_key)]);
        let index = insert_before_setup(&mut bare, 0).expect("bare");
        assert_eq!(index, 0);
        assert!(bare.steps.iter().all(|step| step.name != TOOL_SEED_NAME));
        let checkout = crate::steps::checkout_step(
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
        )
        .expect("checkout");
        let mut checked = job(vec![checkout, setup(&cache_key)]);
        let index = insert_before_setup(&mut checked, 1).expect("checked");
        assert_eq!(index, 2);
        assert_eq!(checked.steps[1].name, TOOL_SEED_NAME);
        let again = insert_before_setup(&mut checked, 2).expect("again");
        assert_eq!(again, 2);
    }
}
