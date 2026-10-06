//! Verify restored Rust payloads before executing them; let Rustup repair damage.
//!
//! Rustup's installed component metadata does not checksum installed files.
//! A qualified installation records its complete owner-reported tree digest.
//! Missing or changed inventories require exact owner uninstall/reinstallation.

use super::rust_bootstrap::RustHost;
use super::rust_proxies::RUSTUP_VERSION;
use super::{RustInstallOptions, ToolCatalog};
use crate::MiseError;
use crate::inventory_loader::compiled_inventory_loader;

/// Cache identity boundary for the selected Rust toolchain payload inventory.
pub const RUST_HEALTH_POLICY: &str = "rustup-source-archive-tree-v2";
const SOURCE_ARCHIVE_INVENTORY_SCHEMA: &str = "2";

/// Exact Rustup toolchain and its typed installation obligations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustToolchainHealth {
    version: String,
    host: RustHost,
    options: RustInstallOptions,
}

impl RustToolchainHealth {
    /// Derive compiler and host together from the catalog's closed role.
    #[must_use]
    pub fn for_catalog(catalog: &ToolCatalog, options: RustInstallOptions) -> Self {
        Self {
            version: catalog.version(catalog.compiler_tool()).to_owned(),
            host: catalog.rust_host(),
            options,
        }
    }

    /// Before Mise install: retain verified state, otherwise uninstall by owner.
    ///
    /// Never runs a restored compiler to decide whether its bytes are healthy.
    /// # Errors
    /// Propagates failure to encode the compiler-owned inventory source closure.
    pub fn prepare_script(&self) -> Result<String, MiseError> {
        Ok(format!(
            "{} if ! velnor_rust_digest || ! velnor_rust_marker; then \
             printf 'Repairing unverified owned Rust toolchain: %s\\n' \"$toolchain\"; \
             \"$manager\" toolchain uninstall \"$toolchain\"; \
             rm -f -- \"$inventory\"; \
             fi",
            self.common_script(true)?,
        ))
    }

    /// After exact installation: verify obligations and persist the tree digest.
    ///
    /// Rustup verifies downloaded distribution archives. This local inventory
    /// detects accidental restore damage; it is not authentication against an
    /// attacker able to replace both cached payloads and their inventory.
    /// # Errors
    /// Propagates failure to encode the compiler-owned inventory source closure.
    pub fn finalize_script(&self) -> Result<String, MiseError> {
        Ok(format!(
            "{} {} {} \
             velnor_rust_digest; \
             test ! -L \"$inventory\"; \
             printf '%s:%s\\n' \"$binding\" \"$digest\" > \"$inventory\"",
            self.common_script(true)?,
            self.obligations_script(),
            self.probes_script(),
        ))
    }

    /// Verify authenticated restored state without repair or inventory writes.
    ///
    /// The caller must authenticate and verify the exact manager first.
    /// Payload and marker verification precedes every compiler invocation.
    /// # Errors
    /// Propagates failure to encode the compiler-owned inventory source closure.
    pub fn terminal_restore_script(&self) -> Result<String, MiseError> {
        Ok(format!(
            "{} velnor_rust_digest; velnor_rust_marker; {} {}",
            self.common_script(false)?,
            self.obligations_script(),
            self.probes_script(),
        ))
    }

    fn probes_script(&self) -> String {
        format!(
            "for command in rustc cargo rustdoc cargo-clippy clippy-driver rustfmt cargo-fmt; do \
               \"$manager\" run \"$toolchain\" \"$command\" --version; \
             done; \
             compiler=$(\"$manager\" run \"$toolchain\" rustc --version); \
             case \"$compiler\" in 'rustc {} '*) ;; *) exit 1 ;; esac;",
            self.version,
        )
    }

