//! HD-7: the renderer and the Mise adapter share one private-fetch
//! credential contract, and only the private-fetch builder reaches the
//! private-fetch policy.

use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_mise::command as mise;
use velnor_actions_workflow_renderer::toolchain_env as renderer;

#[test]
fn allowlists_and_redirect_keys_are_pinned_equal() {
    assert_eq!(
        renderer::STEP_PRIVATE_FETCH_ALLOWLIST,
        mise::CREDENTIAL_ALLOWLIST_PRIVATE_FETCH,
        "rendered bindings and local spawns share one private-fetch allowlist"
    );
    assert_eq!(
        renderer::STEP_FETCH_REDIRECT_DENYLIST,
        mise::FETCH_REDIRECT_ENV_KEYS,
        "rendered steps and local spawns share one fetch-redirect denylist"
    );
    assert_eq!(
        [
            velnor_actions_contract::PRIVATE_FETCH_TOKEN_ENV,
            velnor_actions_contract::PRIVATE_NPM_READ_TOKEN_ENV,
        ],
        mise::CREDENTIAL_ALLOWLIST_PRIVATE_FETCH,
        "the contract's binding keys are the policy's allowlist"
    );
}

#[test]
fn redirect_predicates_agree_on_a_corpus() {
    for key in [
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "GIT_CONFIG",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_3",
        "GIT_CONFIG_VALUE_3",
        "GIT_CONFIG_PARAMETERS",
        "NPM_CONFIG_REGISTRY",
        "npm_config_registry",
        "BUN_CONFIG_REGISTRY",
        "GIT_TERMINAL_PROMPT",
        "GIT_DIR",
        "PATH",
        "NPM",
        "CARGO_HOME",
        "BUN_INSTALL",
    ] {
        assert_eq!(
            renderer::is_fetch_redirect_key(key),
            mise::is_fetch_redirect_key(key),
            "fetch-redirect parity for {key}"
        );
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Production sources outside the policy's own module that name the policy
/// or its constructor; tests and the Mise adapter's definition are exempt.
#[test]
fn only_the_private_fetch_builders_reach_the_private_fetch_policy() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for krate in [
        "velnor-actions-orchestrator",
        "velnor-actions-cli",
        "velnor-actions-workflow-renderer",
        "velnor-actions-rust",
        "velnor-actions-tofu",
    ] {
        rust_files(&crates.join(krate).join("src"), &mut files);
    }
    let allowed =
        |name: &str| name.starts_with("private_fetch") || name == "bun_registry_install.rs";
    let mut offenders = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.ends_with("_tests.rs") || allowed(&name) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        if text.contains("IsolatedCommand::private_fetch")
            || text.contains("EnvPolicy::PrivateFetch")
        {
            offenders.push(file.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "private-fetch policy used outside its builders: {offenders:?}"
    );
}
