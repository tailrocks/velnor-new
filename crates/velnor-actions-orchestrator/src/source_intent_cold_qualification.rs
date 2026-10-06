//! Source-only RootRust candidate closure, never an installed SDK or runtime grant.

use serde_json::{Value, json};
use velnor_actions_contract::{canonical_json_str, compiled_source_sha256};
use velnor_actions_mise::source_archive_inventory::{InventorySourceProgram, fixed_sources};

use crate::{OrchestratorError, source_intent_cold_supplier::RootRustCandidateRecipe};

const LOADER: &str = r#"
import importlib.abc, importlib.util, sys
if any(name in sys.modules for name in _SOURCES):
    raise ImportError('root_candidate_owned_module_collision')
class _RootCandidateLoader(importlib.abc.MetaPathFinder, importlib.abc.Loader):
    def find_spec(self, fullname, path=None, target=None):
        if fullname in _SOURCES:
            return importlib.util.spec_from_loader(fullname, self, origin='<velnor-root-candidate>')
        if fullname.startswith(('source_intent_', 'source_root_rust_', 'source_archive_')):
            raise ImportError('root_candidate_source_closure_incomplete')
        return None
    def create_module(self, spec):
        return None
    def exec_module(self, module):
        source = _SOURCES[module.__name__]
        exec(compile(source, '<velnor-root-candidate:' + module.__name__ + '>', 'exec'), module.__dict__)
sys.meta_path.insert(0, _RootCandidateLoader())
"#;

const ENTRYPOINT: &str = r#"
import json
from source_root_rust_candidate_observe import observe_root_rust_candidate
observations = observe_root_rust_candidate()
if observations.get('purpose') != 'root-linux-compiler-candidate-observation':
    raise RuntimeError('root_candidate_observation_purpose')
print(json.dumps({'schema': 1, 'authority': False, 'observations': observations},
                 sort_keys=True, separators=(',', ':'), ensure_ascii=True, allow_nan=False))
"#;

/// Immutable payload. Foundation TYPE/GETTER stay unavailable, not caller supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootRustCandidateSourceClosure {
    recipe: RootRustCandidateRecipe,
    modules: Vec<(String, String)>,
    python_source: String,
    source_closure_sha256: String,
}

impl RootRustCandidateSourceClosure {
    /// Bind the distinct candidate owner, without production SDK qualification.
    pub(crate) fn root_linux(version: &str) -> Result<Self, OrchestratorError> {
        Self::from_owner(RootRustCandidateRecipe::root_linux(version)?)
    }

    fn from_owner(recipe: RootRustCandidateRecipe) -> Result<Self, OrchestratorError> {
        recipe.verify_fresh()?;
        let mut modules = candidate_sources();
        validate_sources(&modules)?;
        bind_recipe(&mut modules, &recipe.projection())?;
        let source_closure_sha256 = compiled_source_sha256(
            canonical_json_str(&json!({
                "schema": 1, "purpose": "root-rust-candidate-source-only",
                "modules": modules, "loader": LOADER, "entrypoint": ENTRYPOINT,
            }))?
            .as_bytes(),
        );
        let sources = modules
            .iter()
            .map(|(name, source)| (name.clone(), json!(source)))
            .collect::<serde_json::Map<String, Value>>();
        let encoded = hex(canonical_json_str(&Value::Object(sources))?.as_bytes());
        let python_source = format!(
            "import json\n_SOURCES = json.loads(bytes.fromhex('{encoded}'))\n{LOADER}\n{ENTRYPOINT}"
        );
        if python_source.contains("${{") {
            return Err(invalid("source_expression"));
        }
        Ok(Self {
            recipe,
            modules,
            python_source,
            source_closure_sha256,
        })
    }

    /// Source bytes only; launch needs a separate genuine same-process Foundation.
    pub(crate) fn python_source(&self) -> &str {
        &self.python_source
    }

    /// Full twenty-source and fixed loader/entrypoint transport identity.
    pub(crate) fn source_closure_sha256(&self) -> &str {
        &self.source_closure_sha256
    }

    /// Diagnostic source data, never installation or workflow execution authority.
    pub(crate) fn projection(&self) -> Value {
        json!({"schema": 1, "purpose": "root-rust-candidate-source-only", "authority": false,
            "recipe": self.recipe.projection(), "source_closure_sha256": self.source_closure_sha256,
            "modules": self.modules.iter().map(|(name, source)| json!({
                "module": name, "bytes": source.len(), "sha256": compiled_source_sha256(source.as_bytes()),
            })).collect::<Vec<_>>(), "foundation_issuer": "unavailable"})
    }

    /// Reconstruct every source byte and owner recipe before transport.
    pub(crate) fn verify_fresh(&self) -> Result<(), OrchestratorError> {
        if Self::from_owner(self.recipe.clone())? != *self {
            return Err(invalid("source_binding_changed"));
        }
        Ok(())
    }
}

