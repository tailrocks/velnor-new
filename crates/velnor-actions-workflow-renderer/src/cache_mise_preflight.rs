//! Verify late-restored bootstrap bytes before invoking the owned Mise binary.
use std::collections::BTreeMap;
use velnor_actions_contract::{SourceBoundOperation, Step, ToolCacheDomain};

use crate::{MiseBootstrap, RenderError};

pub(super) const PREFLIGHT_NAME: &str = "Verify restored Mise binary";
const TEMP_ENV: &str = "VELNOR_MISE_RUNNER_TEMP";
const SHA_ENV: &str = "VELNOR_MISE_PREFLIGHT_SHA256";
const RUNNER_TEMP: &str = "${{ runner.temp }}";

// Only owned descendants of the trusted runner temp anchor are inspected.
// Neither verification nor repair executes restored bytes or traverses links.
const SCRIPT: &str = r#"set -eu
temp=${VELNOR_MISE_RUNNER_TEMP:?missing runner temp}
sha=${VELNOR_MISE_PREFLIGHT_SHA256:?missing Mise digest}
case "$temp" in /*) ;; *) exit 1;; esac
case "$temp/" in */../*|*/./*|*//*) exit 1;; esac
case "$sha" in *[!0-9a-f]*) exit 1;; esac
test "${#sha}" -eq 64 || exit 1
case "${VELNOR_MISE_CACHE_DOMAIN:?missing cache domain}" in
  tools) relative=mise; set -- mise ;;
  planning) relative=planning/mise; set -- planning mise ;;
  npm-bootstrap) relative=npm-source/mise; set -- npm-source mise ;;
  bun-bootstrap) relative=bun-source/mise; set -- bun-source mise ;;
  tofu-bootstrap) relative=tofu-provider-producer/mise; set -- tofu-provider-producer mise ;;
  gradle-bootstrap) relative=gradle-source/mise; set -- gradle-source mise ;;
  *) exit 1 ;;
esac
root="$temp/velnor/$relative"
# Walk each owned ancestor, including the native producer directory.
current="$temp"
for component in velnor "$@" bin; do
  test ! -L "$current" || exit 1
  if test -e "$current"; then test -d "$current" || exit 1; else exit 0; fi
  current="$current/$component"
done
test ! -L "$current" || exit 1
if test -e "$current"; then test -d "$current" || exit 1; else exit 0; fi
binary="$root/bin/mise"
if test -L "$binary"; then /bin/rm -f -- "$binary"; exit 0; fi
test -e "$binary" || exit 0
test -f "$binary" || exit 1
if test -x /usr/bin/sha256sum; then
  if test -x "$binary" && printf '%s  %s\n' "$sha" "$binary" | /usr/bin/sha256sum -c -; then exit 0; fi
elif test -x /usr/bin/shasum; then
  if test -x "$binary" && printf '%s  %s\n' "$sha" "$binary" | /usr/bin/shasum -a 256 -c -; then exit 0; fi
else
  exit 1
fi
/bin/rm -f -- "$binary"
"#;

/// Verify restored bytes using the exact owner-qualified binary pin and domain.
/// # Errors
/// Rejects malformed compiled binary digests before emitting any shell step.
pub(crate) fn preflight_step(
    pin: &MiseBootstrap,
    domain: ToolCacheDomain,
) -> Result<Step, RenderError> {
    let invocation = pin.helper.invocation();
    invocation.validate().map_err(RenderError::Contract)?;
    if invocation.descriptor().operation() != SourceBoundOperation::MiseBootstrap
        || !invocation.installed_selectors().is_empty()
        || !velnor_actions_contract::ids::is_lower_hex_len(&pin.binary_sha256, 64)
        || pin.helper.environment().get("VELNOR_MISE_SHA256") != Some(&pin.binary_sha256)
        || pin
            .helper
            .environment()
            .get("MISE_DATA_DIR")
            .map(String::as_str)
            != Some(domain.root())
    {
        return Err(invalid("bad_mise_sha256"));
    }
    crate::steps::shell_step(
        PREFLIGHT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), SCRIPT.to_owned()],
        BTreeMap::from([
            (TEMP_ENV.to_owned(), RUNNER_TEMP.to_owned()),
            (SHA_ENV.to_owned(), pin.binary_sha256.clone()),
            (
                "VELNOR_MISE_CACHE_DOMAIN".to_owned(),
                domain_name(domain).to_owned(),
            ),
        ]),
    )
}

/// Recognize only this generator-owned script through the scrub prefix.
pub(crate) fn is_preflight_argv(argv: &[String]) -> bool {
    let prefix = crate::toolchain_env::unset_prefix_len(argv);
    matches!(&argv[prefix..], [program, flag, script]
        if program == "sh" && flag == "-c"
            && (script == SCRIPT
                || *script == crate::toolchain_env::with_credential_unset_script(SCRIPT)))
}

/// Reject changed preflight controls before workflow serialization.
pub(crate) fn validate_preflight_env(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    if !env.contains_key(SHA_ENV)
        && !env.contains_key(TEMP_ENV)
        && !env.contains_key("VELNOR_MISE_CACHE_DOMAIN")
    {
        return Ok(());
    }
    if !env
        .get("VELNOR_MISE_CACHE_DOMAIN")
        .is_some_and(|name| domains().iter().any(|domain| domain_name(*domain) == name))
        || env.get(TEMP_ENV).map(String::as_str) != Some(RUNNER_TEMP)
        || !env
            .get(SHA_ENV)
            .is_some_and(|sha| velnor_actions_contract::ids::is_lower_hex_len(sha, 64))
    {
        return Err(invalid("mise_preflight_contract_changed"));
    }
    Ok(())
}

fn domain_name(domain: ToolCacheDomain) -> &'static str {
    if domain == ToolCacheDomain::Full {
        "tools"
    } else {
        domain.name()
    }
}

fn domains() -> [ToolCacheDomain; 6] {
    [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ]
}

fn invalid(reason: &str) -> RenderError {
    RenderError::BadCommand(reason.to_owned())
}

#[cfg(test)]
#[path = "cache_mise_preflight_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cache_mise_preflight_domain_tests.rs"]
mod domain_tests;
