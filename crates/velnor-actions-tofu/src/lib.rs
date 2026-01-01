//! `OpenTofu` stack adapter: registration, roots, and dialect evidence.
//!
//! T09 owns strict minimal `[stacks.tofu]` roots plus STRONG / WEAK /
//! CONFLICT evidence. T10 adds the bounded structural parser (S8),
//! file families, effective precedence, independent fmt scope (S2),
//! and E4 content signals, and converts configured roots to detector
//! records, inventories, toolchain inputs, and closures. T11 adds
//! local-module edges (S6), H5 canonicalization, M2 identity, and
//! calling-root selection over the base/head union. T12 adds native
//! task proposals (kinds, IDs, identities, edges). T13 adds the fixed
//! per-kind payload argv plus env. This crate launches no processes,
//! builds no tool invocations, and renders no workflow text.

pub mod argv;
pub mod closure;
pub mod closure_inputs;
pub mod content;
pub mod detect;
pub mod diagnostics;
pub mod effective;
pub mod env;
pub mod evidence;
pub mod family;
pub mod file_cache;
pub mod fmt_scope;
pub mod identity;
pub mod kinds;
pub mod lockfile;
pub mod modules;
pub mod parser;
pub mod parser_json;
pub mod propose;
pub mod roots;
pub mod select;
pub mod task_identity;
pub mod units;
pub mod version;

pub use argv::{CHDIR_FINDING_TAG, chdir_finding_for_root, tofu_payload_argv};
pub use closure::resolve_closure_at_root;
pub use content::{ContentSignals, signals_for};
pub use detect::{detected_projects_for_units, discover_stack_candidates, manifest_for_unit_root};
pub use diagnostics::{
    LOCKFILE_MISSING, LOCKFILE_STALE, PROVIDER_DEPENDENCY_CHANGES,
    REQUIRED_VERSION_EXCLUDES_TOOLCHAIN, RequiredVersionClaim, lockfile_findings_for_root,
    remediation_for_init_stderr, require_committed_provider_lock, required_versions_for_root,
    version_compat_findings,
};
pub use effective::{Dialect, config_shape, dir_has_effective_config, effective_set};
pub use env::{
    DIR_DIGEST_HEX_CHARS, MAX_CLI_CONFIG_PATH_BYTES, MAX_DIR_SLUG_CHARS, TF_CLI_CONFIG_FILE_ENV,
    TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON, TF_INPUT_ENV, TF_INPUT_OFF,
    TF_PLUGIN_CACHE_DIR_ENV, tofu_cache_dir_under, tofu_cli_config, tofu_data_dir_under,
    tofu_isolation_env, tofu_payload_env, tofu_root_slug,
};
pub use evidence::{
    Advisory, Evidence, EvidenceLevel, MISE_OPENTOFU_TOOL, MISE_TERRAFORM_TOOL, TofuNote, classify,
    classify_with_contents, mise_tool_selected, plan_note,
};
pub use family::{Family, LOCKFILE_NAME, family_of, is_auto_var, is_override_stem};
pub use file_cache::{FileCache, PinnedOutcome};
pub use fmt_scope::{
    covered_fmt_roots, fmt_scope_for_root, fmt_set, is_excluded_name, is_fmt_file, under_hidden_dir,
};
pub use identity::{
    TofuGroupExtensionInputs, entry_metadata_for_task, extension_for_proposal, lock_slot_at_root,
};
pub use kinds::TofuTaskKind;
pub use lockfile::{
    LOCKFILE_CORRUPT, LOCKFILE_UNPINNED_HASHES, LockfileInspection, LockfileSpec, TofuLockSnapshot,
    inspect_lockfile, lock_digest_at_root, lock_slot_for_kind,
};
pub use modules::{
    ModuleDecl, ModuleEdge, ModuleEdges, ModuleError, ModuleFinding, ModuleRef, ModuleSource,
    RemoteKind, SourceClass, canonicalize_edges, check_acyclic, classify_literal,
    identities_digest, module_edge_pairs, qualify_module_edges, resolve_local_target, resolve_refs,
};
pub use parser::{
    BlockModel, FileModel, MAX_DEPTH, MAX_DIAGNOSTIC_CHARS, MAX_FILE_BYTES, MAX_FILES_PER_UNIT,
    MAX_NODES, ParseError, has_legacy_ref_text, parse_json, parse_native, strip_template_spans,
};
pub use propose::{
    KIND_DISPLAY_WORDS, TOFU_DRIVER, TOFU_PROFILE, TOFU_RUNNER, TofuTaskGroup, display_for_root,
    is_init_kind, is_validate_kind, key_for_root, payload_env_for_kind, propose_task,
    resource_class_for_kind, root_for_key, step_base_name, task_id_for_root, task_kind_rank,
};
pub use roots::qualify_roots;
pub use select::{RootSelection, SelectAllReason, select_roots};
pub use task_identity::{
    DigestSlot, ExtensionInputs, SlotState, TofuTaskIdentityExtension, provider_toolchain_entries,
    toolchain_inputs_for_task,
};
pub use units::{AnalyzedUnit, UnitError, analyze_files, files_for_prefix, module_refs_for_texts};
pub use version::{
    OPENTOFU_FLOOR, admits_opentofu, admits_version, is_terraform_only, toolchain_triple,
};

/// Stable identifier for the `OpenTofu` stack.
pub const STACK_ID: &str = "tofu";