    fn common_script(&self, create_inventory: bool) -> Result<String, MiseError> {
        let inventory_source = compiled_inventory_loader()?;
        let inventory_code = python_expression(&format!("{inventory_source}\n{INVENTORY_ENTRY}"));
        let roots_code = python_expression(ROOTS_JSON_PYTHON);
        let inventory_directory = if create_inventory {
            "mkdir -p \"$RUSTUP_HOME/velnor-integrity\";"
        } else {
            "test -d \"$RUSTUP_HOME/velnor-integrity\";"
        };
        Ok(format!(
            "set -euo pipefail; \
             export RUSTUP_AUTO_INSTALL=0; \
             manager=\"$CARGO_HOME/bin/rustup\"; \
             toolchain='{}-{}'; \
             binding='{RUST_HEALTH_POLICY}:{SOURCE_ARCHIVE_INVENTORY_SCHEMA}:rustup-{RUSTUP_VERSION}:{}:{}'; \
             test ! -L \"$RUSTUP_HOME\"; \
             test ! -L \"$RUSTUP_HOME/velnor-integrity\"; \
             {inventory_directory} \
             inventory=\"$RUSTUP_HOME/velnor-integrity/$toolchain.sha256\"; \
             velnor_rust_marker() {{ \
               test -f \"$inventory\" && test ! -L \"$inventory\" || return 1; \
               printf '%s:%s\\n' \"$binding\" \"$digest\" | cmp -s - \"$inventory\"; \
             }}; \
             velnor_rust_digest() {{ \
               local roots root roots_json; \
               roots=$(\"$manager\" toolchain list --verbose) || return 1; \
               root=$(printf '%s\\n' \"$roots\" | awk -v name=\"$toolchain\" '$1 == name {{ sub(/^[^[:space:]]+[[:space:]]+/, \"\"); sub(/^\\([^)]*\\)[[:space:]]+/, \"\"); print }}') || return 1; \
               roots_json=$(/usr/bin/python3 -I -S -c {} \"$root\" \"$RUSTUP_HOME\") || return 1; \
               digest=$(/usr/bin/python3 -I -S -c {} \"$RUSTUP_HOME\" \"$roots_json\") || return 1; \
             }};",
            self.version,
            self.host.target_triple(),
            self.host.sha256(),
            self.options.pinned_spec(&self.version),
            roots_code,
            inventory_code,
        ))
    }

    fn obligations_script(&self) -> String {
        let mut components = vec!["cargo", "rustc", "rust-std"];
        components.extend(self.options.components().iter().map(String::as_str));
        let mut targets = vec![self.host.target_triple()];
        targets.extend(self.options.targets().iter().map(String::as_str));
        format!(
            "components=$(\"$manager\" component list --installed --toolchain \"$toolchain\"); \
             for component in {}; do \
               printf '%s\\n' \"$components\" | grep -Fx -- \"$component\" > /dev/null || \
               printf '%s\\n' \"$components\" | grep -Fx -- \"$component-{}\" > /dev/null; \
             done; \
             targets=$(\"$manager\" target list --installed --toolchain \"$toolchain\"); \
             for target in {}; do printf '%s\\n' \"$targets\" | grep -Fx -- \"$target\" > /dev/null; done;",
            components.join(" "),
            self.host.target_triple(),
            targets.join(" "),
        )
    }
}

/// Quote the fixed Python expression as one shell argument, including apostrophes.
fn python_expression(source: &str) -> String {
    let expression = format!("exec({source:?})");
    format!("'{}'", expression.replace('\'', "'\"'\"'"))
}

/// Owner root qualification only; the selected archive inventory owns tree observation.
const ROOTS_JSON_PYTHON: &str = r#"import json, os, pathlib, sys
try:
    root_arg, home_arg = sys.argv[1:]
    if not root_arg or os.path.islink(root_arg):
        raise ValueError("missing or linked owner root")
    root = pathlib.Path(root_arg).resolve(strict=True)
    home = pathlib.Path(home_arg).resolve(strict=True)
    if root == home or home not in root.parents or not root.is_dir():
        raise ValueError("owner root escaped home")
    print(json.dumps([root.relative_to(home).as_posix()]))
except (OSError, ValueError, RuntimeError):
    print("velnor: Rust owner root unavailable", file=sys.stderr)
    sys.exit(1)
"#;

/// Run the shared schema-2 archive inventory against the selected toolchain only.
///
/// The source archive projection remains deliberately unqualified for cache
/// publication. Rust health uses the registered source closure's strict
/// observation seam and never calls the original-filesystem observer.
const INVENTORY_ENTRY: &str = "import json, sys
from source_archive_inventory import _inventory
from source_archive_inventory_common import InventoryError
try:
    if len(sys.argv) != 3:
        raise InventoryError('payload_inventory_arguments')
    _root = sys.argv[1]
    _relative_roots = json.loads(sys.argv[2])
    if (type(_relative_roots) is not list or not _relative_roots
            or any(type(root) is not str for root in _relative_roots)):
        raise InventoryError('payload_inventory_roots')
    _context = {
        'schema': 2, 'root': _root, 'roots': tuple(_relative_roots),
        'purpose': 'payload-v1',
    }
    if (type(_context) is not dict or set(_context) != {'schema', 'root', 'roots', 'purpose'}
            or type(_context['schema']) is not int or _context['schema'] != 2
            or _context['root'] != _root or _context['roots'] != tuple(_relative_roots)
            or _context['purpose'] != 'payload-v1'):
        raise InventoryError('payload_inventory_context')
    _result = _inventory(_context['root'], _context['roots'])
    if json.loads(_result.canonical_bytes).get('schema') != 2:
        raise InventoryError('payload_inventory_schema')
    print(_result.digest)
except (InventoryError, OSError, ValueError, TypeError, KeyError):
    raise SystemExit(1) from None
";
