//! Fixed fresh compiler source; independent of Full cache namespaces.

use crate::{
    MiseError,
    catalog::{
        ToolCatalog,
        rust_bootstrap::{RuntimePreparationPurpose, RustupBootstrap},
        rust_compiler_authority::GuardedRootRustInstallSource,
        rust_proxies::repair_data_script_for_host,
    },
    source_intent_cold_root::SourceIntentColdRoot,
};

pub(super) fn source(
    root: SourceIntentColdRoot,
    version: &str,
    mise_sha: &str,
    guarded_install: &GuardedRootRustInstallSource,
) -> Result<String, MiseError> {
    let catalog = ToolCatalog::pinned();
    let host = root.host();
    let body = format!(
        "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\nexport RUSTUP_AUTO_INSTALL=0\nexport MISE_AUTO_INSTALL=false\nexport MISE_EXEC_AUTO_INSTALL=false\n{ROOTS}\ntest \"${{RUSTUP_TOOLCHAIN:?missing exact toolchain}}\" = '{}'\nprintf '%s  %s\\n' '{mise_sha}' \"$mise\" | {}\n{TRUSTED_PATH}\n{}\n{}\n{}\n",
        catalog.rust_toolchain_name(),
        host.sha256_command(),
        RustupBootstrap::for_host(host).script(RuntimePreparationPurpose::SourceIntent(root))?,
        guarded_install.source(),
        repair_data_script_for_host(host),
    );
    velnor_actions_contract::generated_source(version, &body).map_err(|error| MiseError::Contract {
        problem: error.to_string(),
    })
}

const ROOTS: &str = r#"temp=${RUNNER_TEMP:?missing runner temp}
root=${VELNOR_SOURCE_INTENT_COLD_ROOT:?missing SourceIntent root}
test "$root" = "$temp/velnor-control/source-intent"
test "${MISE_DATA_DIR:?missing Mise home}" = "$root/mise"
test "${CARGO_HOME:?missing Cargo home}" = "$root/cargo"
test "${RUSTUP_HOME:?missing Rustup home}" = "$root/rustup-home"
test "${MISE_CARGO_HOME:?missing explicit Cargo home}" = "$root/cargo"
test "${MISE_RUSTUP_HOME:?missing explicit Rustup home}" = "$root/rustup-home"
for directory in "$temp" "$temp/velnor-control" "$root" "$root/mise" "$root/mise/bin"; do
  test -d "$directory"
  test ! -L "$directory"
done
for entry in "$root/cargo" "$root/rustup-home" "$root/rustup-bootstrap" "$root/mise-config" "$root/mise-system-config" "$root/trusted-bin"; do
  test ! -e "$entry"
  test ! -L "$entry"
done
mkdir -m 700 "$root/cargo" "$root/rustup-home" "$root/mise-config" "$root/mise-system-config" "$root/trusted-bin"
mise="$root/mise/bin/mise"
test -f "$mise"
test ! -L "$mise"
test -x "$mise"
cd -P "$root/mise/bin"
mise="$PWD/mise"
cd -P "$root"
export MISE_CEILING_PATHS="$PWD"
export MISE_CONFIG_DIR="$root/mise-config"
export MISE_SYSTEM_CONFIG_DIR="$root/mise-system-config""#;

const TRUSTED_PATH: &str = r#"ln -s -- "$mise" "$root/trusted-bin/mise"
export PATH="$root/trusted-bin:/usr/bin:/bin:/usr/sbin:/sbin""#;
