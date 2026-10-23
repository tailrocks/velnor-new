use super::{ShellDialect, bodies};

#[test]
fn generated_release_verifier_shell_is_scanned_as_bash() -> Result<(), String> {
    let generated = include_str!("../tests/snapshots/product-release.yml");
    let runs = bodies(generated)?;
    assert!(
        runs.iter().any(|(body, shell)| {
            *shell == ShellDialect::Bash
                && body.starts_with(
                    r#"function_env_unset=(); function_env_scan_complete=0; while IFS= read -r -d '' env_entry; do case "$env_entry" in BASH_FUNC_*=*) function_env_unset+=(-u "${env_entry%%=*}");; VELNOR_VERIFIER_ENV_SCAN_COMPLETE) function_env_scan_complete=$((function_env_scan_complete + 1));; esac; done < <({ /usr/bin/env -0 && printf '%s\000' 'VELNOR_VERIFIER_ENV_SCAN_COMPLETE'; }); [ "$function_env_scan_complete" -eq 1 ] || exit 1; exec /usr/bin/env "${function_env_unset[@]}" -u BASH_ENV -u ENV "#,
                )
                && body.contains(
                    r"-u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR /bin/bash --noprofile --norc -p -e -o pipefail -c 'unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR; ",
                )
                && body.contains("PATH=\"/usr/bin:/bin:/usr/sbin:/sbin\"; export PATH; ")
        }),
        "generated verifier must use the complete credential, startup and PATH boundary"
    );
    Ok(())
}
