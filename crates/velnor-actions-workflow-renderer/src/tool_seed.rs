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

/// Derive the immutable host-seed key from the resolved tool selectors.
///
/// This key is separate from the remote V2 archive key: the seed is an
/// image-provisioned snapshot, so it cannot depend on observations made by
/// the runtime identity step.
/// # Errors
pub(crate) fn seed_key_for_tools(
    target: &str,
    mise_version: &str,
    tool_specs: &[String],
) -> Result<String, RenderError> {
    if !velnor_actions_contract::is_supported_target(target)
        || mise_version.is_empty()
        || mise_version == "latest"
        || !mise_version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
        || tool_specs.is_empty()
        || tool_specs
            .iter()
            .any(|spec| !crate::cache_p08::is_tool_spec(spec))
    {
        return Err(RenderError::BadCommand("bad_tool_seed_identity".to_owned()));
    }
    let mut specs = tool_specs.to_vec();
    specs.sort();
    specs.dedup();
    let digest = velnor_actions_contract::digest_b3(specs.join(",").as_bytes());
    let short_digest = digest
        .get(3..19)
        .ok_or_else(|| RenderError::BadCommand("bad_tool_seed_digest".to_owned()))?;
    Ok(format!("mise-v1-{target}-{mise_version}-{short_digest}"))
}

/// Insert the seed step immediately before the setup at `setup_index`.
///
/// A second call leaves the existing seed step in place. A job with no
/// checkout before the setup stays without the local action. GitHub
/// cannot load `./.github/actions/velnor-tool-seed` until checkout runs.
///
/// # Errors
///
/// Returns [`RenderError`] when the seed key or an existing seed step is invalid.
pub(crate) fn insert_before_setup(
    job: &mut Job,
    setup_index: usize,
    cache_key: &str,
) -> Result<usize, RenderError> {
    let seed = seed_step(cache_key)?;
    if setup_index > 0 && job.steps[setup_index - 1].name == TOOL_SEED_NAME {
        if job.steps[setup_index - 1] != seed {
            return Err(RenderError::InvalidWorkflow(
                "tool_seed_step_mismatch".to_owned(),
            ));
        }
        return Ok(setup_index);
    }
    if !checkout_before(job, setup_index) {
        return Ok(setup_index);
    }
    job.steps.insert(setup_index, seed);
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
    if !is_seed_key(cache_key) {
        return Err(RenderError::BadCommand(format!(
            "bad_cache_key:{cache_key}"
        )));
    }
    let mut step = crate::steps::action_step(
        TOOL_SEED_NAME,
        TOOL_SEED_USES,
        BTreeMap::from([("cache_key".to_owned(), cache_key.to_owned())]),
    )?;
    step.condition = Some("github.event_name != 'workflow_dispatch'".to_owned());
    Ok(step)
}

fn is_seed_key(value: &str) -> bool {
    let Some((prefix, digest)) = value.rsplit_once('-') else {
        return false;
    };
    prefix.starts_with("mise-v1-")
        && digest.len() == 16
        && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'+'))
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
                        Yaml::str("Exact immutable host-seed key.".to_owned()),
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

#[cfg(test)]
#[path = "tool_seed_tests.rs"]
mod tests;
