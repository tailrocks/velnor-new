//! Fixed Rust helper bytes; configuration cannot supply source or executable paths.

use super::{MiseError, RustCompilerRole, catalog_for_role};
use crate::{
    MISE_GLOBAL_FLAGS,
    catalog::{
        rust_bootstrap::{RuntimePreparationPurpose, RustupBootstrap},
        rust_compiler_authority::RootRustCandidateSource,
        rust_health::RustToolchainHealth,
        rust_proxies::repair_script_for_host,
    },
    root_rust_candidate_root::RootRustCandidateRoot,
};

pub(super) fn source(
    role: RustCompilerRole,
    version: &str,
    mise: &super::QualifiedDistribution,
    mbx: Option<&super::QualifiedDistribution>,
) -> Result<String, MiseError> {
    let catalog = catalog_for_role(role)?;
    let host = catalog.rust_host();
    let health = RustToolchainHealth::for_catalog(&catalog, catalog.rust_install_options());
    let bootstrap = RustupBootstrap::for_host(host);
    let body = format!(
        "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\nexport RUSTUP_AUTO_INSTALL=0\nexport MISE_AUTO_INSTALL=false\nexport MISE_EXEC_AUTO_INSTALL=false\n{}\n{}\n{}\n{}\nMISE_OFFLINE=true \"$mise\" {} reshim --force\n\"$mise\" {} install \"$@\"\n{}\n{}\n{}\n{}\nprintf '%s\\n' \"$CARGO_HOME/bin\" >> \"${{GITHUB_PATH:?missing runner path file}}\"\nif test -n \"${{GITHUB_OUTPUT:-}}\"; then printf 'verified=true\\n' >> \"$GITHUB_OUTPUT\"; fi\n",
        mise_verification(role, mise),
        crate::catalog::rust_cold::prepare_script(mise),
        bootstrap.script(RuntimePreparationPurpose::Full)?,
        health.prepare_script()?,
        MISE_GLOBAL_FLAGS.join(" "),
        MISE_GLOBAL_FLAGS.join(" "),
        mbx_verification(mbx, catalog.rust_uses_mbx())?,
        repair_script_for_host(host),
        health.finalize_script()?,
        wrapper_verification(&catalog)?,
    );
    velnor_actions_contract::generated_source(version, &body).map_err(super::contract)
}

pub(super) fn manager_verification(catalog: &super::ToolCatalog) -> String {
    let host = catalog.rust_host();
    format!(
        "export RUSTUP_AUTO_INSTALL=0\nexport MISE_AUTO_INSTALL=false\nexport MISE_EXEC_AUTO_INSTALL=false\ntest ! -L \"$CARGO_HOME\"\ntest ! -L \"$CARGO_HOME/bin\"\ntest ! -L \"$RUSTUP_HOME\"\ntest ! -L \"$RUSTUP_HOME/settings.toml\"\nmanager=\"$CARGO_HOME/bin/rustup\"\ntest -f \"$manager\"\ntest ! -L \"$manager\"\ntest -x \"$manager\"\ntest -f \"$RUSTUP_HOME/settings.toml\"\nprintf '%s  %s\\n' '{}' \"$manager\" | {}\n",
        host.sha256(),
        host.sha256_command(),
    )
}

pub(super) fn mbx_verification(
    mbx: Option<&super::QualifiedDistribution>,
    uses_wrapper: bool,
) -> Result<String, MiseError> {
    let Some(mbx) = mbx else {
        return Ok(String::new());
    };
    let relative = mbx
        .required_installed_binary_path()?
        .replace('\'', "'\"'\"'");
    let pair = if uses_wrapper {
        "export MISE_OWNED_CARGO_WRAPPER=\"$mbx\"\nexport MISE_OWNED_CARGO_WRAPPER_SHA256=\"$mbx_sha\"\n"
    } else {
        ""
    };
    Ok(format!(
        "cd -P \"$MISE_DATA_DIR\"\nowned_data=\"$PWD\"\nmbx=\"$owned_data\"/'{relative}'\ncd -P \"$temp/velnor\"\nmbx_sha='{}'\n{MBX_VERIFY}\n{pair}",
        mbx.binary_sha256()
    ))
}

pub(super) fn mise_verification(
    role: RustCompilerRole,
    mise: &super::QualifiedDistribution,
) -> String {
    let sha = mise.binary_sha256();
    format!(
        "{}\nsha='{sha}'\nprintf '%s  %s\\n' \"$sha\" \"$mise\" | {}\n{}\n",
        MISE_ROOTS,
        role.host().sha256_command(),
        VERIFIED_PATH,
    )
}

