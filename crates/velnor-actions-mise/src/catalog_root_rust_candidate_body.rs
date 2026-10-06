//! Fixed native candidate sources; no installed authority or SDK is constructed here.

use crate::{
    MiseError,
    catalog::{rust_bootstrap::RustupBootstrap, rust_compiler_authority::RootRustCandidateSource},
    root_rust_candidate_root::RootRustCandidateRoot,
};

pub(super) fn clear(root: RootRustCandidateRoot, version: &str) -> Result<String, MiseError> {
    let source = format!(
        "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\n/usr/bin/python3 -I -S -B - <<'VELNOR_ROOT_RUST_CLEAR'\n{}\nROOT_RELATIVE = {:?}\nROOT_ENV = {:?}\n{}\nclear_root_rust_candidate()\nVELNOR_ROOT_RUST_CLEAR\n",
        include_str!("catalog_native_health.py"),
        root.relative_to_runner_temp(),
        root.namespace_environment().0,
        include_str!("root_rust_candidate_clear.py"),
    );
    marked(version, &source)
}

pub(super) fn acquire(
    root: RootRustCandidateRoot,
    inputs: &RootRustCandidateSource,
    version: &str,
) -> Result<String, MiseError> {
    let source = format!(
        "{prelude}\n{roots}\nfor leaf in cargo-home rustup-home rustup-bootstrap native-dist manager-bin; do test ! -e \"$root/$leaf\"; test ! -L \"$root/$leaf\"; done\n{}\n{}\n",
        crate::catalog::rust_prepare::candidate_archive_source(inputs, false)?,
        RustupBootstrap::for_host(root.host()).candidate_acquire_source(root)?,
        prelude = crate::catalog::rust_prepare::CANDIDATE_PRELUDE,
        roots = crate::catalog::rust_prepare::CANDIDATE_ROOTS,
    );
    marked(version, &source)
}

fn marked(version: &str, source: &str) -> Result<String, MiseError> {
    velnor_actions_contract::generated_source(version, source).map_err(|error| {
        MiseError::Contract {
            problem: error.to_string(),
        }
    })
}

pub(super) fn install(
    root: RootRustCandidateRoot,
    inputs: &RootRustCandidateSource,
    version: &str,
) -> Result<String, MiseError> {
    crate::catalog::rust_prepare::candidate_install_source(root, inputs, version)
}