fn candidate_sources() -> Vec<(String, String)> {
    let mut sources = vec![(
        "source_intent_cold_common".to_owned(),
        include_str!("source_intent_cold_common.py").to_owned(),
    )];
    sources.extend(
        fixed_sources(InventorySourceProgram::OriginalFilesystem)
            .into_iter()
            .map(|(name, source)| (name.to_owned(), source.to_owned())),
    );
    sources.extend(
        [
            (
                "source_intent_cold_compiler",
                include_str!("source_intent_cold_compiler.py"),
            ),
            (
                "source_intent_cold_recipe",
                include_str!("source_intent_cold_recipe.py"),
            ),
            (
                "source_intent_cold_foundation",
                include_str!("source_intent_cold_foundation.py"),
            ),
            (
                "source_intent_cold_process",
                include_str!("source_intent_cold_process.py"),
            ),
            (
                "source_intent_cold_install",
                include_str!("source_intent_cold_install.py"),
            ),
            (
                "source_root_rust_candidate_recipe",
                include_str!("source_root_rust_candidate_recipe.py"),
            ),
            (
                "source_root_rust_candidate_process",
                include_str!("source_root_rust_candidate_process.py"),
            ),
            (
                "source_root_rust_candidate_install",
                include_str!("source_root_rust_candidate_install.py"),
            ),
            (
                "source_root_rust_candidate_context",
                include_str!("source_root_rust_candidate_context.py"),
            ),
            (
                "source_root_rust_candidate_manifest",
                include_str!("source_root_rust_candidate_manifest.py"),
            ),
            (
                "source_root_rust_candidate_observe",
                include_str!("source_root_rust_candidate_observe.py"),
            ),
        ]
        .into_iter()
        .map(|(name, source)| (name.to_owned(), source.to_owned())),
    );
    sources
}

fn validate_sources(modules: &[(String, String)]) -> Result<(), OrchestratorError> {
    let expected = [
        "source_intent_cold_common",
        "source_archive_inventory_common",
        "metadata_container",
        "source_archive_inventory_fs",
        "opaque_inventory_metadata",
        "source_archive_inventory_leaf",
        "source_archive_inventory_walk",
        "source_archive_inventory",
        "source_archive_inventory_original",
        "source_intent_cold_compiler",
        "source_intent_cold_recipe",
        "source_intent_cold_foundation",
        "source_intent_cold_process",
        "source_intent_cold_install",
        "source_root_rust_candidate_recipe",
        "source_root_rust_candidate_process",
        "source_root_rust_candidate_install",
        "source_root_rust_candidate_context",
        "source_root_rust_candidate_manifest",
        "source_root_rust_candidate_observe",
    ];
    if modules.len() != expected.len()
        || modules
            .iter()
            .zip(expected)
            .any(|((name, source), expected)| name != expected || source.is_empty())
    {
        return Err(invalid("source_closure_incomplete"));
    }
    Ok(())
}

fn bind_recipe(modules: &mut [(String, String)], recipe: &Value) -> Result<(), OrchestratorError> {
    let (_, source) = modules
        .iter_mut()
        .find(|(name, _)| name == "source_root_rust_candidate_recipe")
        .ok_or_else(|| invalid("module_missing"))?;
    let needle = "_COMPILED_ROOT_RUST_CANDIDATE_RECIPE = None";
    if source.matches(needle).count() != 1 {
        return Err(invalid("recipe_slot_changed"));
    }
    let encoded = hex(canonical_json_str(recipe)?.as_bytes());
    *source = source.replace(needle, &format!(
        "_COMPILED_ROOT_RUST_CANDIDATE_RECIPE = __import__('json').loads(bytes.fromhex('{encoded}'))"
    ));
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)],
                DIGITS[usize::from(byte & 15)],
            ]
        })
        .map(char::from)
        .collect()
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("root_rust_candidate_closure_{problem}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_process_is_required_before_installation() {
        let modules = candidate_sources();
        assert_eq!(modules.len(), 20);
        assert!(validate_sources(&modules).is_ok());
        assert_eq!(modules[15].0, "source_root_rust_candidate_process");
        assert_eq!(modules[16].0, "source_root_rust_candidate_install");
        for index in 0..modules.len() {
            let mut missing = modules.clone();
            missing.remove(index);
            assert!(validate_sources(&missing).is_err());
        }
        let mut swapped = modules.clone();
        swapped.swap(15, 16);
        assert!(validate_sources(&swapped).is_err());
        let mut duplicate = modules.clone();
        duplicate[15] = duplicate[16].clone();
        assert!(validate_sources(&duplicate).is_err());
        let mut empty = modules;
        empty[15].1.clear();
        assert!(validate_sources(&empty).is_err());
    }

    #[test]
    fn missing_owned_module_never_falls_back_to_ambient_imports() {
        assert!(LOADER.contains("raise ImportError('root_candidate_source_closure_incomplete')"));
        assert!(LOADER.contains("if any(name in sys.modules for name in _SOURCES):"));
        assert!(LOADER.contains("raise ImportError('root_candidate_owned_module_collision')"));
        assert!(ENTRYPOINT.contains("observe_root_rust_candidate()"));
        assert!(ENTRYPOINT.contains("'authority': False"));
        assert!(!ENTRYPOINT.contains("source_intent_cold_sdk"));
    }
}