fn wrapper_verification(catalog: &super::ToolCatalog) -> Result<String, MiseError> {
    if !catalog.rust_uses_mbx() {
        return Ok(String::new());
    }
    Ok(format!(
        "\"$mise\" {} exec '{}' '{}' -- \"$CARGO_HOME/bin/rustup\" run '{}' rustc --version\n{}\n",
        MISE_GLOBAL_FLAGS.join(" "),
        catalog.tool_spec(catalog.compiler_tool())?,
        catalog.tool_spec(super::PinnedTool::MrBoxington)?,
        catalog.rust_toolchain_name(),
        WRAPPER_VERIFY,
    ))
}

const MISE_ROOTS: &str = r#"temp=${RUNNER_TEMP:?missing runner temp}
case "$temp" in /*) ;; *) exit 1 ;; esac
case "$temp/" in */../*|*/./*|*//*) exit 1 ;; esac
test "${MISE_DATA_DIR:?missing owned Mise data}" = "$temp/velnor/mise"
test "${CARGO_HOME:?missing owned Cargo home}" = "$temp/velnor/cargo"
test "${RUSTUP_HOME:?missing owned Rustup home}" = "$temp/velnor/rustup"
case "${1:?missing bootstrap domain}" in
  tools) mise_root="$temp/velnor/mise" ;;
  planning-bootstrap)
    test ! -L "$temp/velnor/planning"
    mise_root="$temp/velnor/planning/mise" ;;
  *) exit 1 ;;
esac
shift
for directory in "$temp" "$temp/velnor" "$mise_root" "$mise_root/bin"; do
  test -d "$directory"
  test ! -L "$directory"
done
test ! -L "$MISE_DATA_DIR"
mkdir -p "$MISE_DATA_DIR"
test -d "$MISE_DATA_DIR"
for directory in "$MISE_DATA_DIR/command-wrappers" "$MISE_DATA_DIR/command-wrappers/bin"; do
  test ! -L "$directory"
  if test -e "$directory"; then test -d "$directory"; fi
done
mise="$mise_root/bin/mise"
test -f "$mise"
test -x "$mise"
test ! -L "$mise"
cd -P "$mise_root/bin"
mise="$PWD/mise"
cd -P "$temp/velnor"
export MISE_CEILING_PATHS="$PWD"
export MISE_CONFIG_DIR="$temp/velnor/rust-prepare-config"
export MISE_SYSTEM_CONFIG_DIR="$temp/velnor/rust-prepare-system-config"
for directory in "$MISE_CONFIG_DIR" "$MISE_SYSTEM_CONFIG_DIR"; do
  test ! -L "$directory"
  mkdir -p "$directory"
  test -d "$directory"
  for entry in "$directory"/* "$directory"/.[!.]* "$directory"/..?*; do
    test ! -e "$entry"
    test ! -L "$entry"
  done
done"#;

pub(super) const WRAPPER_VERIFY: &str = r#"/usr/bin/python3 -I -S - "$MISE_DATA_DIR" "$mise" "$sha" <<'VELNOR_WRAPPER_VERIFY'
import hashlib, os, pathlib, stat, sys
home, binary, expected = sys.argv[1:]
home = pathlib.Path(home)
binary = pathlib.Path(binary)
farm = home / "command-wrappers"
for directory in (home, farm, farm / "bin"):
    if stat.S_ISLNK(directory.lstat().st_mode) or not directory.is_dir():
        raise ValueError("invalid owned command wrapper directory")
wrapper = farm / "bin" / "cargo"
info = wrapper.lstat()
if not stat.S_ISLNK(info.st_mode) or os.readlink(wrapper) != str(binary):
    raise ValueError("foreign cargo wrapper")
if hashlib.sha256(wrapper.read_bytes()).hexdigest() != expected:
    raise ValueError("unverified cargo wrapper bytes")
if not os.access(wrapper, os.X_OK):
    raise ValueError("nonexecutable cargo wrapper")
VELNOR_WRAPPER_VERIFY"#;

const MBX_VERIFY: &str = r#"/usr/bin/python3 -I -S - "$owned_data" "$mbx" "$mbx_sha" <<'VELNOR_MBX_VERIFY'
import hashlib, os, pathlib, stat, sys
home, executable, expected = map(str, sys.argv[1:])
home, executable = pathlib.Path(home), pathlib.Path(executable)
if home not in executable.parents or executable.resolve(strict=True) != executable:
    raise ValueError("noncanonical owned MBX path")
for directory in executable.parents:
    if stat.S_ISLNK(directory.lstat().st_mode) or not directory.is_dir():
        raise ValueError("linked owned MBX ancestor")
    if directory == home:
        break
info = executable.lstat()
if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o022 or not info.st_mode & 0o111:
    raise ValueError("invalid owned MBX executable")
if hashlib.sha256(executable.read_bytes()).hexdigest() != expected:
    raise ValueError("unverified owned MBX bytes")
VELNOR_MBX_VERIFY"#;

const VERIFIED_PATH: &str = r#"trusted_bin="$temp/velnor/rust-prepare-bin"
test ! -L "$trusted_bin"
mkdir -p "$trusted_bin"
test -d "$trusted_bin"
for entry in "$trusted_bin"/* "$trusted_bin"/.[!.]* "$trusted_bin"/..?*; do
  if test "$entry" = "$trusted_bin/mise"; then continue; fi
  test ! -e "$entry"
  test ! -L "$entry"
done
rm -f -- "$trusted_bin/mise"
ln -s -- "$mise" "$trusted_bin/mise"
export PATH="$trusted_bin:/usr/bin:/bin:/usr/sbin:/sbin""#;

pub(in crate::catalog) fn candidate_install_source(
    root: RootRustCandidateRoot,
    inputs: &RootRustCandidateSource,
    version: &str,
) -> Result<String, MiseError> {
    let manager = RustupBootstrap::for_host(root.host());
    let source = format!(
        "{CANDIDATE_PRELUDE}\n{CANDIDATE_ROOTS}\n{}\n{}\nmanager=\"$CARGO_HOME/bin/rustup\"\ntest -f \"$manager\"; test ! -L \"$manager\"; test -x \"$manager\"\nprintf '%s  %s\\n' '{}' \"$manager\" | {}\ntest ! -e \"$root/manager-bin\"; test ! -L \"$root/manager-bin\"\nmkdir -m 700 \"$root/manager-bin\"\nln -s -- \"$manager\" \"$root/manager-bin/rustup\"\nread -r RUSTUP_DIST_SERVER < <(/usr/bin/python3 -I -S -B -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).as_uri())' \"$root/native-dist\")\nexport RUSTUP_DIST_SERVER\nexport PATH=\"$root/manager-bin\"\n\"$manager\" toolchain install \"$RUSTUP_TOOLCHAIN\" --profile minimal --component clippy --component rustfmt --no-self-update\n",
        candidate_archive_source(inputs, true)?,
        manager.candidate_initialize_source(root)?,
        manager.sha256(),
        root.host().sha256_command(),
    );
    velnor_actions_contract::generated_source(version, &source).map_err(|error| {
        MiseError::Contract {
            problem: error.to_string(),
        }
    })
}

pub(in crate::catalog) fn candidate_archive_source(
    inputs: &RootRustCandidateSource,
    verify: bool,
) -> Result<String, MiseError> {
    let manifest = inputs.manifest();
    let config = serde_json::json!({
        "manifest": {"version": manifest.version(), "target": manifest.target(), "manifest_url": manifest.manifest_url(), "manifest_sha256": manifest.manifest_sha256(),
            "components": manifest.components().iter().map(|item| serde_json::json!({"component": item.component(), "xz_url": item.xz_url(), "xz_sha256": item.xz_sha256()})).collect::<Vec<_>>()},
        "payload": inputs.payload(),
    });
    let encoded =
        serde_json::to_string(&config.to_string()).map_err(|error| MiseError::Contract {
            problem: error.to_string(),
        })?;
    let extra = if verify {
        include_str!("catalog_root_rust_candidate_verify.py")
    } else {
        ""
    };
    let call = if verify {
        "candidate_verify_mirror()"
    } else {
        "candidate_acquire_archives()"
    };
    Ok(format!(
        "/usr/bin/python3 -I -S -B - <<'VELNOR_ROOT_RUST_ARCHIVES'\nimport json\nCONFIG = json.loads({encoded})\n{}\n{extra}\n{call}\nVELNOR_ROOT_RUST_ARCHIVES",
        include_str!("catalog_root_rust_candidate_archives.py")
    ))
}

pub(in crate::catalog) const CANDIDATE_PRELUDE: &str = "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\nexport RUSTUP_AUTO_INSTALL=0\numask 022\nunset RUSTUP_PERMIT_COPY_RENAME RUSTUP_DIST_SERVER RUSTUP_UPDATE_ROOT RUSTUP_USE_CURL RUSTUP_USE_RUSTLS LD_LIBRARY_PATH LD_PRELOAD LD_AUDIT DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH DYLD_INSERT_LIBRARIES";
pub(in crate::catalog) const CANDIDATE_ROOTS: &str = r#"temp=${RUNNER_TEMP:?missing runner temp}
root=${VELNOR_ROOT_RUST_CANDIDATE_ROOT:?missing candidate root}
test "$root" = "$temp/velnor-control/root-rust-candidate"
test "${CARGO_HOME:?missing Cargo home}" = "$root/cargo-home"
test "${RUSTUP_HOME:?missing Rustup home}" = "$root/rustup-home"
test "${RUSTUP_TOOLCHAIN:?missing exact toolchain}" = '1.98.1-x86_64-unknown-linux-gnu'
for directory in "$temp" "$temp/velnor-control" "$root"; do test -d "$directory"; test ! -L "$directory"; done
cd -P "$root""#;
